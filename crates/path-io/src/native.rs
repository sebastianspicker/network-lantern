use crate::{
    HopResult, HopStatistics, ProbeError, ProbeIdentity, RoundParameters, Sample, SampleStatus,
    Statistics,
};
#[cfg(unix)]
use lantern_packet::{
    AddressFamily, IcmpKind, ProbeKey, TcpSynSpec, TransportProtocol, build_icmpv4_echo,
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
pub use windows::{tcp_trace_scoped, udp_trace_scoped};

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

/// Trace with kernel-generated UDP datagrams and a native raw ICMP response
/// socket. The destination port identifies each quoted datagram.
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
    let listener = RawIcmpSocket::new(family, parameters.timeout.min(Duration::from_millis(100)))?;
    let mut hops = Vec::new();
    for ttl in first_ttl..=parameters.max_hops {
        if cancellation.is_cancelled() {
            if hops.is_empty() {
                return Err(ProbeError::Cancelled);
            }
            break;
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
        begin_probe(cancellation, || {
            sender
                .send_to(&payload, destination)
                .map_err(|error| ProbeError::Io {
                    operation: "send UDP trace probe",
                    message: error.to_string(),
                })
        })?;
        let key = ProbeKey::ports(
            family,
            TransportProtocol::Udp,
            source_port,
            destination_port,
        );
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
                && matches!(
                    (family, response.code),
                    (AddressFamily::Ipv4, 3) | (AddressFamily::Ipv6, 4)
                );
            let status = match response.kind {
                IcmpKind::TimeExceeded => SampleStatus::TimeExceeded,
                IcmpKind::DestinationUnreachable if reached => SampleStatus::Reply,
                _ => SampleStatus::Unreachable,
            };
            observed = Some((
                Sample {
                    sequence: u16::from(ttl),
                    hop: response.source,
                    elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                    status,
                    next_hop_mtu: response.next_hop_mtu,
                    mpls: response.mpls_labels,
                },
                reached,
            ));
            break;
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

/// Trace with native raw TCP SYN packets. ICMP quotations identify intermediate
/// hops; SYN-ACK or RST packets identify the destination without completing a
/// TCP handshake.
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
    let source = source_for(target)?;
    let source_port = 49_152u16.wrapping_add(rand::random::<u16>() % 16_383);
    let sender = RawIpSocket::new(family)?;
    let icmp_listener =
        RawIcmpSocket::new(family, parameters.timeout.min(Duration::from_millis(50)))?;
    let tcp_listener = RawIcmpSocket::new_protocol(
        family,
        libc::IPPROTO_TCP,
        parameters.timeout.min(Duration::from_millis(50)),
    )?;
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
        let packet = match (source, target.ip()) {
            (IpAddr::V4(source), IpAddr::V4(destination)) => {
                build_tcp_syn_ipv4(source, destination, &spec)
            }
            (IpAddr::V6(source), IpAddr::V6(destination)) => {
                build_tcp_syn_ipv6(source, destination, &spec)
            }
            _ => {
                return Err(ProbeError::Io {
                    operation: "build TCP trace probe",
                    message: "source and destination families differ".into(),
                });
            }
        };
        let started = Instant::now();
        begin_probe(cancellation, || sender.send_to(&packet, target))?;
        let key = ProbeKey::tcp(family, source_port, destination_port, Some(sequence));
        let mut observed = None;
        let mut cancelled = false;
        while started.elapsed() < parameters.timeout {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let icmp_packet = icmp_listener.receive()?;
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
                let status = if response.kind == IcmpKind::TimeExceeded {
                    SampleStatus::TimeExceeded
                } else {
                    SampleStatus::Unreachable
                };
                observed = Some((
                    Sample {
                        sequence: ttl.into(),
                        hop: response.source,
                        elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                        status,
                        next_hop_mtu: response.next_hop_mtu,
                        mpls: response.mpls_labels,
                    },
                    false,
                ));
            }
            if observed.is_some() {
                break;
            }
            let tcp_packet = tcp_listener.receive()?;
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            if let Some((bytes, source_address)) = tcp_packet
                && let Ok(mut response) = parse_tcp_response(&bytes, family)
            {
                if response.source.is_none() {
                    response.source = source_address;
                }
                if response.source_port == destination_port
                    && response.destination_port == source_port
                    && (response.rst
                        || (response.syn
                            && response.ack
                            && response.acknowledgment == sequence.wrapping_add(1)))
                {
                    observed = Some((
                        Sample {
                            sequence: ttl.into(),
                            hop: response.source,
                            elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                            status: SampleStatus::Reply,
                            next_hop_mtu: None,
                            mpls: Vec::new(),
                        },
                        true,
                    ));
                }
            }
            if observed.is_some() {
                break;
            }
        }
        let (sample, reached_destination) = if cancelled {
            (cancelled_sample(ttl.into()), false)
        } else {
            observed.unwrap_or_else(|| (timeout_sample(ttl.into()), false))
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
    let packet = match (source, target.ip()) {
        (IpAddr::V4(source), IpAddr::V4(destination)) => {
            build_tcp_syn_ipv4(source, destination, &spec)
        }
        (IpAddr::V6(source), IpAddr::V6(destination)) => {
            build_tcp_syn_ipv6(source, destination, &spec)
        }
        _ => {
            return Err(ProbeError::Io {
                operation: "build TCP trace probe",
                message: "source and destination families differ".into(),
            });
        }
    };
    let sender = RawIpSocket::new(family)?;
    let icmp_listener =
        RawIcmpSocket::new(family, parameters.timeout.min(Duration::from_millis(50)))?;
    let tcp_listener = RawIcmpSocket::new_protocol(
        family,
        libc::IPPROTO_TCP,
        parameters.timeout.min(Duration::from_millis(50)),
    )?;
    let started = Instant::now();
    begin_probe(cancellation, || sender.send_to(&packet, target))?;
    let key = ProbeKey::tcp(family, source_port, destination_port, Some(sequence));
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
        let icmp_packet = icmp_listener.receive()?;
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
        let tcp_packet = tcp_listener.receive()?;
        if cancellation.is_cancelled() {
            let sample = cancelled_sample(ttl.into());
            return Ok(HopResult {
                ttl,
                address: None,
                reached_destination: false,
                samples: vec![sample],
            });
        }
        if let Some((bytes, source_address)) = tcp_packet
            && let Ok(mut response) = parse_tcp_response(&bytes, family)
        {
            if response.source.is_none() {
                response.source = source_address;
            }
            if response.source_port == destination_port
                && response.destination_port == source_port
                && response.ack
                && response.acknowledgment == sequence.wrapping_add(1)
                && (response.rst || response.syn)
            {
                let sample = Sample {
                    sequence: ttl.into(),
                    hop: response.source,
                    elapsed_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                    status: SampleStatus::Reply,
                    next_hop_mtu: None,
                    mpls: Vec::new(),
                };
                return Ok(HopResult {
                    ttl,
                    address: sample.hop,
                    reached_destination: true,
                    samples: vec![sample],
                });
            }
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
    begin_probe(cancellation, || socket.send_to(&packet, target))?;
    let expected = ProbeKey::icmp(family, identifier, sequence);
    loop {
        if cancellation.is_cancelled() {
            return Ok(cancelled_sample(sequence));
        }
        if started.elapsed() >= parameters.timeout {
            return Ok(timeout_sample(sequence));
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
        if !response.correlates(&expected) {
            continue;
        }
        if response.source.is_none() {
            response.source = source;
        }
        let status = match response.kind {
            IcmpKind::EchoReply => SampleStatus::Reply,
            IcmpKind::TimeExceeded => SampleStatus::TimeExceeded,
            IcmpKind::DestinationUnreachable
            | IcmpKind::PacketTooBig
            | IcmpKind::ParameterProblem
            | IcmpKind::Other => SampleStatus::Unreachable,
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

fn begin_probe<T>(
    cancellation: &tokio_util::sync::CancellationToken,
    send: impl FnOnce() -> Result<T, ProbeError>,
) -> Result<T, ProbeError> {
    if cancellation.is_cancelled() {
        return Err(ProbeError::Cancelled);
    }
    send()
}

#[cfg(unix)]
struct RawIcmpSocket {
    fd: OwnedFd,
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

    fn new_protocol(
        family: AddressFamily,
        protocol: libc::c_int,
        timeout: Duration,
    ) -> Result<Self, ProbeError> {
        let domain = match family {
            AddressFamily::Ipv4 => libc::AF_INET,
            AddressFamily::Ipv6 => libc::AF_INET6,
        };
        let raw_fd = unsafe { libc::socket(domain, libc::SOCK_RAW, protocol) };
        if raw_fd < 0 {
            return Err(socket_error(
                "open raw ICMP socket",
                io::Error::last_os_error(),
            ));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
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
        Ok(Self { fd })
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

    fn send_to(&self, packet: &[u8], target: SocketAddr) -> Result<(), ProbeError> {
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
        if sent < 0 {
            return Err(socket_error(
                "send raw ICMP probe",
                io::Error::last_os_error(),
            ));
        }
        if sent as usize != packet.len() {
            return Err(ProbeError::Io {
                operation: "send raw ICMP probe",
                message: format!("short write: sent {sent} of {} bytes", packet.len()),
            });
        }
        Ok(())
    }

    fn receive(&self) -> Result<ReceivedPacket, ProbeError> {
        let mut bytes = vec![0u8; 65_535];
        let mut storage = MaybeUninit::<libc::sockaddr_storage>::zeroed();
        let mut length = mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let received = unsafe {
            libc::recvfrom(
                self.fd.as_raw_fd(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                storage.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if received < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) {
                return Ok(None);
            }
            return Err(socket_error("receive raw ICMP response", error));
        }
        bytes.truncate(received as usize);
        let storage = unsafe { storage.assume_init() };
        Ok(Some((bytes, sockaddr_ip(&storage))))
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
        let raw_fd = unsafe { libc::socket(domain, libc::SOCK_RAW, libc::IPPROTO_RAW) };
        if raw_fd < 0 {
            return Err(socket_error(
                "open raw IP socket",
                io::Error::last_os_error(),
            ));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
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

    fn send_to(&self, packet: &[u8], target: SocketAddr) -> Result<(), ProbeError> {
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
        if sent < 0 {
            return Err(socket_error(
                "send raw TCP probe",
                io::Error::last_os_error(),
            ));
        }
        if sent as usize != packet.len() {
            return Err(ProbeError::Io {
                operation: "send raw TCP probe",
                message: format!("short write: sent {sent} of {} bytes", packet.len()),
            });
        }
        Ok(())
    }
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
    if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) {
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
