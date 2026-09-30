use super::{begin_probe, cancelled_sample, timeout_sample};
use crate::{HopResult, ProbeError, ProbeIdentity, RoundParameters, Sample, SampleStatus};
use lantern_packet::{
    AddressFamily, IcmpKind, ProbeKey, TransportProtocol, build_icmpv4_echo, build_icmpv6_echo,
    parse_icmp_response,
};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::{
    io,
    mem::MaybeUninit,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    os::windows::io::AsRawSocket,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

type ReceivedPacket = Option<(Vec<u8>, Option<IpAddr>)>;

pub(super) fn probe_once(
    target: SocketAddr,
    identifier: u16,
    sequence: u16,
    ttl: Option<u8>,
    parameters: &RoundParameters,
    cancellation: &CancellationToken,
) -> Result<Sample, ProbeError> {
    let family = family(target.ip());
    let socket = RawSocket::new_icmp(family, parameters.timeout.min(Duration::from_millis(100)))?;
    if let Some(ttl) = ttl {
        socket.set_hop_limit(family, ttl)?;
    }
    socket.set_traffic_class(family, parameters.traffic_class)?;
    if parameters.dont_fragment && family == AddressFamily::Ipv4 {
        set_windows_option(&socket.socket, 0, 14, 1, "set IPv4 do-not-fragment")?;
    }
    let payload = vec![0x4c; parameters.payload_size];
    let packet = match target.ip() {
        IpAddr::V4(_) => build_icmpv4_echo(identifier, sequence, &payload),
        IpAddr::V6(destination) => {
            let IpAddr::V6(source) = source_for(target)? else {
                return Err(ProbeError::Io {
                    operation: "select IPv6 source",
                    message: "source family mismatch".into(),
                });
            };
            build_icmpv6_echo(source, destination, identifier, sequence, &payload)
        }
    };
    let started = Instant::now();
    begin_probe(cancellation, || socket.send_to(&packet, target))?;
    let key = ProbeKey::icmp(family, identifier, sequence);
    while started.elapsed() < parameters.timeout {
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        let received = socket.receive()?;
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        let Some((bytes, source)) = received else {
            continue;
        };
        let Ok(mut response) = parse_icmp_response(&bytes, family) else {
            continue;
        };
        if !response.correlates(&key) {
            continue;
        }
        if response.source.is_none() {
            response.source = source;
        }
        let status = match response.kind {
            IcmpKind::EchoReply => SampleStatus::Reply,
            IcmpKind::TimeExceeded => SampleStatus::TimeExceeded,
            _ => SampleStatus::Unreachable,
        };
        return Ok(Sample {
            sequence,
            hop: response.source,
            elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
            status,
            next_hop_mtu: response.next_hop_mtu,
            mpls: response.mpls_labels,
        });
    }
    Ok(timeout_sample(sequence))
}

pub fn udp_trace_scoped(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    base_port: u16,
    cancellation: &CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    validate_hops(first_ttl, parameters.max_hops)?;
    let family = family(target.ip());
    let sender = UdpSocket::bind(unspecified(family))
        .map_err(|error| io_error("bind UDP trace socket", error))?;
    let source_port = sender
        .local_addr()
        .map_err(|error| io_error("read UDP source port", error))?
        .port();
    let listener = RawSocket::new_icmp(family, parameters.timeout.min(Duration::from_millis(100)))?;
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        let reference = socket2::SockRef::from(&sender);
        match family {
            AddressFamily::Ipv4 => reference.set_ttl_v4(u32::from(ttl)),
            AddressFamily::Ipv6 => reference.set_unicast_hops_v6(u32::from(ttl)),
        }
        .map_err(|error| io_error("set UDP hop limit", error))?;
        set_socket_traffic_class(&reference, family, parameters.traffic_class)?;
        let port = base_port.checked_add(ttl.into()).ok_or_else(|| {
            ProbeError::InvalidPlan("UDP destination port range overflows".into())
        })?;
        let started = Instant::now();
        begin_probe(cancellation, || {
            sender
                .send_to(
                    &vec![0x4c; parameters.payload_size],
                    with_port(target, port),
                )
                .map_err(|error| io_error("send UDP trace probe", error))
        })?;
        let key = ProbeKey::ports(family, TransportProtocol::Udp, source_port, port);
        let mut observed = None;
        let mut cancelled = false;
        while started.elapsed() < parameters.timeout {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let received = listener.receive()?;
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let Some((bytes, source)) = received else {
                continue;
            };
            let Ok(mut response) = parse_icmp_response(&bytes, family) else {
                continue;
            };
            if !response.correlates(&key) {
                continue;
            }
            if response.source.is_none() {
                response.source = source;
            }
            let reached = response.kind == IcmpKind::DestinationUnreachable
                && response.source == Some(target.ip())
                && is_port_unreachable(family, response.code);
            observed = Some((
                Sample {
                    sequence: ttl.into(),
                    hop: response.source,
                    elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                    status: if reached {
                        SampleStatus::Reply
                    } else if response.kind == IcmpKind::TimeExceeded {
                        SampleStatus::TimeExceeded
                    } else {
                        SampleStatus::Unreachable
                    },
                    next_hop_mtu: response.next_hop_mtu,
                    mpls: response.mpls_labels,
                },
                reached,
            ));
            break;
        }
        let (sample, reached) = if cancelled {
            (cancelled_sample(ttl.into()), false)
        } else {
            observed.unwrap_or_else(|| (timeout_sample(ttl.into()), false))
        };
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination: reached,
            samples: vec![sample],
        });
        if reached || cancelled {
            break;
        }
    }
    Ok(hops)
}

