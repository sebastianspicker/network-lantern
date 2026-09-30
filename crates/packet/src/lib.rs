//! Packet construction and decoding for Network Lantern path probes.
//!
//! This crate deliberately contains no socket or process access.  Callers can
//! construct a probe, send it through their OS boundary, and correlate a reply
//! using the same [`ProbeKey`].

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportProtocol {
    Icmp,
    Udp,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeKey {
    pub family: AddressFamily,
    pub protocol: TransportProtocol,
    pub identifier: Option<u16>,
    pub sequence: Option<u16>,
    pub source_port: Option<u16>,
    pub destination_port: Option<u16>,
    /// TCP sequence number when the sender controls it. Kernel-generated TCP
    /// probes may leave this unset and must use a uniquely bound source port.
    pub transport_sequence: Option<u32>,
}

impl ProbeKey {
    pub fn icmp(family: AddressFamily, identifier: u16, sequence: u16) -> Self {
        Self {
            family,
            protocol: TransportProtocol::Icmp,
            identifier: Some(identifier),
            sequence: Some(sequence),
            source_port: None,
            destination_port: None,
            transport_sequence: None,
        }
    }

    pub fn ports(
        family: AddressFamily,
        protocol: TransportProtocol,
        source_port: u16,
        destination_port: u16,
    ) -> Self {
        debug_assert!(matches!(
            protocol,
            TransportProtocol::Udp | TransportProtocol::Tcp
        ));
        Self {
            family,
            protocol,
            identifier: None,
            sequence: None,
            source_port: Some(source_port),
            destination_port: Some(destination_port),
            transport_sequence: None,
        }
    }

    pub fn tcp(
        family: AddressFamily,
        source_port: u16,
        destination_port: u16,
        sequence: Option<u32>,
    ) -> Self {
        let mut key = Self::ports(
            family,
            TransportProtocol::Tcp,
            source_port,
            destination_port,
        );
        key.transport_sequence = sequence;
        key
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IcmpKind {
    EchoReply,
    TimeExceeded,
    DestinationUnreachable,
    PacketTooBig,
    ParameterProblem,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MplsLabel {
    pub label: u32,
    pub traffic_class: u8,
    pub bottom_of_stack: bool,
    pub ttl: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedIcmp {
    pub family: AddressFamily,
    pub source: Option<IpAddr>,
    pub kind: IcmpKind,
    pub code: u8,
    pub echoed_identifier: Option<u16>,
    pub echoed_sequence: Option<u16>,
    pub quoted_probe: Option<ProbeKey>,
    pub next_hop_mtu: Option<u32>,
    pub mpls_labels: Vec<MplsLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTcp {
    pub family: AddressFamily,
    pub source: Option<IpAddr>,
    pub source_port: u16,
    pub destination_port: u16,
    pub sequence: u32,
    pub acknowledgment: u32,
    pub syn: bool,
    pub rst: bool,
    pub ack: bool,
}

impl ParsedIcmp {
    pub fn correlates(&self, key: &ProbeKey) -> bool {
        if self.family != key.family {
            return false;
        }
        match key.protocol {
            TransportProtocol::Icmp => self.quoted_probe.as_ref().map_or_else(
                || {
                    self.echoed_identifier == key.identifier
                        && self.echoed_sequence == key.sequence
                        && self.echoed_identifier.is_some()
                },
                |candidate| candidate == key,
            ),
            TransportProtocol::Udp => self
                .quoted_probe
                .as_ref()
                .is_some_and(|candidate| candidate == key),
            TransportProtocol::Tcp => self.quoted_probe.as_ref().is_some_and(|candidate| {
                candidate.family == key.family
                    && candidate.protocol == key.protocol
                    && candidate.source_port == key.source_port
                    && candidate.destination_port == key.destination_port
                    && key
                        .transport_sequence
                        .is_none_or(|sequence| candidate.transport_sequence == Some(sequence))
            }),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("packet is truncated: need {needed} bytes, have {actual}")]
    Truncated { needed: usize, actual: usize },
    #[error("unsupported IP version {0}")]
    UnsupportedIpVersion(u8),
    #[error("outer packet is not ICMP (protocol {0})")]
    NotIcmp(u8),
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
}

/// RFC 1071 one's-complement checksum.
pub fn internet_checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in bytes.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_be_bytes([chunk[0], 0])
        };
        sum = sum.wrapping_add(u32::from(word));
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
    }
    !(sum as u16)
}

pub fn build_icmpv4_echo(identifier: u16, sequence: u16, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(8 + payload.len());
    packet.extend_from_slice(&[8, 0, 0, 0]);
    packet.extend_from_slice(&identifier.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(payload);
    let checksum = internet_checksum(&packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    packet
}

/// Construct ICMPv6 Echo Request bytes, including its IPv6 pseudo-header checksum.
pub fn build_icmpv6_echo(
    source: std::net::Ipv6Addr,
    destination: std::net::Ipv6Addr,
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut packet = Vec::with_capacity(8 + payload.len());
    packet.extend_from_slice(&[128, 0, 0, 0]);
    packet.extend_from_slice(&identifier.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(payload);
    let mut checksum_input = Vec::with_capacity(40 + packet.len());
    checksum_input.extend_from_slice(&source.octets());
    checksum_input.extend_from_slice(&destination.octets());
    checksum_input.extend_from_slice(&(packet.len() as u32).to_be_bytes());
    checksum_input.extend_from_slice(&[0, 0, 0, 58]);
    checksum_input.extend_from_slice(&packet);
    let checksum = internet_checksum(&checksum_input);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    packet
}

pub fn transport_checksum_ipv4(
    source: std::net::Ipv4Addr,
    destination: std::net::Ipv4Addr,
    protocol: u8,
    segment: &[u8],
) -> u16 {
    let mut pseudo = Vec::with_capacity(12 + segment.len());
    pseudo.extend_from_slice(&source.octets());
    pseudo.extend_from_slice(&destination.octets());
    pseudo.extend_from_slice(&[0, protocol]);
    pseudo.extend_from_slice(&(segment.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(segment);
    internet_checksum(&pseudo)
}

pub fn transport_checksum_ipv6(
    source: std::net::Ipv6Addr,
    destination: std::net::Ipv6Addr,
    protocol: u8,
    segment: &[u8],
) -> u16 {
    let mut pseudo = Vec::with_capacity(40 + segment.len());
    pseudo.extend_from_slice(&source.octets());
    pseudo.extend_from_slice(&destination.octets());
    pseudo.extend_from_slice(&(segment.len() as u32).to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, protocol]);
    pseudo.extend_from_slice(segment);
    internet_checksum(&pseudo)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpSynSpec {
    pub source_port: u16,
    pub destination_port: u16,
    pub sequence: u32,
    pub hop_limit: u8,
    pub traffic_class: u8,
    pub dont_fragment: bool,
}

pub fn build_tcp_syn_ipv4(
    source: std::net::Ipv4Addr,
    destination: std::net::Ipv4Addr,
    spec: &TcpSynSpec,
) -> Vec<u8> {
    let mut packet = vec![0u8; 40];
    packet[0] = 0x45;
    packet[1] = spec.traffic_class;
    packet[2..4].copy_from_slice(&40u16.to_be_bytes());
    packet[4..6].copy_from_slice(&(spec.sequence as u16).to_be_bytes());
    if spec.dont_fragment {
        packet[6..8].copy_from_slice(&0x4000u16.to_be_bytes());
    }
    packet[8] = spec.hop_limit;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&source.octets());
    packet[16..20].copy_from_slice(&destination.octets());
    packet[20..22].copy_from_slice(&spec.source_port.to_be_bytes());
    packet[22..24].copy_from_slice(&spec.destination_port.to_be_bytes());
    packet[24..28].copy_from_slice(&spec.sequence.to_be_bytes());
    packet[32] = 5 << 4;
    packet[33] = 0x02;
    packet[34..36].copy_from_slice(&64240u16.to_be_bytes());
    let tcp_checksum = transport_checksum_ipv4(source, destination, 6, &packet[20..]);
    packet[36..38].copy_from_slice(&tcp_checksum.to_be_bytes());
    let ip_checksum = internet_checksum(&packet[..20]);
    packet[10..12].copy_from_slice(&ip_checksum.to_be_bytes());
    packet
}

pub fn build_tcp_syn_ipv6(
    source: std::net::Ipv6Addr,
    destination: std::net::Ipv6Addr,
    spec: &TcpSynSpec,
) -> Vec<u8> {
    let mut packet = vec![0u8; 60];
    packet[0] = 0x60 | (spec.traffic_class >> 4);
    packet[1] = spec.traffic_class << 4;
    packet[4..6].copy_from_slice(&20u16.to_be_bytes());
    packet[6] = 6;
    packet[7] = spec.hop_limit;
    packet[8..24].copy_from_slice(&source.octets());
    packet[24..40].copy_from_slice(&destination.octets());
    packet[40..42].copy_from_slice(&spec.source_port.to_be_bytes());
    packet[42..44].copy_from_slice(&spec.destination_port.to_be_bytes());
    packet[44..48].copy_from_slice(&spec.sequence.to_be_bytes());
    packet[52] = 5 << 4;
    packet[53] = 0x02;
    packet[54..56].copy_from_slice(&64240u16.to_be_bytes());
    let checksum = transport_checksum_ipv6(source, destination, 6, &packet[40..]);
    packet[56..58].copy_from_slice(&checksum.to_be_bytes());
    packet
}

pub fn parse_tcp_response(bytes: &[u8], expected: AddressFamily) -> Result<ParsedTcp, PacketError> {
    need(bytes, 20)?;
    let (family, source, segment) = match bytes[0] >> 4 {
        4 => {
            let header_len = usize::from(bytes[0] & 0x0f) * 4;
            if header_len < 20 {
                return Err(PacketError::Malformed("invalid IPv4 header length"));
            }
            need(bytes, header_len + 20)?;
            if bytes[9] != 6 {
                return Err(PacketError::Malformed("outer IPv4 packet is not TCP"));
            }
            let source = std::net::Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]);
            (
                AddressFamily::Ipv4,
                Some(IpAddr::V4(source)),
                &bytes[header_len..],
            )
        }
        6 => {
            need(bytes, 40)?;
            let (protocol, offset) = ipv6_transport(bytes)?;
            if protocol != 6 {
                return Err(PacketError::Malformed("outer IPv6 packet is not TCP"));
            }
            need(bytes, offset + 20)?;
            let source = std::net::Ipv6Addr::from(
                <[u8; 16]>::try_from(&bytes[8..24]).expect("fixed IPv6 source slice"),
            );
            (
                AddressFamily::Ipv6,
                Some(IpAddr::V6(source)),
                &bytes[offset..],
            )
        }
        _ => (expected, None, bytes),
    };
    need(segment, 20)?;
    Ok(ParsedTcp {
        family,
        source,
        source_port: be_u16(&segment[..2]),
        destination_port: be_u16(&segment[2..4]),
        sequence: be_u32(&segment[4..8]),
        acknowledgment: be_u32(&segment[8..12]),
        syn: segment[13] & 0x02 != 0,
        rst: segment[13] & 0x04 != 0,
        ack: segment[13] & 0x10 != 0,
    })
}

/// Decode an ICMP response. `bytes` may include an outer IPv4/IPv6 header or
/// start directly at the ICMP header, as raw-socket behavior differs by OS.
pub fn parse_icmp_response(
    bytes: &[u8],
    expected_family: AddressFamily,
) -> Result<ParsedIcmp, PacketError> {
    let (family, source, icmp) = strip_outer_ip(bytes, expected_family)?;
    need(icmp, 8)?;
    let kind = match (family, icmp[0]) {
        (AddressFamily::Ipv4, 0) | (AddressFamily::Ipv6, 129) => IcmpKind::EchoReply,
        (AddressFamily::Ipv4, 11) | (AddressFamily::Ipv6, 3) => IcmpKind::TimeExceeded,
        (AddressFamily::Ipv4, 3) | (AddressFamily::Ipv6, 1) => IcmpKind::DestinationUnreachable,
        (AddressFamily::Ipv6, 2) => IcmpKind::PacketTooBig,
        (AddressFamily::Ipv4, 12) | (AddressFamily::Ipv6, 4) => IcmpKind::ParameterProblem,
        _ => IcmpKind::Other,
    };
    let is_echo = kind == IcmpKind::EchoReply;
    let (echoed_identifier, echoed_sequence) = if is_echo {
        (Some(be_u16(&icmp[4..6])), Some(be_u16(&icmp[6..8])))
    } else {
        (None, None)
    };
    let next_hop_mtu = match (family, icmp[0], icmp[1]) {
        (AddressFamily::Ipv4, 3, 4) => Some(u32::from(be_u16(&icmp[6..8]))),
        (AddressFamily::Ipv6, 2, _) => Some(be_u32(&icmp[4..8])),
        _ => None,
    };
    let quoted_probe = if matches!(
        kind,
        IcmpKind::TimeExceeded
            | IcmpKind::DestinationUnreachable
            | IcmpKind::PacketTooBig
            | IcmpKind::ParameterProblem
    ) {
        parse_quoted_probe(&icmp[8..]).ok()
    } else {
        None
    };
    let mpls_labels = find_mpls_extension(family, kind, icmp);
    Ok(ParsedIcmp {
        family,
        source,
        kind,
        code: icmp[1],
        echoed_identifier,
        echoed_sequence,
        quoted_probe,
        next_hop_mtu,
        mpls_labels,
    })
}

fn strip_outer_ip(
    bytes: &[u8],
    expected: AddressFamily,
) -> Result<(AddressFamily, Option<IpAddr>, &[u8]), PacketError> {
    need(bytes, 1)?;
    match bytes[0] >> 4 {
        4 => {
            need(bytes, 20)?;
            let header_len = usize::from(bytes[0] & 0x0f) * 4;
            if header_len < 20 {
                return Err(PacketError::Malformed("invalid IPv4 header length"));
            }
            need(bytes, header_len + 8)?;
            if bytes[9] != 1 {
                return Err(PacketError::NotIcmp(bytes[9]));
            }
            let source = std::net::Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]);
            Ok((
                AddressFamily::Ipv4,
                Some(IpAddr::V4(source)),
                &bytes[header_len..],
            ))
        }
        6 => {
            need(bytes, 40)?;
            let (protocol, offset) = ipv6_transport(bytes)?;
            if protocol != 58 {
                return Err(PacketError::NotIcmp(protocol));
            }
            need(bytes, offset + 8)?;
            let source = std::net::Ipv6Addr::from(
                <[u8; 16]>::try_from(&bytes[8..24]).expect("fixed IPv6 source slice"),
            );
            Ok((
                AddressFamily::Ipv6,
                Some(IpAddr::V6(source)),
                &bytes[offset..],
            ))
        }
        version if bytes.len() >= 8 && version != 4 && version != 6 => Ok((expected, None, bytes)),
        version => Err(PacketError::UnsupportedIpVersion(version)),
    }
}

fn parse_quoted_probe(bytes: &[u8]) -> Result<ProbeKey, PacketError> {
    need(bytes, 1)?;
    match bytes[0] >> 4 {
        4 => {
            need(bytes, 20)?;
            let header_len = usize::from(bytes[0] & 0x0f) * 4;
            if header_len < 20 {
                return Err(PacketError::Malformed("invalid quoted IPv4 header length"));
            }
            parse_transport(AddressFamily::Ipv4, bytes[9], bytes, header_len)
        }
        6 => {
            need(bytes, 40)?;
            let (protocol, offset) = ipv6_transport(bytes)?;
            parse_transport(AddressFamily::Ipv6, protocol, bytes, offset)
        }
        version => Err(PacketError::UnsupportedIpVersion(version)),
    }
}

fn parse_transport(
    family: AddressFamily,
    protocol: u8,
    bytes: &[u8],
    offset: usize,
) -> Result<ProbeKey, PacketError> {
    need(bytes, offset + 8)?;
    match protocol {
        1 | 58 => Ok(ProbeKey::icmp(
            family,
            be_u16(&bytes[offset + 4..offset + 6]),
            be_u16(&bytes[offset + 6..offset + 8]),
        )),
        17 => Ok(ProbeKey::ports(
            family,
            TransportProtocol::Udp,
            be_u16(&bytes[offset..offset + 2]),
            be_u16(&bytes[offset + 2..offset + 4]),
        )),
        6 => Ok(ProbeKey::tcp(
            family,
            be_u16(&bytes[offset..offset + 2]),
            be_u16(&bytes[offset + 2..offset + 4]),
            Some(be_u32(&bytes[offset + 4..offset + 8])),
        )),
        _ => Err(PacketError::Malformed(
            "unsupported quoted transport protocol",
        )),
    }
}

/// Parse RFC 4950 MPLS label-stack objects from an RFC 4884 extension block.
pub fn parse_mpls_extensions(bytes: &[u8]) -> Result<Vec<MplsLabel>, PacketError> {
    need(bytes, 4)?;
    if bytes[0] >> 4 != 2 || bytes[0] & 0x0f != 0 || bytes[1] != 0 {
        return Err(PacketError::Malformed("ICMP extension version is not 2"));
    }
    let transmitted_checksum = be_u16(&bytes[2..4]);
    if transmitted_checksum != 0 && internet_checksum(bytes) != 0 {
        return Err(PacketError::Malformed("invalid ICMP extension checksum"));
    }
    let mut labels = Vec::new();
    let mut offset = 4;
    while offset < bytes.len() {
        need(&bytes[offset..], 4)?;
        let length = usize::from(be_u16(&bytes[offset..offset + 2]));
        if length < 4 || offset + length > bytes.len() {
            return Err(PacketError::Malformed(
                "invalid ICMP extension object length",
            ));
        }
        let class_num = bytes[offset + 2];
        let c_type = bytes[offset + 3];
        if class_num == 1 && c_type == 1 {
            let payload = &bytes[offset + 4..offset + length];
            if !payload.len().is_multiple_of(4) {
                return Err(PacketError::Malformed("misaligned MPLS label stack"));
            }
            for word in payload.chunks_exact(4) {
                let value = be_u32(word);
                labels.push(MplsLabel {
                    label: value >> 12,
                    traffic_class: ((value >> 9) & 0x7) as u8,
                    bottom_of_stack: value & 0x100 != 0,
                    ttl: value as u8,
                });
            }
        }
        offset += length;
    }
    Ok(labels)
}

fn find_mpls_extension(family: AddressFamily, kind: IcmpKind, icmp: &[u8]) -> Vec<MplsLabel> {
    if !matches!(
        kind,
        IcmpKind::TimeExceeded | IcmpKind::DestinationUnreachable
    ) {
        return Vec::new();
    }
    let units = match family {
        AddressFamily::Ipv4 => icmp.get(5).copied().unwrap_or(0),
        AddressFamily::Ipv6 => icmp.get(4).copied().unwrap_or(0),
    };
    let unit_size = if family == AddressFamily::Ipv4 { 4 } else { 8 };
    let original_length = if units == 0 {
        // Compatibility mode from RFC 4884 section 5.5: early MPLS-aware
        // implementations used a fixed 128-byte original datagram.
        128
    } else {
        usize::from(units) * unit_size
    };
    if original_length < 128 {
        return Vec::new();
    }
    let Some(extension_offset) = 8usize.checked_add(original_length) else {
        return Vec::new();
    };
    icmp.get(extension_offset..)
        .and_then(|extension| parse_mpls_extensions(extension).ok())
        .unwrap_or_default()
}

/// Return the final next-header value and transport offset after bounded IPv6
/// extension traversal. Non-first fragments cannot contain a parseable
/// transport header and are rejected.
fn ipv6_transport(bytes: &[u8]) -> Result<(u8, usize), PacketError> {
    need(bytes, 40)?;
    let mut next = bytes[6];
    let mut offset = 40usize;
    for _ in 0..16 {
        let length = match next {
            0 | 43 | 60 => {
                need(bytes, offset + 2)?;
                let value = (usize::from(bytes[offset + 1]) + 1) * 8;
                let following = bytes[offset];
                next = following;
                value
            }
            44 => {
                need(bytes, offset + 8)?;
                let fragment = be_u16(&bytes[offset + 2..offset + 4]);
                if fragment & 0xfff8 != 0 {
                    return Err(PacketError::Malformed("non-first IPv6 fragment"));
                }
                next = bytes[offset];
                8
            }
            51 => {
                need(bytes, offset + 2)?;
                let value = (usize::from(bytes[offset + 1]) + 2) * 4;
                next = bytes[offset];
                value
            }
            _ => return Ok((next, offset)),
        };
        if length == 0 {
            return Err(PacketError::Malformed("zero-length IPv6 extension"));
        }
        offset = offset
            .checked_add(length)
            .ok_or(PacketError::Malformed("IPv6 extension length overflow"))?;
        need(bytes, offset)?;
    }
    Err(PacketError::Malformed("too many IPv6 extension headers"))
}

fn need(bytes: &[u8], needed: usize) -> Result<(), PacketError> {
    if bytes.len() < needed {
        Err(PacketError::Truncated {
            needed,
            actual: bytes.len(),
        })
    } else {
        Ok(())
    }
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn checksum_matches_rfc_1071_vector() {
        assert_eq!(
            internet_checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]),
            0x220d
        );
        let echo = build_icmpv4_echo(0x1234, 7, b"lantern");
        assert_eq!(internet_checksum(&echo), 0);
    }

    #[test]
    fn parses_ipv4_echo_reply_fixture() {
        let mut packet = vec![
            0x45, 0, 0, 28, 0, 0, 0, 0, 64, 1, 0, 0, 192, 0, 2, 1, 192, 0, 2, 2,
        ];
        packet.extend_from_slice(&[0, 0, 0, 0, 0x12, 0x34, 0, 7]);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv4).unwrap();
        assert_eq!(parsed.source, Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))));
        assert_eq!(parsed.kind, IcmpKind::EchoReply);
        assert!(parsed.correlates(&ProbeKey::icmp(AddressFamily::Ipv4, 0x1234, 7)));
        assert!(!parsed.correlates(&ProbeKey::icmp(AddressFamily::Ipv4, 0x1234, 8)));
    }

    #[test]
    fn correlates_ipv4_time_exceeded_udp_fixture() {
        let mut packet = vec![
            0x45, 0, 0, 56, 0, 0, 0, 0, 64, 1, 0, 0, 198, 51, 100, 1, 192, 0, 2, 1,
        ];
        packet.extend_from_slice(&[11, 0, 0, 0, 0, 0, 0, 0]);
        packet.extend_from_slice(&[
            0x45, 0, 0, 28, 0, 0, 0, 0, 1, 17, 0, 0, 192, 0, 2, 1, 203, 0, 113, 8,
        ]);
        packet.extend_from_slice(&[0xc3, 0x50, 0x82, 0x9a, 0, 8, 0, 0]);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv4).unwrap();
        let key = ProbeKey::ports(AddressFamily::Ipv4, TransportProtocol::Udp, 50000, 33434);
        assert_eq!(parsed.kind, IcmpKind::TimeExceeded);
        assert!(parsed.correlates(&key));
    }

    #[test]
    fn parses_ipv6_packet_too_big_and_quoted_tcp_fixture() {
        let mut packet = vec![0x60, 0, 0, 0, 0, 56, 58, 64];
        packet.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        packet.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0x05, 0xdc]);
        packet.extend_from_slice(&[0x60, 0, 0, 0, 0, 8, 6, 1]);
        packet.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        packet.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet.extend_from_slice(&[0xc3, 0x51, 0x01, 0xbb, 0, 0, 0, 0]);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv6).unwrap();
        assert_eq!(parsed.next_hop_mtu, Some(1500));
        assert!(parsed.correlates(&ProbeKey::tcp(AddressFamily::Ipv6, 50001, 443, Some(0),)));
        assert!(!parsed.correlates(&ProbeKey::tcp(AddressFamily::Ipv6, 50001, 443, Some(1),)));
    }

    #[test]
    fn parses_rfc_4950_mpls_stack_fixture() {
        let extension = [0x20, 0, 0, 0, 0, 12, 1, 1, 0, 0, 0x10, 64, 0, 0, 0x21, 63];
        let labels = parse_mpls_extensions(&extension).unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].label, 1);
        assert!(!labels[0].bottom_of_stack);
        assert_eq!(labels[1].label, 2);
        assert!(labels[1].bottom_of_stack);
    }

    #[test]
    fn icmpv6_checksum_uses_pseudo_header() {
        let packet = build_icmpv6_echo(Ipv6Addr::LOCALHOST, Ipv6Addr::LOCALHOST, 9, 2, b"x");
        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        pseudo.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        pseudo.extend_from_slice(&(packet.len() as u32).to_be_bytes());
        pseudo.extend_from_slice(&[0, 0, 0, 58]);
        pseudo.extend_from_slice(&packet);
        assert_eq!(internet_checksum(&pseudo), 0);
    }

    #[test]
    fn tcp_syn_builders_have_valid_network_checksums() {
        let spec = TcpSynSpec {
            source_port: 50000,
            destination_port: 443,
            sequence: 7,
            hop_limit: 3,
            traffic_class: 160,
            dont_fragment: true,
        };
        let v4 = build_tcp_syn_ipv4(Ipv4Addr::LOCALHOST, Ipv4Addr::new(192, 0, 2, 1), &spec);
        assert_eq!(internet_checksum(&v4[..20]), 0);
        assert_eq!(
            transport_checksum_ipv4(
                Ipv4Addr::LOCALHOST,
                Ipv4Addr::new(192, 0, 2, 1),
                6,
                &v4[20..]
            ),
            0
        );
        let v6_spec = TcpSynSpec {
            traffic_class: 40,
            ..spec
        };
        let v6 = build_tcp_syn_ipv6(Ipv6Addr::LOCALHOST, Ipv6Addr::LOCALHOST, &v6_spec);
        assert_eq!(
            transport_checksum_ipv6(Ipv6Addr::LOCALHOST, Ipv6Addr::LOCALHOST, 6, &v6[40..]),
            0
        );
    }

    #[test]
    fn parses_recorded_tcp_syn_ack_fixture() {
        let spec = TcpSynSpec {
            source_port: 443,
            destination_port: 50000,
            sequence: 9,
            hop_limit: 64,
            traffic_class: 0,
            dont_fragment: false,
        };
        let mut packet =
            build_tcp_syn_ipv4(Ipv4Addr::new(192, 0, 2, 1), Ipv4Addr::LOCALHOST, &spec);
        packet[33] = 0x12;
        packet[28..32].copy_from_slice(&8u32.to_be_bytes());
        let parsed = parse_tcp_response(&packet, AddressFamily::Ipv4).unwrap();
        assert!(parsed.syn && parsed.ack);
        assert_eq!(parsed.destination_port, 50000);
        assert_eq!(parsed.acknowledgment, 8);
    }

    #[test]
    fn walks_ipv6_extensions_for_outer_and_quoted_transports() {
        let mut outer = vec![0x60, 0, 0, 0, 0, 0, 0, 64];
        outer.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        outer.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        outer.extend_from_slice(&[58, 0, 0, 0, 0, 0, 0, 0]);
        outer.extend_from_slice(&[3, 0, 0, 0, 0, 0, 0, 0]);
        outer.extend_from_slice(&[0x60, 0, 0, 0, 0, 16, 60, 1]);
        outer.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        outer.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        outer.extend_from_slice(&[6, 0, 0, 0, 0, 0, 0, 0]);
        outer.extend_from_slice(&[0xc3, 0x50, 0x01, 0xbb, 0, 0, 0, 9]);
        let parsed = parse_icmp_response(&outer, AddressFamily::Ipv6).unwrap();
        assert!(parsed.correlates(&ProbeKey::tcp(AddressFamily::Ipv6, 50_000, 443, Some(9),)));

        let mut tcp = build_tcp_syn_ipv6(
            Ipv6Addr::LOCALHOST,
            Ipv6Addr::LOCALHOST,
            &TcpSynSpec {
                source_port: 443,
                destination_port: 50_000,
                sequence: 4,
                hop_limit: 64,
                traffic_class: 0,
                dont_fragment: false,
            },
        );
        tcp[6] = 0;
        tcp.splice(40..40, [6, 0, 0, 0, 0, 0, 0, 0]);
        let parsed = parse_tcp_response(&tcp, AddressFamily::Ipv6).unwrap();
        assert_eq!(parsed.destination_port, 50_000);
    }

    #[test]
    fn rejects_non_first_ipv6_fragment() {
        let mut packet = vec![0x60, 0, 0, 0, 0, 0, 44, 64];
        packet.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        packet.extend_from_slice(&[58, 0, 0, 8, 0, 0, 0, 1]);
        packet.extend_from_slice(&[129, 0, 0, 0, 0, 1, 0, 1]);
        assert_eq!(
            parse_icmp_response(&packet, AddressFamily::Ipv6),
            Err(PacketError::Malformed("non-first IPv6 fragment"))
        );
    }

    #[test]
    fn rejects_non_first_fragment_in_quoted_probe() {
        let mut packet = vec![3, 0, 0, 0, 0, 0, 0, 0];
        packet.extend_from_slice(&[0x60, 0, 0, 0, 0, 16, 44, 1]);
        packet.extend_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        packet.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet.extend_from_slice(&[17, 0, 0, 8, 0, 0, 0, 1]);
        packet.extend_from_slice(&[0xc3, 0x50, 0x82, 0x9a, 0, 8, 0, 0]);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv6).unwrap();
        assert_eq!(parsed.kind, IcmpKind::TimeExceeded);
        assert!(parsed.quoted_probe.is_none());
    }

    #[test]
    fn locates_ipv6_mpls_extension_from_rfc4884_length() {
        let mut packet = vec![3, 0, 0, 0, 16, 0, 0, 0];
        let mut quote = vec![0u8; 128];
        quote[0] = 0x60;
        quote[6] = 58;
        quote[40] = 128;
        packet.extend_from_slice(&quote);
        let mut extension = vec![0x20, 0, 0, 0, 0, 8, 1, 1, 0, 0, 0x11, 64];
        let checksum = internet_checksum(&extension);
        extension[2..4].copy_from_slice(&checksum.to_be_bytes());
        packet.extend_from_slice(&extension);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv6).unwrap();
        assert_eq!(parsed.mpls_labels.len(), 1);
        assert_eq!(parsed.mpls_labels[0].label, 1);
    }

    #[test]
    fn locates_checksummed_mpls_extension_from_rfc4884_length() {
        let mut packet = vec![
            0x45, 0, 0, 0, 0, 0, 0, 0, 64, 1, 0, 0, 192, 0, 2, 1, 192, 0, 2, 2,
        ];
        packet.extend_from_slice(&[11, 0, 0, 0, 0, 32, 0, 0]);
        let mut quote = vec![0u8; 128];
        quote[0] = 0x45;
        quote[9] = 1;
        quote[20] = 8;
        quote[24] = 0x12;
        quote[25] = 0x34;
        quote[27] = 1;
        packet.extend_from_slice(&quote);
        let mut extension = vec![0x20, 0, 0, 0, 0, 8, 1, 1, 0, 0, 0x11, 64];
        let checksum = internet_checksum(&extension);
        extension[2..4].copy_from_slice(&checksum.to_be_bytes());
        packet.extend_from_slice(&extension);
        let parsed = parse_icmp_response(&packet, AddressFamily::Ipv4).unwrap();
        assert_eq!(parsed.mpls_labels.len(), 1);
        assert_eq!(parsed.mpls_labels[0].label, 1);

        let last = packet.len() - 1;
        packet[last] ^= 1;
        assert!(
            parse_icmp_response(&packet, AddressFamily::Ipv4)
                .unwrap()
                .mpls_labels
                .is_empty()
        );
    }
}
