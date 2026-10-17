use crate::{
    HopResult, HopStatistics, ProbeError, ProbeIdentity, RoundParameters, Sample, SampleStatus,
    Statistics,
};
#[cfg(not(windows))]
use crate::{NativeAccess, TcpTraceMode};
#[cfg(unix)]
use lantern_packet::{
    AddressFamily, IcmpKind, ParsedTcp, ProbeKey, TcpSynSpec, TransportProtocol, build_icmpv4_echo,
    build_icmpv6_echo, build_tcp_syn_ipv4, build_tcp_syn_ipv6, parse_icmp_response,
    parse_tcp_response,
};
#[cfg(unix)]
use std::time::Instant;
#[cfg(unix)]
use std::{
    io,
    mem::{self, MaybeUninit},
    net::{Ipv4Addr, Ipv6Addr, UdpSocket},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

mod pacing;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::probe_once;
#[cfg(windows)]
pub use windows::{native_access, tcp_trace_scoped, udp_trace_scoped};

#[cfg(windows)]
pub fn tcp_trace_probe_scoped(
    target: std::net::SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    identity: ProbeIdentity,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<HopResult, ProbeError> {
    windows::tcp_trace_probe_scoped(
        target,
        ttl,
        parameters,
        destination_port,
        identity,
        cancellation,
    )
}

#[cfg(windows)]
pub fn udp_trace(
    target: IpAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    base_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    windows::udp_trace_scoped(
        std::net::SocketAddr::new(target, 0),
        first_ttl,
        parameters,
        base_port,
        cancellation,
    )
}

#[cfg(windows)]
pub fn tcp_trace(
    target: IpAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    windows::tcp_trace_scoped(
        std::net::SocketAddr::new(target, 0),
        first_ttl,
        parameters,
        destination_port,
        cancellation,
    )
}

#[cfg(unix)]
type ReceivedPacket = Option<(Vec<u8>, Option<IpAddr>)>;

pub fn ping(
    target: IpAddr,
    count: u16,
    ttl: u8,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<Sample>, ProbeError> {
    ping_scoped(
        SocketAddr::new(target, 0),
        count,
        ttl,
        parameters,
        cancellation,
    )
}

pub fn ping_scoped(
    target: SocketAddr,
    count: u16,
    ttl: u8,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<Sample>, ProbeError> {
    ping_scoped_with_ttl(target, count, Some(ttl), parameters, cancellation)
}

pub fn ping_scoped_with_ttl(
    target: SocketAddr,
    count: u16,
    ttl: Option<u8>,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<Sample>, ProbeError> {
    if count == 0 {
        return Err(ProbeError::InvalidPlan(
            "ping count must be positive".into(),
        ));
    }
    let identifier = rand::random::<u16>();
    ping_with_probe(
        count,
        Duration::from_secs(1),
        cancellation,
        |sequence, token| probe_once(target, identifier, sequence, ttl, parameters, token),
    )
}

fn ping_with_probe<F>(
    count: u16,
    interval: Duration,
    cancellation: &tokio_util::sync::CancellationToken,
    probe: F,
) -> Result<Vec<Sample>, ProbeError>
where
    F: FnMut(u16, &tokio_util::sync::CancellationToken) -> Result<Sample, ProbeError>,
{
    let outcome = pacing::run_sequential(
        (0..count).map(|sequence| (usize::from(sequence), sequence)),
        interval,
        cancellation,
        probe,
    );
    let samples = outcome
        .completed
        .into_iter()
        .map(|(_, sample)| sample)
        .collect::<Vec<_>>();
    match outcome.failure {
        None => Ok(samples),
        Some(ProbeError::Cancelled) if !samples.is_empty() => Ok(samples),
        Some(error) => Err(error),
    }
}

pub fn trace(
    target: IpAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    trace_scoped(
        SocketAddr::new(target, 0),
        first_ttl,
        parameters,
        cancellation,
    )
}

pub fn trace_scoped(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    if first_ttl == 0 || first_ttl > parameters.max_hops {
        return Err(ProbeError::InvalidPlan(
            "first TTL must be between 1 and max hops".into(),
        ));
    }
    let identifier = rand::random::<u16>();
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        let sample = match probe_once(
            target,
            identifier,
            u16::from(ttl),
            Some(ttl),
            parameters,
            cancellation,
        ) {
            Ok(sample) => sample,
            Err(ProbeError::Cancelled) if !hops.is_empty() => break,
            Err(error) => return Err(error),
        };
        let reached_destination = sample.status == SampleStatus::Reply;
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination,
            samples: vec![sample],
        });
        if reached_destination {
            break;
        }
    }
    Ok(hops)
}

pub fn trace_probe_scoped(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    identity: ProbeIdentity,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<HopResult, ProbeError> {
    let identifier = (identity.nonce ^ identity.ordinal.rotate_left(13)) as u16;
    let sequence = identity.ordinal as u16;
    let sample = probe_once(
        target,
        identifier,
        sequence,
        Some(ttl),
        parameters,
        cancellation,
    )?;
    Ok(HopResult {
        ttl,
        address: sample.hop,
        reached_destination: sample.status == SampleStatus::Reply,
        samples: vec![sample],
    })
}

pub fn udp_trace_probe_scoped(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    base_port: u16,
    identity: ProbeIdentity,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<HopResult, ProbeError> {
    let available = u32::from(u16::MAX - base_port) + 1;
    let destination_port = u32::from(base_port) + identity.ordinal % available;
    if destination_port <= u32::from(ttl) {
        return Err(ProbeError::InvalidPlan(
            "UDP base port cannot encode the probe identity".into(),
        ));
    }
    let adjusted_base = (destination_port - u32::from(ttl)) as u16;
    let mut adjusted = parameters.clone();
    adjusted.max_hops = ttl;
    let mut hops = udp_trace_scoped(target, ttl, &adjusted, adjusted_base, cancellation)?;
    hops.pop().ok_or_else(|| ProbeError::Io {
        operation: "UDP trace probe",
        message: "probe produced no hop result".into(),
    })
}

pub fn measure_hops(
    target: IpAddr,
    trace: &[HopResult],
    probe_count: u16,
    timeout: Duration,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopStatistics>, ProbeError> {
    measure_hops_scoped(
        SocketAddr::new(target, 0),
        trace,
        probe_count,
        timeout,
        parameters,
        cancellation,
    )
}

pub fn measure_hops_scoped(
    target: SocketAddr,
    trace: &[HopResult],
    probe_count: u16,
    timeout: Duration,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopStatistics>, ProbeError> {
    if probe_count == 0 {
        return Err(ProbeError::InvalidPlan(
            "path statistics probe count must be positive".into(),
        ));
    }
    let identifier = rand::random::<u16>();
    let mut adjusted = parameters.clone();
    adjusted.timeout = timeout;
    measure_hops_with_probe(
        trace,
        probe_count,
        Duration::from_millis(250),
        64,
        cancellation,
        |ttl, sequence, token| {
            probe_once(target, identifier, sequence, Some(ttl), &adjusted, token)
        },
    )
}

fn measure_hops_with_probe<F>(
    trace: &[HopResult],
    probe_count: u16,
    interval: Duration,
    max_outstanding: usize,
    cancellation: &tokio_util::sync::CancellationToken,
    probe: F,
) -> Result<Vec<HopStatistics>, ProbeError>
where
    F: Fn(u8, u16, &tokio_util::sync::CancellationToken) -> Result<Sample, ProbeError> + Sync,
{
    // PathPing probes each hop in turn, with 250 ms between sends. Outstanding
    // responses must not serialize the next send behind their timeout.
    let mut samples: Vec<Vec<Sample>> = trace.iter().map(|_| Vec::new()).collect();
    let jobs = (0..probe_count).flat_map(|index| {
        trace.iter().enumerate().map(move |(hop_index, hop)| {
            let sequence = u16::from(hop.ttl)
                .wrapping_mul(probe_count)
                .wrapping_add(index);
            (hop_index, (hop.ttl, sequence))
        })
    });
    let outcome = pacing::run_concurrent(
        jobs,
        interval,
        max_outstanding,
        cancellation,
        |(ttl, sequence), token| probe(ttl, sequence, token),
    );
    for (owner, sample) in outcome.completed {
        samples[owner].push(sample);
    }
    if let Some(error) = outcome.failure
        && (!matches!(error, ProbeError::Cancelled) || samples.iter().all(std::vec::Vec::is_empty))
    {
        return Err(error);
    }
    Ok(trace
        .iter()
        .zip(samples)
        .map(|(hop, mut samples)| {
            samples.sort_by_key(|sample| sample.sequence);
            HopStatistics {
                ttl: hop.ttl,
                address: samples.iter().find_map(|sample| sample.hop).or(hop.address),
                statistics: Statistics::from_samples(&samples),
            }
        })
        .collect())
}

/// Trace with kernel-generated UDP datagrams. A raw (or, on BSD/macOS,
/// datagram) ICMP socket observes the responses; unprivileged Linux reads them
/// from the sender's `IP_RECVERR` error queue instead. The destination port
/// identifies each quoted datagram.
#[cfg(unix)]
pub fn udp_trace(
    target: IpAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    base_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    udp_trace_scoped(
        SocketAddr::new(target, 0),
        first_ttl,
        parameters,
        base_port,
        cancellation,
    )
}

#[cfg(unix)]
pub fn udp_trace_scoped(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    base_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    if first_ttl == 0 || first_ttl > parameters.max_hops {
        return Err(ProbeError::InvalidPlan(
            "first TTL must be between 1 and max hops".into(),
        ));
    }
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let bind_address = match family {
        AddressFamily::Ipv4 => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        AddressFamily::Ipv6 => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let sender = UdpSocket::bind(bind_address).map_err(|error| ProbeError::Io {
        operation: "bind UDP trace socket",
        message: error.to_string(),
    })?;
    let source_port = sender
        .local_addr()
        .map_err(|error| ProbeError::Io {
            operation: "read UDP trace source port",
            message: error.to_string(),
        })?
        .port();
    let listener = udp_trace_listener(family, parameters.timeout.min(Duration::from_millis(100)))?;
    #[cfg(target_os = "linux")]
    if listener.is_none() {
        enable_error_queue(sender.as_raw_fd(), family)?;
    }
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        #[cfg(target_os = "linux")]
        if listener.is_none() {
            discard_error_queue(sender.as_raw_fd())?;
        }
        set_datagram_hop_limit(&sender, family, ttl)?;
        set_datagram_traffic_class(&sender, family, parameters.traffic_class)?;
        let destination_port = base_port.checked_add(u16::from(ttl)).ok_or_else(|| {
            ProbeError::InvalidPlan("UDP destination port range overflows".into())
        })?;
        let mut destination = target;
        destination.set_port(destination_port);
        let payload = vec![0x4c; parameters.payload_size];
        let started = Instant::now();
        let outcome = begin_probe(cancellation, || {
            match sender.send_to(&payload, destination) {
                Ok(_) => Ok(SendOutcome::Sent),
                Err(error) if is_route_error(&error) => Ok(SendOutcome::Unreachable),
                Err(error) => Err(ProbeError::Io {
                    operation: "send UDP trace probe",
                    message: error.to_string(),
                }),
            }
        })?;
        let key = ProbeKey::ports(
            family,
            TransportProtocol::Udp,
            source_port,
            destination_port,
        );
        let mut observed = (outcome == SendOutcome::Unreachable)
            .then(|| (unreachable_sample(u16::from(ttl)), false));
        let mut cancelled = false;
        let deadline = started + parameters.timeout;
        while observed.is_none() && Instant::now() < deadline {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let mut fds = [match &listener {
                Some(listener) => pollfd(listener.fd.as_raw_fd(), libc::POLLIN),
                // Queued ICMP errors raise POLLERR without requesting events.
                None => pollfd(sender.as_raw_fd(), 0),
            }];
            poll_sockets(&mut fds, deadline)?;
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            if let Some(listener) = &listener {
                while let Some((bytes, source)) = listener.receive()? {
                    let Ok(mut response) = parse_icmp_response(&bytes, family) else {
                        continue;
                    };
                    if !response.correlates(&key) {
                        continue;
                    }
                    if response.source.is_none() {
                        response.source = source;
                    }
                    observed = Some(udp_trace_sample(
                        ttl,
                        target,
                        response.kind,
                        response.code,
                        response.source,
                        started,
                        response.next_hop_mtu,
                        response.mpls_labels,
                    ));
                    break;
                }
            }
            #[cfg(target_os = "linux")]
            if listener.is_none() {
                while let Some(queued) = receive_error_queue(sender.as_raw_fd())? {
                    let Some(kind) = queued.kind() else {
                        continue;
                    };
                    if !queued.destination.is_some_and(|quoted| {
                        quoted.ip() == target.ip() && quoted.port() == destination_port
                    }) {
                        continue;
                    }
                    observed = Some(udp_trace_sample(
                        ttl,
                        target,
                        kind,
                        queued.code,
                        queued.offender,
                        started,
                        queued.next_hop_mtu(),
                        Vec::new(),
                    ));
                    break;
                }
            }
        }
        let (sample, reached_destination) = if cancelled {
            (cancelled_sample(u16::from(ttl)), false)
        } else {
            observed.unwrap_or_else(|| (timeout_sample(u16::from(ttl)), false))
        };
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination,
            samples: vec![sample],
        });
        if reached_destination || cancelled {
            break;
        }
    }
    Ok(hops)
}

/// Build a UDP trace sample; port unreachable from the target is arrival.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn udp_trace_sample(
    ttl: u8,
    target: SocketAddr,
    kind: IcmpKind,
    code: u8,
    hop: Option<IpAddr>,
    started: Instant,
    next_hop_mtu: Option<u32>,
    mpls: Vec<lantern_packet::MplsLabel>,
) -> (Sample, bool) {
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let reached = kind == IcmpKind::DestinationUnreachable
        && hop == Some(target.ip())
        && matches!(
            (family, code),
            (AddressFamily::Ipv4, 3) | (AddressFamily::Ipv6, 4)
        );
    let mut sample = icmp_sample(u16::from(ttl), kind, hop, started, next_hop_mtu, mpls);
    if reached {
        sample.status = SampleStatus::Reply;
    }
    (sample, reached)
}

/// The ICMP listener for UDP traces. Linux datagram ICMP sockets never see
/// errors for other sockets, so unprivileged Linux uses the sender's error
/// queue instead (`None`).
#[cfg(target_os = "linux")]
fn udp_trace_listener(
    family: AddressFamily,
    timeout: Duration,
) -> Result<Option<RawIcmpSocket>, ProbeError> {
    match RawIcmpSocket::new(family, timeout) {
        Ok(socket) if socket.kind == SocketKind::Raw => Ok(Some(socket)),
        Ok(_) | Err(ProbeError::Permission { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn udp_trace_listener(
    family: AddressFamily,
    timeout: Duration,
) -> Result<Option<RawIcmpSocket>, ProbeError> {
    RawIcmpSocket::new(family, timeout).map(Some)
}

/// Trace TCP. Linux with raw socket access sends hand-built SYN packets and
/// observes SYN-ACK or RST without completing a handshake. macOS/BSD (whose
/// kernels never deliver TCP segments to raw sockets) and unprivileged Linux
/// use one kernel `connect()` per probe instead: the destination then sees a
/// completed handshake immediately followed by RST.
#[cfg(unix)]
pub fn tcp_trace(
    target: IpAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    tcp_trace_scoped(
        SocketAddr::new(target, 0),
        first_ttl,
        parameters,
        destination_port,
        cancellation,
    )
}

/// See [`tcp_trace`] for the raw and connect modes.
#[cfg(unix)]
pub fn tcp_trace_scoped(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    if first_ttl == 0 || first_ttl > parameters.max_hops || destination_port == 0 {
        return Err(ProbeError::InvalidPlan(
            "invalid TCP trace hop or port settings".into(),
        ));
    }
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    match raw_tcp_sender(family)? {
        Some(sender) => tcp_trace_raw(
            target,
            first_ttl,
            parameters,
            destination_port,
            &sender,
            cancellation,
        ),
        None => {
            log_connect_mode();
            tcp_trace_connect(
                target,
                first_ttl,
                parameters,
                destination_port,
                cancellation,
            )
        }
    }
}

/// See [`tcp_trace`] for the raw and connect modes.
#[cfg(unix)]
pub fn tcp_trace_probe_scoped(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    identity: ProbeIdentity,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<HopResult, ProbeError> {
    if ttl == 0 || ttl > parameters.max_hops || destination_port == 0 {
        return Err(ProbeError::InvalidPlan(
            "invalid TCP trace hop or port settings".into(),
        ));
    }
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let sample = match raw_tcp_sender(family)? {
        Some(sender) => tcp_trace_probe_raw(
            target,
            ttl,
            parameters,
            destination_port,
            identity,
            &sender,
            cancellation,
        )?,
        None => {
            log_connect_mode();
            let listener = connect_icmp_listener(family, parameters.timeout)?;
            tcp_connect_probe(
                target,
                ttl,
                parameters,
                destination_port,
                listener.as_ref(),
                cancellation,
            )?
        }
    };
    Ok(HopResult {
        ttl,
        address: sample.hop,
        reached_destination: sample.status == SampleStatus::Reply,
        samples: vec![sample],
    })
}

#[cfg(unix)]
fn log_connect_mode() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| log::info!("TCP trace uses connect() probes"));
}

/// The raw IP sender for raw TCP traces, or `None` for connect mode.
#[cfg(target_os = "linux")]
fn raw_tcp_sender(family: AddressFamily) -> Result<Option<RawIpSocket>, ProbeError> {
    match RawIpSocket::new(family) {
        Ok(sender) => Ok(Some(sender)),
        Err(ProbeError::Permission { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// BSD/macOS kernels never deliver TCP segments to raw sockets, so a raw SYN
/// trace cannot observe the destination; connect mode is always used.
#[cfg(all(unix, not(target_os = "linux")))]
fn raw_tcp_sender(_family: AddressFamily) -> Result<Option<RawIpSocket>, ProbeError> {
    Ok(None)
}

/// The ICMP listener for connect-mode TCP probes. Linux reads the TCP socket's
/// own error queue instead (`None`).
#[cfg(target_os = "linux")]
fn connect_icmp_listener(
    _family: AddressFamily,
    _timeout: Duration,
) -> Result<Option<RawIcmpSocket>, ProbeError> {
    Ok(None)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn connect_icmp_listener(
    family: AddressFamily,
    timeout: Duration,
) -> Result<Option<RawIcmpSocket>, ProbeError> {
    RawIcmpSocket::new(family, timeout.min(Duration::from_millis(50))).map(Some)
}

#[cfg(unix)]
fn tcp_trace_connect(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let listener = connect_icmp_listener(family, parameters.timeout)?;
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        let sample = tcp_connect_probe(
            target,
            ttl,
            parameters,
            destination_port,
            listener.as_ref(),
            cancellation,
        )?;
        let reached_destination = sample.status == SampleStatus::Reply;
        let cancelled = sample.status == SampleStatus::Cancelled;
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination,
            samples: vec![sample],
        });
        if reached_destination || cancelled {
            break;
        }
    }
    Ok(hops)
}

/// One connect-mode TCP probe. SYN-ACK (connection established) or RST
/// (connection refused) mark the destination; closing with a zero linger
/// timeout then sends RST, so the destination sees a completed handshake
/// followed by RST. ICMP errors quoting the probe's ports identify routers.
#[cfg(unix)]
fn tcp_connect_probe(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    icmp_listener: Option<&RawIcmpSocket>,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Sample, ProbeError> {
    let sequence = u16::from(ttl);
    let (family, domain, hop_option, class_option) = match target.ip() {
        IpAddr::V4(_) => (
            AddressFamily::Ipv4,
            libc::AF_INET,
            (libc::IPPROTO_IP, libc::IP_TTL),
            (libc::IPPROTO_IP, libc::IP_TOS),
        ),
        IpAddr::V6(_) => (
            AddressFamily::Ipv6,
            libc::AF_INET6,
            (libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS),
            (libc::IPPROTO_IPV6, libc::IPV6_TCLASS),
        ),
    };
    let socket = open_socket(
        domain,
        libc::SOCK_STREAM,
        libc::IPPROTO_TCP,
        "open TCP trace socket",
    )?;
    let fd = socket.as_raw_fd();
    set_nonblocking(fd)?;
    let hops = libc::c_int::from(ttl);
    set_option(
        fd,
        hop_option.0,
        hop_option.1,
        &hops,
        "set TCP probe hop limit",
    )?;
    if let Some(traffic_class) = parameters.traffic_class {
        let value = libc::c_int::from(traffic_class);
        set_option(
            fd,
            class_option.0,
            class_option.1,
            &value,
            "set TCP probe traffic class",
        )?;
    }
    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    set_option(
        fd,
        libc::SOL_SOCKET,
        libc::SO_LINGER,
        &linger,
        "set TCP probe linger",
    )?;
    #[cfg(target_os = "linux")]
    if icmp_listener.is_none() {
        enable_error_queue(fd, family)?;
    }
    let mut destination = target;
    destination.set_port(destination_port);
    let (storage, length) = socket_address_with_port(destination);
    let started = Instant::now();
    let connected = begin_probe(cancellation, || {
        let status = unsafe { libc::connect(fd, (&raw const storage).cast(), length) };
        Ok(if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        })
    })?;
    match connected {
        Ok(()) => return Ok(reply_sample(sequence, target.ip(), started)),
        Err(error) => match error.raw_os_error() {
            // An interrupted connect continues asynchronously.
            Some(libc::EINPROGRESS | libc::EINTR) => {}
            Some(libc::ECONNREFUSED) => return Ok(reply_sample(sequence, target.ip(), started)),
            _ if is_route_error(&error) => return Ok(unreachable_sample(sequence)),
            _ => return Err(socket_error("start TCP trace connection", error)),
        },
    }
    let source_port = local_port(fd, "read TCP trace source port")?;
    let key = ProbeKey::ports(
        family,
        TransportProtocol::Tcp,
        source_port,
        destination_port,
    );
    let deadline = started + parameters.timeout;
    loop {
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        if Instant::now() >= deadline {
            return Ok(timeout_sample(sequence));
        }
        let mut fds = [
            pollfd(fd, libc::POLLOUT),
            pollfd(
                icmp_listener.map_or(-1, |listener| listener.fd.as_raw_fd()),
                libc::POLLIN,
            ),
        ];
        poll_sockets(&mut fds, deadline)?;
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        if let Some(listener) = icmp_listener {
            while let Some((bytes, source)) = listener.receive()? {
                if let Ok(mut response) = parse_icmp_response(&bytes, family)
                    && response.correlates(&key)
                {
                    if response.source.is_none() {
                        response.source = source;
                    }
                    return Ok(icmp_sample(
                        sequence,
                        response.kind,
                        response.source,
                        started,
                        response.next_hop_mtu,
                        response.mpls_labels,
                    ));
                }
            }
        }
        if fds[0].revents & (libc::POLLOUT | libc::POLLERR | libc::POLLHUP) == 0 {
            continue;
        }
        #[cfg(target_os = "linux")]
        while let Some(queued) = receive_error_queue(fd)? {
            if let Some(kind) = queued.kind() {
                return Ok(icmp_sample(
                    sequence,
                    kind,
                    queued.offender,
                    started,
                    queued.next_hop_mtu(),
                    Vec::new(),
                ));
            }
        }
        match pending_socket_error(fd)? {
            0 | libc::ECONNREFUSED => return Ok(reply_sample(sequence, target.ip(), started)),
            code => {
                let error = io::Error::from_raw_os_error(code);
                if is_route_error(&error) {
                    return Ok(unreachable_sample(sequence));
                }
                return Err(socket_error("complete TCP trace connection", error));
            }
        }
    }
}

/// Raw SYN trace (Linux with raw socket access). ICMP quotations identify
/// intermediate hops; SYN-ACK or RST packets identify the destination without
/// completing a TCP handshake.
#[cfg(unix)]
fn tcp_trace_raw(
    target: SocketAddr,
    first_ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    sender: &RawIpSocket,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let source = source_for(target)?;
    let source_port = 49_152u16.wrapping_add(rand::random::<u16>() % 16_383);
    let listeners = RawTcpListeners::new(family, parameters.timeout)?;
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
        }
        let sequence = rand::random::<u32>();
        let spec = TcpSynSpec {
            source_port,
            destination_port,
            sequence,
            hop_limit: ttl,
            traffic_class: parameters.traffic_class.unwrap_or(0),
            dont_fragment: parameters.dont_fragment,
        };
        let packet = build_raw_syn(source, target.ip(), &spec)?;
        let started = Instant::now();
        let sample = match begin_probe(cancellation, || sender.send_to(&packet, target))? {
            SendOutcome::Unreachable => unreachable_sample(ttl.into()),
            SendOutcome::Sent => listeners.wait(
                ttl,
                &ProbeKey::tcp(family, source_port, destination_port, Some(sequence)),
                started + parameters.timeout,
                started,
                |response| {
                    response.source_port == destination_port
                        && response.destination_port == source_port
                        && (response.rst
                            || (response.syn
                                && response.ack
                                && response.acknowledgment == sequence.wrapping_add(1)))
                },
                cancellation,
            )?,
        };
        let reached_destination = sample.status == SampleStatus::Reply;
        let cancelled = sample.status == SampleStatus::Cancelled;
        hops.push(HopResult {
            ttl,
            address: sample.hop,
            reached_destination,
            samples: vec![sample],
        });
        if reached_destination || cancelled {
            break;
        }
    }
    Ok(hops)
}

#[cfg(unix)]
fn tcp_trace_probe_raw(
    target: SocketAddr,
    ttl: u8,
    parameters: &RoundParameters,
    destination_port: u16,
    identity: ProbeIdentity,
    sender: &RawIpSocket,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Sample, ProbeError> {
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let source = source_for(target)?;
    let source_port = 49_152 + (identity.ordinal % 16_383) as u16;
    let sequence = identity.nonce.rotate_left(identity.ordinal % 31) ^ identity.ordinal;
    let spec = TcpSynSpec {
        source_port,
        destination_port,
        sequence,
        hop_limit: ttl,
        traffic_class: parameters.traffic_class.unwrap_or(0),
        dont_fragment: parameters.dont_fragment,
    };
    let packet = build_raw_syn(source, target.ip(), &spec)?;
    let listeners = RawTcpListeners::new(family, parameters.timeout)?;
    let started = Instant::now();
    if begin_probe(cancellation, || sender.send_to(&packet, target))? == SendOutcome::Unreachable {
        return Ok(unreachable_sample(ttl.into()));
    }
    listeners.wait(
        ttl,
        &ProbeKey::tcp(family, source_port, destination_port, Some(sequence)),
        started + parameters.timeout,
        started,
        |response| {
            response.source_port == destination_port
                && response.destination_port == source_port
                && response.ack
                && response.acknowledgment == sequence.wrapping_add(1)
                && (response.rst || response.syn)
        },
        cancellation,
    )
}

#[cfg(unix)]
fn build_raw_syn(
    source: IpAddr,
    destination: IpAddr,
    spec: &TcpSynSpec,
) -> Result<Vec<u8>, ProbeError> {
    match (source, destination) {
        (IpAddr::V4(source), IpAddr::V4(destination)) => {
            Ok(build_tcp_syn_ipv4(source, destination, spec))
        }
        (IpAddr::V6(source), IpAddr::V6(destination)) => {
            Ok(build_tcp_syn_ipv6(source, destination, spec))
        }
        _ => Err(ProbeError::Io {
            operation: "build TCP trace probe",
            message: "source and destination families differ".into(),
        }),
    }
}

/// Raw ICMP and TCP listeners for a raw SYN probe, polled together so that
/// neither read delays the other.
#[cfg(unix)]
struct RawTcpListeners {
    icmp: RawIcmpSocket,
    tcp: RawIcmpSocket,
}

#[cfg(unix)]
impl RawTcpListeners {
    fn new(family: AddressFamily, timeout: Duration) -> Result<Self, ProbeError> {
        let timeout = timeout.min(Duration::from_millis(50));
        Ok(Self {
            icmp: RawIcmpSocket::new(family, timeout)?,
            tcp: RawIcmpSocket::new_protocol(family, libc::IPPROTO_TCP, timeout)?,
        })
    }

    fn wait(
        &self,
        ttl: u8,
        key: &ProbeKey,
        deadline: Instant,
        started: Instant,
        accept: impl Fn(&ParsedTcp) -> bool,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<Sample, ProbeError> {
        let sequence = u16::from(ttl);
        loop {
            if cancellation.is_cancelled() {
                return Ok(cancelled_sample(sequence));
            }
            if Instant::now() >= deadline {
                return Ok(timeout_sample(sequence));
            }
            let mut fds = [
                pollfd(self.icmp.fd.as_raw_fd(), libc::POLLIN),
                pollfd(self.tcp.fd.as_raw_fd(), libc::POLLIN),
            ];
            poll_sockets(&mut fds, deadline)?;
            if cancellation.is_cancelled() {
                return Ok(cancelled_sample(sequence));
            }
            while let Some((bytes, source)) = self.icmp.receive()? {
                if let Ok(mut response) = parse_icmp_response(&bytes, key.family)
                    && response.correlates(key)
                {
                    if response.source.is_none() {
                        response.source = source;
                    }
                    return Ok(icmp_sample(
                        sequence,
                        response.kind,
                        response.source,
                        started,
                        response.next_hop_mtu,
                        response.mpls_labels,
                    ));
                }
            }
            while let Some((bytes, source)) = self.tcp.receive()? {
                if let Ok(mut response) = parse_tcp_response(&bytes, key.family) {
                    if response.source.is_none() {
                        response.source = source;
                    }
                    if accept(&response) {
                        return Ok(Sample {
                            sequence,
                            hop: response.source,
                            elapsed_ms: Some(elapsed_ms(started)),
                            status: SampleStatus::Reply,
                            next_hop_mtu: None,
                            mpls: Vec::new(),
                        });
                    }
                }
            }
        }
    }
}

#[cfg(all(not(unix), not(windows)))]
pub fn tcp_trace(
    _target: IpAddr,
    _first_ttl: u8,
    _parameters: &RoundParameters,
    _destination_port: u16,
    _cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    Err(ProbeError::Unsupported {
        operation: "TCP trace",
        message: "the current native raw socket backend supports Unix targets".into(),
    })
}

#[cfg(all(not(unix), not(windows)))]
pub fn udp_trace(
    _target: IpAddr,
    _first_ttl: u8,
    _parameters: &RoundParameters,
    _base_port: u16,
    _cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<HopResult>, ProbeError> {
    Err(ProbeError::Unsupported {
        operation: "UDP trace",
        message: "the current native socket backend supports Unix targets".into(),
    })
}

#[cfg(all(not(unix), not(windows)))]
pub fn tcp_trace_probe_scoped(
    _target: std::net::SocketAddr,
    _ttl: u8,
    _parameters: &RoundParameters,
    _destination_port: u16,
    _identity: ProbeIdentity,
    _cancellation: &tokio_util::sync::CancellationToken,
) -> Result<HopResult, ProbeError> {
    Err(ProbeError::Unsupported {
        operation: "TCP trace probe",
        message: "the current native socket backend supports Unix and Windows targets".into(),
    })
}

/// Report which native sockets this process may open. Each socket is opened and
/// immediately closed; no packet is sent.
#[cfg(unix)]
pub fn native_access() -> NativeAccess {
    let opens = |domain, kind, protocol| {
        open_socket(domain, kind, protocol, "check native socket access").is_ok()
    };
    let raw_ip_sender = RawIpSocket::new(AddressFamily::Ipv4).is_ok();
    NativeAccess {
        raw_icmpv4: opens(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP),
        raw_icmpv6: opens(libc::AF_INET6, libc::SOCK_RAW, libc::IPPROTO_ICMPV6),
        datagram_icmpv4: opens(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP),
        datagram_icmpv6: opens(libc::AF_INET6, libc::SOCK_DGRAM, libc::IPPROTO_ICMPV6),
        raw_ip_sender,
        tcp_trace_mode: if cfg!(target_os = "linux") && raw_ip_sender {
            TcpTraceMode::Raw
        } else {
            TcpTraceMode::Connect
        },
        elevated: None,
        windows_tcp_intermediate_hops: None,
        traffic_class_supported: true,
    }
}

#[cfg(all(not(unix), not(windows)))]
pub fn native_access() -> NativeAccess {
    NativeAccess {
        raw_icmpv4: false,
        raw_icmpv6: false,
        datagram_icmpv4: false,
        datagram_icmpv6: false,
        raw_ip_sender: false,
        tcp_trace_mode: TcpTraceMode::Connect,
        elevated: None,
        windows_tcp_intermediate_hops: None,
        traffic_class_supported: false,
    }
}

#[cfg(unix)]
fn set_datagram_hop_limit(
    socket: &UdpSocket,
    family: AddressFamily,
    ttl: u8,
) -> Result<(), ProbeError> {
    let value = libc::c_int::from(ttl);
    let (level, option) = match family {
        AddressFamily::Ipv4 => (libc::IPPROTO_IP, libc::IP_TTL),
        AddressFamily::Ipv6 => (libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS),
    };
    set_option(
        socket.as_raw_fd(),
        level,
        option,
        &value,
        "set UDP probe hop limit",
    )
}

#[cfg(unix)]
fn set_datagram_traffic_class(
    socket: &UdpSocket,
    family: AddressFamily,
    traffic_class: Option<u8>,
) -> Result<(), ProbeError> {
    let Some(traffic_class) = traffic_class else {
        return Ok(());
    };
    let value = libc::c_int::from(traffic_class);
    let (level, option) = match family {
        AddressFamily::Ipv4 => (libc::IPPROTO_IP, libc::IP_TOS),
        AddressFamily::Ipv6 => (libc::IPPROTO_IPV6, libc::IPV6_TCLASS),
    };
    set_option(
        socket.as_raw_fd(),
        level,
        option,
        &value,
        "set UDP probe traffic class",
    )
}

#[cfg(unix)]
fn probe_once(
    target: SocketAddr,
    identifier: u16,
    sequence: u16,
    ttl: Option<u8>,
    parameters: &RoundParameters,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Sample, ProbeError> {
    let family = match target.ip() {
        IpAddr::V4(_) => AddressFamily::Ipv4,
        IpAddr::V6(_) => AddressFamily::Ipv6,
    };
    let socket = RawIcmpSocket::new(family, parameters.timeout.min(Duration::from_millis(100)))?;
    let identifier = socket.prepare_echo(family, identifier)?;
    if let Some(ttl) = ttl {
        socket.set_hop_limit(family, ttl)?;
    }
    socket.set_traffic_class(family, parameters.traffic_class)?;
    if parameters.dont_fragment && family == AddressFamily::Ipv4 {
        socket.set_do_not_fragment()?;
    }
    let payload = vec![0x4c; parameters.payload_size];
    let packet = match target.ip() {
        IpAddr::V4(_) => build_icmpv4_echo(identifier, sequence, &payload),
        IpAddr::V6(destination) => {
            let source = ipv6_source_for(target, destination)?;
            build_icmpv6_echo(source, destination, identifier, sequence, &payload)
        }
    };
    let started = Instant::now();
    if begin_probe(cancellation, || socket.send_to(&packet, target))? == SendOutcome::Unreachable {
        return Ok(unreachable_sample(sequence));
    }
    let expected = ProbeKey::icmp(family, identifier, sequence);
    let deadline = started + parameters.timeout;
    loop {
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        if Instant::now() >= deadline {
            return Ok(timeout_sample(sequence));
        }
        poll_sockets(&mut [pollfd(socket.fd.as_raw_fd(), libc::POLLIN)], deadline)?;
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        // Linux ping sockets report ICMP errors only through the error queue,
        // which is read first because a pending error also fails a normal read.
        #[cfg(target_os = "linux")]
        if socket.kind == SocketKind::Datagram {
            while let Some(queued) = receive_error_queue(socket.fd.as_raw_fd())? {
                if let Some(kind) = queued.kind()
                    && quoted_echo_sequence(&queued.payload, family) == Some(sequence)
                {
                    return Ok(icmp_sample(
                        sequence,
                        kind,
                        queued.offender,
                        started,
                        queued.next_hop_mtu(),
                        Vec::new(),
                    ));
                }
            }
        }
        while let Some((bytes, source)) = socket.receive()? {
            let Ok(mut response) = parse_icmp_response(&bytes, family) else {
                continue;
            };
            if !response.correlates(&expected) {
                continue;
            }
            if response.source.is_none() {
                response.source = source;
            }
            return Ok(icmp_sample(
                sequence,
                response.kind,
                response.source,
                started,
                response.next_hop_mtu,
                response.mpls_labels,
            ));
        }
    }
}

#[cfg(all(not(unix), not(windows)))]
fn probe_once(
    _target: SocketAddr,
    _identifier: u16,
    _sequence: u16,
    _ttl: Option<u8>,
    _parameters: &RoundParameters,
    _cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Sample, ProbeError> {
    Err(ProbeError::Unsupported {
        operation: "raw ICMP probe",
        message: "the current native socket backend supports Unix targets".into(),
    })
}

fn timeout_sample(sequence: u16) -> Sample {
    Sample {
        sequence,
        hop: None,
        elapsed_ms: None,
        status: SampleStatus::Timeout,
        next_hop_mtu: None,
        mpls: Vec::new(),
    }
}

fn cancelled_sample(sequence: u16) -> Sample {
    Sample {
        sequence,
        hop: None,
        elapsed_ms: None,
        status: SampleStatus::Cancelled,
        next_hop_mtu: None,
        mpls: Vec::new(),
    }
}

/// The kernel had no route for the probe and no router identified itself.
#[cfg(unix)]
fn unreachable_sample(sequence: u16) -> Sample {
    Sample {
        sequence,
        hop: None,
        elapsed_ms: None,
        status: SampleStatus::Unreachable,
        next_hop_mtu: None,
        mpls: Vec::new(),
    }
}

#[cfg(unix)]
fn reply_sample(sequence: u16, target: IpAddr, started: Instant) -> Sample {
    Sample {
        sequence,
        hop: Some(target),
        elapsed_ms: Some(elapsed_ms(started)),
        status: SampleStatus::Reply,
        next_hop_mtu: None,
        mpls: Vec::new(),
    }
}

#[cfg(unix)]
fn icmp_sample(
    sequence: u16,
    kind: IcmpKind,
    hop: Option<IpAddr>,
    started: Instant,
    next_hop_mtu: Option<u32>,
    mpls: Vec<lantern_packet::MplsLabel>,
) -> Sample {
    let status = match kind {
        IcmpKind::EchoReply => SampleStatus::Reply,
        IcmpKind::TimeExceeded => SampleStatus::TimeExceeded,
        IcmpKind::DestinationUnreachable
        | IcmpKind::PacketTooBig
        | IcmpKind::ParameterProblem
        | IcmpKind::Other => SampleStatus::Unreachable,
    };
    Sample {
        sequence,
        hop,
        elapsed_ms: Some(elapsed_ms(started)),
        status,
        next_hop_mtu,
        mpls,
    }
}

#[cfg(unix)]
fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn begin_probe<T>(
    cancellation: &tokio_util::sync::CancellationToken,
    send: impl FnOnce() -> Result<T, ProbeError>,
) -> Result<T, ProbeError> {
    if cancellation.is_cancelled() {
        return Err(ProbeError::Cancelled);
    }
    send()
}

/// Outcome of handing one probe to the kernel.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendOutcome {
    Sent,
    /// The kernel refused the send because no route reaches the target.
    Unreachable,
}

#[cfg(unix)]
fn is_route_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENETUNREACH | libc::EHOSTUNREACH | libc::EHOSTDOWN)
    )
}

#[cfg(unix)]
fn send_outcome(
    operation: &'static str,
    sent: isize,
    expected: usize,
) -> Result<SendOutcome, ProbeError> {
    if sent < 0 {
        let error = io::Error::last_os_error();
        if is_route_error(&error) {
            return Ok(SendOutcome::Unreachable);
        }
        return Err(socket_error(operation, error));
    }
    if sent as usize != expected {
        return Err(ProbeError::Io {
            operation,
            message: format!("short write: sent {sent} of {expected} bytes"),
        });
    }
    Ok(SendOutcome::Sent)
}

/// How long one `poll()` may block before cancellation is checked again.
#[cfg(unix)]
const POLL_SLICE: Duration = Duration::from_millis(25);

#[cfg(unix)]
fn pollfd(fd: libc::c_int, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

/// Wait until a socket is ready, the deadline passes, or one poll slice ends.
/// An interrupted wait returns early so the caller re-checks its state.
#[cfg(unix)]
fn poll_sockets(fds: &mut [libc::pollfd], deadline: Instant) -> Result<(), ProbeError> {
    for entry in fds.iter_mut() {
        entry.revents = 0;
    }
    let wait = deadline
        .saturating_duration_since(Instant::now())
        .min(POLL_SLICE);
    let millis = wait.as_micros().div_ceil(1000) as libc::c_int;
    let status = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) };
    if status < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(socket_error("wait for probe response", error));
    }
    Ok(())
}

#[cfg(unix)]
fn open_socket(
    domain: libc::c_int,
    kind: libc::c_int,
    protocol: libc::c_int,
    operation: &'static str,
) -> Result<OwnedFd, ProbeError> {
    let raw_fd = unsafe { libc::socket(domain, kind, protocol) };
    if raw_fd < 0 {
        return Err(socket_error(operation, io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
}

#[cfg(unix)]
fn set_nonblocking(fd: libc::c_int) -> Result<(), ProbeError> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(socket_error(
            "set TCP trace nonblocking",
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

/// Read and clear the socket's pending error (`SO_ERROR`).
#[cfg(unix)]
fn pending_socket_error(fd: libc::c_int) -> Result<libc::c_int, ProbeError> {
    let mut value: libc::c_int = 0;
    let mut length = mem::size_of::<libc::c_int>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&raw mut value).cast(),
            &mut length,
        )
    };
    if status != 0 {
        return Err(socket_error(
            "read socket error state",
            io::Error::last_os_error(),
        ));
    }
    Ok(value)
}

#[cfg(unix)]
fn local_port(fd: libc::c_int, operation: &'static str) -> Result<u16, ProbeError> {
    let mut storage = MaybeUninit::<libc::sockaddr_storage>::zeroed();
    let mut length = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    if unsafe { libc::getsockname(fd, storage.as_mut_ptr().cast(), &mut length) } != 0 {
        return Err(socket_error(operation, io::Error::last_os_error()));
    }
    let storage = unsafe { storage.assume_init() };
    sockaddr_port(&storage).ok_or_else(|| ProbeError::Io {
        operation,
        message: "socket has no IP address".into(),
    })
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketKind {
    Raw,
    /// Unprivileged ICMP socket (`SOCK_DGRAM`). BSD/macOS deliver ICMP to it
    /// like a raw socket; Linux "ping sockets" rewrite the echo identifier and
    /// queue ICMP errors on the socket error queue.
    Datagram,
}

#[cfg(unix)]
const ICMP_PERMISSION_HINT: &str = "run with sudo/root (or grant CAP_NET_RAW on Linux), \
     register the privileged helper, or on Linux allow unprivileged ICMP sockets via \
     the net.ipv4.ping_group_range sysctl";

#[cfg(unix)]
struct RawIcmpSocket {
    fd: OwnedFd,
    kind: SocketKind,
}

#[cfg(unix)]
impl RawIcmpSocket {
    fn new(family: AddressFamily, timeout: Duration) -> Result<Self, ProbeError> {
        let protocol = match family {
            AddressFamily::Ipv4 => libc::IPPROTO_ICMP,
            AddressFamily::Ipv6 => libc::IPPROTO_ICMPV6,
        };
        Self::new_protocol(family, protocol, timeout)
    }

    /// Open a raw socket for `protocol`. ICMP falls back to an unprivileged
    /// datagram socket when raw access is denied.
    fn new_protocol(
        family: AddressFamily,
        protocol: libc::c_int,
        timeout: Duration,
    ) -> Result<Self, ProbeError> {
        let domain = match family {
            AddressFamily::Ipv4 => libc::AF_INET,
            AddressFamily::Ipv6 => libc::AF_INET6,
        };
        let icmp = matches!(protocol, libc::IPPROTO_ICMP | libc::IPPROTO_ICMPV6);
        let (fd, kind) = match open_socket(domain, libc::SOCK_RAW, protocol, "open raw ICMP socket")
        {
            Ok(fd) => (fd, SocketKind::Raw),
            Err(ProbeError::Permission { message: raw, .. }) if icmp => {
                match open_socket(
                    domain,
                    libc::SOCK_DGRAM,
                    protocol,
                    "open datagram ICMP socket",
                ) {
                    Ok(fd) => {
                        static ONCE: std::sync::Once = std::sync::Once::new();
                        ONCE.call_once(|| {
                            log::info!(
                                "raw ICMP socket denied ({raw}); using unprivileged datagram ICMP sockets"
                            );
                        });
                        (fd, SocketKind::Datagram)
                    }
                    Err(error) => {
                        let datagram = match error {
                            ProbeError::Permission { message, .. }
                            | ProbeError::Io { message, .. } => message,
                            other => other.to_string(),
                        };
                        return Err(ProbeError::Permission {
                            operation: "open ICMP socket",
                            message: format!(
                                "raw socket: {raw}; datagram socket: {datagram}; \
                                 {ICMP_PERMISSION_HINT}"
                            ),
                        });
                    }
                }
            }
            Err(error) => return Err(error),
        };
        let timeval = libc::timeval {
            tv_sec: timeout.as_secs().try_into().unwrap_or(libc::time_t::MAX),
            tv_usec: timeout.subsec_micros() as libc::suseconds_t,
        };
        set_option(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &timeval,
            "set raw socket receive timeout",
        )?;
        Ok(Self { fd, kind })
    }

    /// Return the echo identifier replies will carry. Linux ping sockets
    /// replace it with the socket's local port and report ICMP errors through
    /// the error queue, which is enabled here.
    #[cfg(target_os = "linux")]
    fn prepare_echo(&self, family: AddressFamily, identifier: u16) -> Result<u16, ProbeError> {
        if self.kind == SocketKind::Raw {
            return Ok(identifier);
        }
        let fd = self.fd.as_raw_fd();
        enable_error_queue(fd, family)?;
        let unspecified = match family {
            AddressFamily::Ipv4 => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            AddressFamily::Ipv6 => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        };
        let (storage, length) = socket_address(unspecified);
        if unsafe { libc::bind(fd, (&raw const storage).cast(), length) } != 0 {
            return Err(socket_error(
                "bind datagram ICMP socket",
                io::Error::last_os_error(),
            ));
        }
        local_port(fd, "read datagram ICMP identifier")
    }

    #[cfg(not(target_os = "linux"))]
    fn prepare_echo(&self, _family: AddressFamily, identifier: u16) -> Result<u16, ProbeError> {
        Ok(identifier)
    }

    fn set_hop_limit(&self, family: AddressFamily, ttl: u8) -> Result<(), ProbeError> {
        let value = libc::c_int::from(ttl);
        let (level, option) = match family {
            AddressFamily::Ipv4 => (libc::IPPROTO_IP, libc::IP_TTL),
            AddressFamily::Ipv6 => (libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS),
        };
        set_option(
            self.fd.as_raw_fd(),
            level,
            option,
            &value,
            "set probe hop limit",
        )
    }

    fn set_traffic_class(
        &self,
        family: AddressFamily,
        traffic_class: Option<u8>,
    ) -> Result<(), ProbeError> {
        let Some(traffic_class) = traffic_class else {
            return Ok(());
        };
        let value = libc::c_int::from(traffic_class);
        let (level, option) = match family {
            AddressFamily::Ipv4 => (libc::IPPROTO_IP, libc::IP_TOS),
            AddressFamily::Ipv6 => (libc::IPPROTO_IPV6, libc::IPV6_TCLASS),
        };
        set_option(
            self.fd.as_raw_fd(),
            level,
            option,
            &value,
            "set probe traffic class",
        )
    }

    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    fn set_do_not_fragment(&self) -> Result<(), ProbeError> {
        let value: libc::c_int = 1;
        set_option(
            self.fd.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_DONTFRAG,
            &value,
            "set IPv4 do-not-fragment",
        )
    }

    #[cfg(target_os = "linux")]
    fn set_do_not_fragment(&self) -> Result<(), ProbeError> {
        let value: libc::c_int = libc::IP_PMTUDISC_DO;
        set_option(
            self.fd.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            &value,
            "set IPv4 path MTU discovery",
        )
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "linux"
    )))]
    fn set_do_not_fragment(&self) -> Result<(), ProbeError> {
        Err(ProbeError::Unsupported {
            operation: "IPv4 do-not-fragment",
            message: "no socket option is defined for this Unix target".into(),
        })
    }

    fn send_to(&self, packet: &[u8], target: SocketAddr) -> Result<SendOutcome, ProbeError> {
        let (storage, length) = socket_address(target);
        let sent = unsafe {
            libc::sendto(
                self.fd.as_raw_fd(),
                packet.as_ptr().cast(),
                packet.len(),
                0,
                (&raw const storage).cast(),
                length,
            )
        };
        send_outcome("send raw ICMP probe", sent, packet.len())
    }

    /// Read one queued packet without blocking; callers wait with `poll()`.
    fn receive(&self) -> Result<ReceivedPacket, ProbeError> {
        let mut bytes = vec![0u8; 65_535];
        loop {
            let mut storage = MaybeUninit::<libc::sockaddr_storage>::zeroed();
            let mut length = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            let received = unsafe {
                libc::recvfrom(
                    self.fd.as_raw_fd(),
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                    libc::MSG_DONTWAIT,
                    storage.as_mut_ptr().cast(),
                    &mut length,
                )
            };
            if received < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted => continue,
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => return Ok(None),
                    // A datagram socket surfaces asynchronous ICMP errors as a
                    // failed read; they are reported through the error queue.
                    _ if self.kind == SocketKind::Datagram
                        && (is_route_error(&error)
                            || matches!(
                                error.raw_os_error(),
                                Some(libc::ECONNREFUSED | libc::EMSGSIZE | libc::EPROTO)
                            )) =>
                    {
                        return Ok(None);
                    }
                    _ => return Err(socket_error("receive raw ICMP response", error)),
                }
            }
            bytes.truncate(received as usize);
            let storage = unsafe { storage.assume_init() };
            return Ok(Some((bytes, sockaddr_ip(&storage))));
        }
    }
}

#[cfg(unix)]
struct RawIpSocket {
    fd: OwnedFd,
}

#[cfg(unix)]
impl RawIpSocket {
    fn new(family: AddressFamily) -> Result<Self, ProbeError> {
        let domain = match family {
            AddressFamily::Ipv4 => libc::AF_INET,
            AddressFamily::Ipv6 => libc::AF_INET6,
        };
        let fd = open_socket(
            domain,
            libc::SOCK_RAW,
            libc::IPPROTO_RAW,
            "open raw IP socket",
        )?;
        if family == AddressFamily::Ipv4 {
            let enabled: libc::c_int = 1;
            set_option(
                fd.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_HDRINCL,
                &enabled,
                "enable IPv4 header inclusion",
            )?;
        }
        Ok(Self { fd })
    }

    fn send_to(&self, packet: &[u8], target: SocketAddr) -> Result<SendOutcome, ProbeError> {
        let (storage, length) = socket_address(target);
        let sent = unsafe {
            libc::sendto(
                self.fd.as_raw_fd(),
                packet.as_ptr().cast(),
                packet.len(),
                0,
                (&raw const storage).cast(),
                length,
            )
        };
        send_outcome("send raw TCP probe", sent, packet.len())
    }
}

/// One ICMP error reported through a Linux socket error queue
/// (`IP_RECVERR` / `IPV6_RECVERR`).
#[cfg(target_os = "linux")]
struct QueuedError {
    origin: u8,
    icmp_type: u8,
    code: u8,
    info: u32,
    /// The router or host that sent the ICMP error.
    offender: Option<IpAddr>,
    /// The original destination and port of the probe.
    destination: Option<SocketAddr>,
    /// The start of the original transport payload.
    payload: Vec<u8>,
}

#[cfg(target_os = "linux")]
impl QueuedError {
    fn kind(&self) -> Option<IcmpKind> {
        match (self.origin, self.icmp_type) {
            (libc::SO_EE_ORIGIN_ICMP, 11) | (libc::SO_EE_ORIGIN_ICMP6, 3) => {
                Some(IcmpKind::TimeExceeded)
            }
            (libc::SO_EE_ORIGIN_ICMP, 3) | (libc::SO_EE_ORIGIN_ICMP6, 1) => {
                Some(IcmpKind::DestinationUnreachable)
            }
            (libc::SO_EE_ORIGIN_ICMP6, 2) => Some(IcmpKind::PacketTooBig),
            (libc::SO_EE_ORIGIN_ICMP, 12) | (libc::SO_EE_ORIGIN_ICMP6, 4) => {
                Some(IcmpKind::ParameterProblem)
            }
            _ => None,
        }
    }

    fn next_hop_mtu(&self) -> Option<u32> {
        match (self.origin, self.icmp_type, self.code) {
            (libc::SO_EE_ORIGIN_ICMP, 3, 4) | (libc::SO_EE_ORIGIN_ICMP6, 2, _) => Some(self.info),
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
fn enable_error_queue(fd: libc::c_int, family: AddressFamily) -> Result<(), ProbeError> {
    let enabled: libc::c_int = 1;
    let (level, option) = match family {
        AddressFamily::Ipv4 => (libc::SOL_IP, libc::IP_RECVERR),
        AddressFamily::Ipv6 => (libc::SOL_IPV6, libc::IPV6_RECVERR),
    };
    set_option(fd, level, option, &enabled, "enable ICMP error queue")
}

/// Drop stale queued errors and the pending socket error before a new probe,
/// so neither is attributed to it or fails its send.
#[cfg(target_os = "linux")]
fn discard_error_queue(fd: libc::c_int) -> Result<(), ProbeError> {
    while receive_error_queue(fd)?.is_some() {}
    pending_socket_error(fd).map(|_| ())
}

/// Dequeue one entry from the socket error queue without blocking.
#[cfg(target_os = "linux")]
fn receive_error_queue(fd: libc::c_int) -> Result<Option<QueuedError>, ProbeError> {
    let mut payload = vec![0u8; 512];
    let mut control = [0u64; 64];
    let mut name = MaybeUninit::<libc::sockaddr_storage>::zeroed();
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut header = unsafe { mem::zeroed::<libc::msghdr>() };
    header.msg_name = name.as_mut_ptr().cast();
    header.msg_namelen = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    header.msg_iov = &raw mut iov;
    header.msg_iovlen = 1;
    header.msg_control = control.as_mut_ptr().cast();
    header.msg_controllen = mem::size_of_val(&control) as _;
    let received = loop {
        let received =
            unsafe { libc::recvmsg(fd, &raw mut header, libc::MSG_ERRQUEUE | libc::MSG_DONTWAIT) };
        if received >= 0 {
            break received as usize;
        }
        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok(None),
            _ => return Err(socket_error("receive ICMP error queue", error)),
        }
    };
    payload.truncate(received.min(payload.len()));
    let name = unsafe { name.assume_init() };
    let destination = sockaddr_ip(&name)
        .zip(sockaddr_port(&name))
        .map(|(address, port)| SocketAddr::new(address, port));
    // Entries without an extended-error message keep origin 0 (no ICMP kind).
    let mut queued = QueuedError {
        origin: 0,
        icmp_type: 0,
        code: 0,
        info: 0,
        offender: None,
        destination,
        payload,
    };
    let mut message = unsafe { libc::CMSG_FIRSTHDR(&raw const header) };
    while !message.is_null() {
        let (level, kind, length) = unsafe {
            (
                (*message).cmsg_level,
                (*message).cmsg_type,
                (*message).cmsg_len as usize,
            )
        };
        let is_error = (level == libc::SOL_IP && kind == libc::IP_RECVERR)
            || (level == libc::SOL_IPV6 && kind == libc::IPV6_RECVERR);
        let header_length = unsafe { libc::CMSG_LEN(0) } as usize;
        let error_length = mem::size_of::<libc::sock_extended_err>();
        if is_error && length >= header_length + error_length {
            let data = unsafe { libc::CMSG_DATA(message) };
            let error = unsafe { data.cast::<libc::sock_extended_err>().read_unaligned() };
            // The offender address directly follows the extended error.
            let available = length - header_length - error_length;
            let mut offender = unsafe { mem::zeroed::<libc::sockaddr_storage>() };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.add(error_length),
                    (&raw mut offender).cast::<u8>(),
                    available.min(mem::size_of::<libc::sockaddr_storage>()),
                );
            }
            queued.origin = error.ee_origin;
            queued.icmp_type = error.ee_type;
            queued.code = error.ee_code;
            queued.info = error.ee_info;
            queued.offender = sockaddr_ip(&offender);
        }
        message = unsafe { libc::CMSG_NXTHDR(&raw const header, message) };
    }
    Ok(Some(queued))
}

/// The sequence of an echo request quoted in a Linux error-queue payload.
#[cfg(target_os = "linux")]
fn quoted_echo_sequence(payload: &[u8], family: AddressFamily) -> Option<u16> {
    let request = match family {
        AddressFamily::Ipv4 => 8,
        AddressFamily::Ipv6 => 128,
    };
    (payload.len() >= 8 && payload[0] == request)
        .then(|| u16::from_be_bytes([payload[6], payload[7]]))
}

#[cfg(unix)]
fn set_option<T>(
    fd: libc::c_int,
    level: libc::c_int,
    option: libc::c_int,
    value: &T,
    operation: &'static str,
) -> Result<(), ProbeError> {
    let status = unsafe {
        libc::setsockopt(
            fd,
            level,
            option,
            (value as *const T).cast(),
            mem::size_of::<T>() as libc::socklen_t,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(socket_error(operation, io::Error::last_os_error()))
    }
}

#[cfg(unix)]
fn socket_error(operation: &'static str, error: io::Error) -> ProbeError {
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => ProbeError::Permission {
            operation,
            message: error.to_string(),
        },
        Some(libc::EMFILE | libc::ENFILE) => ProbeError::Io {
            operation,
            message: format!("too many open files: {error}"),
        },
        _ => ProbeError::Io {
            operation,
            message: error.to_string(),
        },
    }
}

#[cfg(unix)]
fn ipv6_source_for(mut target: SocketAddr, destination: Ipv6Addr) -> Result<Ipv6Addr, ProbeError> {
    let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).map_err(|error| ProbeError::Io {
        operation: "select IPv6 source address",
        message: error.to_string(),
    })?;
    target.set_ip(IpAddr::V6(destination));
    target.set_port(9);
    socket.connect(target).map_err(|error| ProbeError::Io {
        operation: "select IPv6 source address",
        message: error.to_string(),
    })?;
    match socket
        .local_addr()
        .map_err(|error| ProbeError::Io {
            operation: "read IPv6 source address",
            message: error.to_string(),
        })?
        .ip()
    {
        IpAddr::V6(address) => Ok(address),
        IpAddr::V4(_) => Err(ProbeError::Io {
            operation: "select IPv6 source address",
            message: "OS selected an IPv4 address for an IPv6 target".into(),
        }),
    }
}

#[cfg(unix)]
fn source_for(mut destination: SocketAddr) -> Result<IpAddr, ProbeError> {
    let bind = match destination.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let socket = UdpSocket::bind(bind).map_err(|error| ProbeError::Io {
        operation: "select probe source address",
        message: error.to_string(),
    })?;
    destination.set_port(9);
    socket
        .connect(destination)
        .map_err(|error| ProbeError::Io {
            operation: "select probe source address",
            message: error.to_string(),
        })?;
    socket
        .local_addr()
        .map(|address| address.ip())
        .map_err(|error| ProbeError::Io {
            operation: "read probe source address",
            message: error.to_string(),
        })
}

#[cfg(unix)]
fn socket_address(target: SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let mut storage = unsafe { mem::zeroed::<libc::sockaddr_storage>() };
    match target {
        SocketAddr::V4(target) => {
            let address = *target.ip();
            let raw = (&raw mut storage).cast::<libc::sockaddr_in>();
            unsafe {
                (*raw).sin_family = libc::AF_INET as libc::sa_family_t;
                (*raw).sin_port = 0;
                (*raw).sin_addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(address.octets()),
                };
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(target) => {
            let address = *target.ip();
            let raw = (&raw mut storage).cast::<libc::sockaddr_in6>();
            unsafe {
                (*raw).sin6_family = libc::AF_INET6 as libc::sa_family_t;
                (*raw).sin6_port = 0;
                (*raw).sin6_addr = libc::in6_addr {
                    s6_addr: address.octets(),
                };
                (*raw).sin6_scope_id = target.scope_id();
            }
            (
                storage,
                mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    }
}

/// Like [`socket_address`], but keeps the port for `connect()`.
#[cfg(unix)]
fn socket_address_with_port(target: SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    let (mut storage, length) = socket_address(target);
    let port = target.port().to_be();
    match target {
        SocketAddr::V4(_) => unsafe {
            (*(&raw mut storage).cast::<libc::sockaddr_in>()).sin_port = port;
        },
        SocketAddr::V6(_) => unsafe {
            (*(&raw mut storage).cast::<libc::sockaddr_in6>()).sin6_port = port;
        },
    }
    (storage, length)
}

#[cfg(unix)]
fn sockaddr_ip(storage: &libc::sockaddr_storage) -> Option<IpAddr> {
    match i32::from(storage.ss_family) {
        libc::AF_INET => {
            let raw = unsafe { &*(storage as *const _ as *const libc::sockaddr_in) };
            Some(IpAddr::V4(Ipv4Addr::from(
                raw.sin_addr.s_addr.to_ne_bytes(),
            )))
        }
        libc::AF_INET6 => {
            let raw = unsafe { &*(storage as *const _ as *const libc::sockaddr_in6) };
            Some(IpAddr::V6(Ipv6Addr::from(raw.sin6_addr.s6_addr)))
        }
        _ => None,
    }
}

#[cfg(unix)]
fn sockaddr_port(storage: &libc::sockaddr_storage) -> Option<u16> {
    match i32::from(storage.ss_family) {
        libc::AF_INET => {
            let raw = unsafe { &*(storage as *const _ as *const libc::sockaddr_in) };
            Some(u16::from_be(raw.sin_port))
        }
        libc::AF_INET6 => {
            let raw = unsafe { &*(storage as *const _ as *const libc::sockaddr_in6) };
            Some(u16::from_be(raw.sin6_port))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::sync::{Arc, Mutex};

    fn sample(ttl: u8, sequence: u16) -> Sample {
        Sample {
            sequence,
            hop: Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, ttl))),
            elapsed_ms: Some(f64::from(ttl)),
            status: SampleStatus::Reply,
            next_hop_mtu: None,
            mpls: Vec::new(),
        }
    }

    fn trace_hops() -> Vec<HopResult> {
        [1, 2]
            .into_iter()
            .map(|ttl| HopResult {
                ttl,
                address: None,
                reached_destination: ttl == 2,
                samples: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn rejects_invalid_trace_ttl_without_touching_sockets() {
        let parameters = RoundParameters {
            payload_size: 32,
            dont_fragment: false,
            max_hops: 10,
            timeout: Duration::from_millis(10),
            traffic_class: None,
        };
        let error = trace(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            &parameters,
            &tokio_util::sync::CancellationToken::new(),
        )
        .unwrap_err();
        assert!(matches!(error, ProbeError::InvalidPlan(_)));
    }

    #[test]
    fn ping_cancellation_returns_samples_already_completed() {
        let cancellation = tokio_util::sync::CancellationToken::new();
        let cancel_probe = cancellation.clone();

        let samples = ping_with_probe(3, Duration::ZERO, &cancellation, move |sequence, _| {
            cancel_probe.cancel();
            Ok(sample(64, sequence))
        })
        .unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].sequence, 0);
    }

    #[test]
    fn send_boundary_distinguishes_unsent_failed_and_cancelled_after_send() {
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        let mut called = false;
        let result = begin_probe(&cancellation, || {
            called = true;
            Ok(())
        });
        assert!(matches!(result, Err(ProbeError::Cancelled)));
        assert!(!called);

        let cancellation = tokio_util::sync::CancellationToken::new();
        let failure = begin_probe(&cancellation, || -> Result<(), ProbeError> {
            Err(ProbeError::Io {
                operation: "injected send",
                message: "failed".into(),
            })
        });
        assert!(matches!(failure, Err(ProbeError::Io { .. })));
        assert_eq!(Statistics::from_samples(&[]).sent, 0);

        let after_send = cancellation.clone();
        begin_probe(&cancellation, || {
            after_send.cancel();
            Ok(())
        })
        .unwrap();
        let mut cancelled = cancelled_sample(7);
        // Status, rather than a potentially inconsistent serialized RTT,
        // governs whether an interrupted observation was received.
        cancelled.elapsed_ms = Some(1.0);
        let statistics = Statistics::from_samples(&[cancelled]);
        assert_eq!(statistics.sent, 1);
        assert_eq!(statistics.received, 0);
        assert_eq!(statistics.cancelled, 1);
        assert_eq!(statistics.loss_percent, 0.0);
    }

    #[test]
    fn path_statistics_launch_round_robin_and_keep_sequence_order() {
        let launches = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&launches);
        let statistics = measure_hops_with_probe(
            &trace_hops(),
            3,
            Duration::ZERO,
            1,
            &tokio_util::sync::CancellationToken::new(),
            move |ttl, sequence, _| {
                observed.lock().unwrap().push((ttl, sequence));
                Ok(sample(ttl, sequence))
            },
        )
        .unwrap();

        assert_eq!(
            *launches.lock().unwrap(),
            [(1, 3), (2, 6), (1, 4), (2, 7), (1, 5), (2, 8)]
        );
        assert_eq!(statistics[0].statistics.sent, 3);
        assert_eq!(statistics[1].statistics.sent, 3);
    }

    #[test]
    fn path_statistics_cancellation_keeps_completed_hop_samples() {
        let cancellation = tokio_util::sync::CancellationToken::new();
        let cancel_probe = cancellation.clone();
        let statistics = measure_hops_with_probe(
            &trace_hops(),
            3,
            Duration::ZERO,
            1,
            &cancellation,
            move |ttl, sequence, _| {
                cancel_probe.cancel();
                Ok(sample(ttl, sequence))
            },
        )
        .unwrap();

        assert_eq!(statistics[0].statistics.sent, 1);
        assert_eq!(statistics[0].statistics.received, 1);
        assert_eq!(statistics[1].statistics.sent, 0);
    }

    #[cfg(unix)]
    #[test]
    fn socket_address_round_trips_recorded_v4_and_v6_values() {
        for address in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ] {
            let (storage, _) = socket_address(SocketAddr::new(address, 0));
            assert_eq!(sockaddr_ip(&storage), Some(address));
        }
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "explicit loopback raw-socket verification"]
    fn probes_ipv4_loopback_without_a_child_process() {
        let parameters = RoundParameters {
            payload_size: 16,
            dont_fragment: false,
            max_hops: 1,
            timeout: Duration::from_millis(500),
            traffic_class: None,
        };
        let result = ping(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            1,
            64,
            &parameters,
            &tokio_util::sync::CancellationToken::new(),
        );
        match result {
            Ok(samples) => assert_eq!(samples[0].status, SampleStatus::Reply),
            Err(error) => {
                panic!("explicit loopback verification requires an actual reply: {error}")
            }
        }
    }
}