pub fn tcp_trace_scoped(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    cancellation: &CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    validate_hops(first_ttl, parameters.max_hops)?;
    if destination_port == 0 {
        return Err(ProbeError::InvalidPlan(
            "TCP destination port must be positive".into(),
        ));
    }
    let family = family(target.ip());
    let source_port = 49_152u16 + rand::random::<u16>() % 16_383;
    let icmp = match RawSocket::new_icmp(family, Duration::from_millis(50)) {
        Ok(socket) => Some(socket),
        Err(ProbeError::Permission { .. } | ProbeError::Unsupported { .. }) => None,
        Err(error) => return Err(error),
    };
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        let connector = Socket::new(domain(family), Type::STREAM, Some(Protocol::TCP))
            .map_err(|error| io_error("open TCP trace socket", error))?;
        connector
            .bind(&SockAddr::from(with_port(unspecified(family), source_port)))
            .map_err(|error| io_error("bind TCP trace source port", error))?;
        connector
            .set_nonblocking(true)
            .map_err(|error| io_error("set TCP trace nonblocking", error))?;
        match family {
            AddressFamily::Ipv4 => connector.set_ttl_v4(ttl.into()),
            AddressFamily::Ipv6 => connector.set_unicast_hops_v6(ttl.into()),
        }
        .map_err(|error| io_error("set TCP trace hop limit", error))?;
        set_socket_traffic_class(&connector, family, parameters.traffic_class)?;
        let started = Instant::now();
        let destination = with_port(target, destination_port);
        let connect_result = begin_probe(cancellation, || {
            Ok(connector.connect(&SockAddr::from(destination)))
        })?;
        let immediately_reached = match connect_result {
            Ok(()) => true,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(10035) =>
            {
                false
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => true,
            Err(error) => return Err(io_error("start TCP trace connection", error)),
        };
        let key = ProbeKey::tcp(family, source_port, destination_port, None);
        let mut observed = None;
        let mut cancelled = cancellation.is_cancelled();
        if immediately_reached && !cancelled {
            observed = Some((reply_sample(ttl, target.ip(), started.elapsed()), true));
        }
        while !cancelled && observed.is_none() && started.elapsed() < parameters.timeout {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let icmp_packet = match &icmp {
                Some(icmp) => icmp.receive()?,
                None => {
                    std::thread::sleep(Duration::from_millis(5));
                    None
                }
            };
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            if let Some((bytes, source_address)) = icmp_packet
                && let Ok(mut response) = parse_icmp_response(&bytes, family)
                && response.correlates(&key)
            {
                if response.source.is_none() {
                    response.source = source_address;
                }
                observed = Some((
                    Sample {
                        sequence: ttl.into(),
                        hop: response.source,
                        elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                        status: if response.kind == IcmpKind::TimeExceeded {
                            SampleStatus::TimeExceeded
                        } else {
                            SampleStatus::Unreachable
                        },
                        next_hop_mtu: response.next_hop_mtu,
                        mpls: response.mpls_labels,
                    },
                    false,
                ));
            }
            if observed.is_some() {
                break;
            }
            let connect_error = connector
                .take_error()
                .map_err(|error| io_error("read TCP trace connection status", error))?;
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            if let Some(error) = connect_error {
                if error.kind() == io::ErrorKind::ConnectionRefused {
                    observed = Some((reply_sample(ttl, target.ip(), started.elapsed()), true));
                } else {
                    return Err(io_error("complete TCP trace connection", error));
                }
            } else if connector.peer_addr().is_ok() && !cancellation.is_cancelled() {
                observed = Some((reply_sample(ttl, target.ip(), started.elapsed()), true));
            }
            if observed.is_some() {
                break;
            }
        }
        let (sample, reached) = if cancelled {
            (cancelled_sample(ttl.into()), false)
        } else {
            observed.unwrap_or_else(|| (timeout_sample(ttl.into()), false))
        };
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination: reached,
            samples: vec![sample],
        });
        if reached || cancelled {
            break;
        }
    }
    Ok(hops)
}

pub fn tcp_trace_probe_scoped(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    identity: ProbeIdentity,
    cancellation: &CancellationToken,
) -> Result<HopResult, ProbeError> {
    validate_hops(ttl, parameters.max_hops)?;
    if destination_port == 0 {
        return Err(ProbeError::InvalidPlan(
            "TCP destination port must be positive".into(),
        ));
    }
    let family = family(target.ip());
    // Windows supplies the TCP sequence number, so a per-probe source port is
    // the correlation identity for quoted TCP headers.
    let source_port = 49_152 + (identity.ordinal % 16_383) as u16;
    let icmp = match RawSocket::new_icmp(family, Duration::from_millis(50)) {
        Ok(socket) => Some(socket),
        Err(ProbeError::Permission { .. } | ProbeError::Unsupported { .. }) => None,
        Err(error) => return Err(error),
    };
    let connector = Socket::new(domain(family), Type::STREAM, Some(Protocol::TCP))
        .map_err(|error| io_error("open TCP trace socket", error))?;
    connector
        .bind(&SockAddr::from(with_port(unspecified(family), source_port)))
        .map_err(|error| io_error("bind TCP trace source port", error))?;
    connector
        .set_nonblocking(true)
        .map_err(|error| io_error("set TCP trace nonblocking", error))?;
    match family {
        AddressFamily::Ipv4 => connector.set_ttl_v4(ttl.into()),
        AddressFamily::Ipv6 => connector.set_unicast_hops_v6(ttl.into()),
    }
    .map_err(|error| io_error("set TCP trace hop limit", error))?;
    set_socket_traffic_class(&connector, family, parameters.traffic_class)?;
    let started = Instant::now();
    let destination = with_port(target, destination_port);
    let connect_result = begin_probe(cancellation, || {
        Ok(connector.connect(&SockAddr::from(destination)))
    })?;
    let immediately_reached = match connect_result {
        Ok(()) => true,
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock || error.raw_os_error() == Some(10035) =>
        {
            false
        }
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => true,
        Err(error) => return Err(io_error("start TCP trace connection", error)),
    };
    if cancellation.is_cancelled() {
        let sample = cancelled_sample(ttl.into());
        return Ok(HopResult {
            ttl,
            address: None,
            reached_destination: false,
            samples: vec![sample],
        });
    }
    if immediately_reached {
        let sample = reply_sample(ttl, target.ip(), started.elapsed());
        return Ok(HopResult {
            ttl,
            address: sample.hop,
            reached_destination: true,
            samples: vec![sample],
        });
    }
    let key = ProbeKey::tcp(family, source_port, destination_port, None);
    while started.elapsed() < parameters.timeout {
        if cancellation.is_cancelled() {
            let sample = cancelled_sample(ttl.into());
            return Ok(HopResult {
                ttl,
                address: None,
                reached_destination: false,
                samples: vec![sample],
            });
        }
        let icmp_packet = match &icmp {
            Some(icmp) => icmp.receive()?,
            None => {
                std::thread::sleep(Duration::from_millis(5));
                None
            }
        };
        if cancellation.is_cancelled() {
            let sample = cancelled_sample(ttl.into());
            return Ok(HopResult {
                ttl,
                address: None,
                reached_destination: false,
                samples: vec![sample],
            });
        }
        if let Some((bytes, source_address)) = icmp_packet
            && let Ok(mut response) = parse_icmp_response(&bytes, family)
            && response.correlates(&key)
        {
            if response.source.is_none() {
                response.source = source_address;
            }
            let sample = Sample {
                sequence: ttl.into(),
                hop: response.source,
                elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                status: if response.kind == IcmpKind::TimeExceeded {
                    SampleStatus::TimeExceeded
                } else {
                    SampleStatus::Unreachable
                },
                next_hop_mtu: response.next_hop_mtu,
                mpls: response.mpls_labels,
            };
            return Ok(HopResult {
                ttl,
                address: sample.hop,
                reached_destination: false,
                samples: vec![sample],
            });
        }
        let connect_error = connector
            .take_error()
            .map_err(|error| io_error("read TCP trace connection status", error))?;
        if cancellation.is_cancelled() {
            let sample = cancelled_sample(ttl.into());
            return Ok(HopResult {
                ttl,
                address: None,
                reached_destination: false,
                samples: vec![sample],
            });
        }
        if let Some(error) = connect_error {
            if error.kind() == io::ErrorKind::ConnectionRefused {
                let sample = reply_sample(ttl, target.ip(), started.elapsed());
                return Ok(HopResult {
                    ttl,
                    address: sample.hop,
                    reached_destination: true,
                    samples: vec![sample],
                });
            }
            return Err(io_error("complete TCP trace connection", error));
        }
        if connector.peer_addr().is_ok() && !cancellation.is_cancelled() {
            let sample = reply_sample(ttl, target.ip(), started.elapsed());
            return Ok(HopResult {
                ttl,
                address: sample.hop,
                reached_destination: true,
                samples: vec![sample],
            });
        }
    }
    let sample = timeout_sample(ttl.into());
    Ok(HopResult {
        ttl,
        address: None,
        reached_destination: false,
        samples: vec![sample],
    })
}

struct RawSocket {
    socket: Socket,
}
impl RawSocket {
    fn new_icmp(family: AddressFamily, timeout: Duration) -> Result<Self, ProbeError> {
        Self::new(
            family,
            if family == AddressFamily::Ipv4 {
                Protocol::ICMPV4
            } else {
                Protocol::ICMPV6
            },
            timeout,
        )
    }
    fn new(
        family: AddressFamily,
        protocol: Protocol,
        timeout: Duration,
    ) -> Result<Self, ProbeError> {
        let socket = Socket::new(domain(family), Type::RAW, Some(protocol))
            .map_err(|error| io_error("open raw socket", error))?;
        socket
            .set_read_timeout(Some(timeout))
            .map_err(|error| io_error("set raw socket timeout", error))?;
        Ok(Self { socket })
    }
    fn set_hop_limit(&self, family: AddressFamily, ttl: u8) -> Result<(), ProbeError> {
        match family {
            AddressFamily::Ipv4 => self.socket.set_ttl_v4(ttl.into()),
            AddressFamily::Ipv6 => self.socket.set_unicast_hops_v6(ttl.into()),
        }
        .map_err(|error| io_error("set hop limit", error))
    }
    fn set_traffic_class(
        &self,
        family: AddressFamily,
        value: Option<u8>,
    ) -> Result<(), ProbeError> {
        set_socket_traffic_class(&self.socket, family, value)
    }
    fn send_to(&self, bytes: &[u8], target: SocketAddr) -> Result<(), ProbeError> {
        let sent = self
            .socket
            .send_to(bytes, &SockAddr::from(target))
            .map_err(|error| io_error("send raw probe", error))?;
        if sent == bytes.len() {
            Ok(())
        } else {
            Err(ProbeError::Io {
                operation: "send raw probe",
                message: "short write".into(),
            })
        }
    }
    fn receive(&self) -> Result<ReceivedPacket, ProbeError> {
        let mut buffer = vec![MaybeUninit::<u8>::uninit(); 65_535];
        match self.socket.recv_from(&mut buffer) {
            Ok((length, source)) => {
                let bytes =
                    unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) }
                        .to_vec();
                Ok(Some((
                    bytes,
                    source.as_socket().map(|address| address.ip()),
                )))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(io_error("receive raw response", error)),
        }
    }
}

fn set_socket_traffic_class(
    socket: &Socket,
    family: AddressFamily,
    value: Option<u8>,
) -> Result<(), ProbeError> {
    let Some(value) = value else {
        return Ok(());
    };
    match family {
        AddressFamily::Ipv4 => socket
            .set_tos_v4(value.into())
            .map_err(|error| io_error("set IPv4 traffic class", error)),
        AddressFamily::Ipv6 => {
            set_windows_option(socket, 41, 39, value.into(), "set IPv6 traffic class")
        }
    }
}
fn set_windows_option(
    socket: &Socket,
    level: i32,
    option: i32,
    value: i32,
    operation: &'static str,
) -> Result<(), ProbeError> {
    let status = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            level,
            option,
            (&raw const value).cast(),
            std::mem::size_of::<i32>() as i32,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(ProbeError::Io {
            operation,
            message: format!("WinSock error {}", unsafe { WSAGetLastError() }),
        })
    }
}
fn family(address: IpAddr) -> AddressFamily {
    if address.is_ipv4() {
        AddressFamily::Ipv4
    } else {
        AddressFamily::Ipv6
    }
}
fn domain(family: AddressFamily) -> Domain {
    if family == AddressFamily::Ipv4 {
        Domain::IPV4
    } else {
        Domain::IPV6
    }
}
fn unspecified(family: AddressFamily) -> SocketAddr {
    match family {
        AddressFamily::Ipv4 => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
        AddressFamily::Ipv6 => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
    }
}
fn validate_hops(first: u8, max: u8) -> Result<(), ProbeError> {
    if first == 0 || first > max {
        Err(ProbeError::InvalidPlan(
            "first TTL must be between 1 and max hops".into(),
        ))
    } else {
        Ok(())
    }
}
fn with_port(mut address: SocketAddr, port: u16) -> SocketAddr {
    address.set_port(port);
    address
}

fn source_for(mut target: SocketAddr) -> Result<IpAddr, ProbeError> {
    let socket = UdpSocket::bind(unspecified(family(target.ip())))
        .map_err(|error| io_error("select source address", error))?;
    target.set_port(9);
    socket
        .connect(target)
        .map_err(|error| io_error("select source address", error))?;
    socket
        .local_addr()
        .map(|value| value.ip())
        .map_err(|error| io_error("read source address", error))
}

fn is_port_unreachable(family: AddressFamily, code: u8) -> bool {
    matches!(
        (family, code),
        (AddressFamily::Ipv4, 3) | (AddressFamily::Ipv6, 4)
    )
}

fn reply_sample(ttl: u8, target: IpAddr, elapsed: Duration) -> Sample {
    Sample {
        sequence: ttl.into(),
        hop: Some(target),
        elapsed_ms: Some(elapsed.as_secs_f64() * 1000.0),
        status: SampleStatus::Reply,
        next_hop_mtu: None,
        mpls: Vec::new(),
    }
}
fn io_error(operation: &'static str, error: io::Error) -> ProbeError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        ProbeError::Permission {
            operation,
            message: error.to_string(),
        }
    } else {
        ProbeError::Io {
            operation,
            message: error.to_string(),
        }
    }
}

#[link(name = "Ws2_32")]
unsafe extern "system" {
    fn setsockopt(socket: usize, level: i32, option: i32, value: *const i8, length: i32) -> i32;
    fn WSAGetLastError() -> i32;
}
