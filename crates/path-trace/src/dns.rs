use lantern_path_io::ProbeError;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub(crate) fn system_resolver() -> Option<SocketAddr> {
    #[cfg(unix)]
    {
        let text = std::fs::read_to_string("/etc/resolv.conf").ok()?;
        text.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next()? == "nameserver")
                .then(|| fields.next()?.parse::<IpAddr>().ok())
                .flatten()
                .map(|ip| SocketAddr::new(ip, 53))
        })
    }
    #[cfg(windows)]
    {
        windows_system_resolver()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_system_resolver() -> Option<SocketAddr> {
    use std::{ffi::CStr, mem::MaybeUninit, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR},
        NetworkManagement::IpHelper::{FIXED_INFO_W2KSP1, GetNetworkParams},
    };

    let mut length = 0u32;
    let first = unsafe { GetNetworkParams(ptr::null_mut(), &mut length) };
    if first != ERROR_BUFFER_OVERFLOW || length < size_of::<FIXED_INFO_W2KSP1>() as u32 {
        return None;
    }
    let mut storage = vec![MaybeUninit::<u8>::uninit(); length as usize];
    let info = storage.as_mut_ptr().cast::<FIXED_INFO_W2KSP1>();
    if unsafe { GetNetworkParams(info, &mut length) } != NO_ERROR {
        return None;
    }
    let mut server = unsafe { ptr::addr_of!((*info).DnsServerList) };
    while !server.is_null() {
        let text = unsafe { CStr::from_ptr((*server).IpAddress.String.as_ptr()) };
        if let Ok(value) = text.to_str()
            && let Ok(address) = value.parse::<IpAddr>()
            && !address.is_unspecified()
        {
            return Some(SocketAddr::new(address, 53));
        }
        server = unsafe { (*server).Next };
    }
    None
}

pub(crate) async fn reverse_name(
    address: IpAddr,
    resolver: SocketAddr,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Option<String>, ProbeError> {
    let name = reverse_pointer(address);
    query(name, 12, resolver, timeout, cancellation)
        .await
        .map(|records| records.into_iter().next())
}

pub(crate) async fn lookup_asn(
    address: IpAddr,
    resolver: SocketAddr,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Option<String>, ProbeError> {
    let name = match address {
        IpAddr::V4(address) => format!(
            "{}.{}.{}.{}.origin.asn.cymru.com",
            address.octets()[3],
            address.octets()[2],
            address.octets()[1],
            address.octets()[0]
        ),
        IpAddr::V6(address) => {
            let nibbles = address
                .octets()
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .collect::<Vec<_>>();
            format!(
                "{}.origin6.asn.cymru.com",
                nibbles
                    .iter()
                    .rev()
                    .map(|nibble| format!("{nibble:x}"))
                    .collect::<Vec<_>>()
                    .join(".")
            )
        }
    };
    query(name, 16, resolver, timeout, cancellation)
        .await
        .map(|records| {
            records.into_iter().next().and_then(|text| {
                text.split('|')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        })
}

async fn query(
    name: String,
    query_type: u16,
    resolver: SocketAddr,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Vec<String>, ProbeError> {
    let transaction = rand::random::<u16>();
    let request = build_query(transaction, &name, query_type)?;
    let bind = if resolver.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = tokio::select! {
        _ = cancellation.cancelled() => return Err(ProbeError::Cancelled),
        outcome = tokio::net::UdpSocket::bind(bind) => outcome
    }
    .map_err(|error| ProbeError::Io {
        operation: "bind DNS socket",
        message: error.to_string(),
    })?;
    tokio::select! {
        _ = cancellation.cancelled() => return Err(ProbeError::Cancelled),
        outcome = socket.connect(resolver) => outcome
    }
    .map_err(|error| ProbeError::Io {
        operation: "connect DNS socket",
        message: error.to_string(),
    })?;
    tokio::select! {
        _ = cancellation.cancelled() => return Err(ProbeError::Cancelled),
        outcome = socket.send(&request) => outcome
    }
    .map_err(|error| ProbeError::Io {
        operation: "send DNS query",
        message: error.to_string(),
    })?;
    let mut response = vec![0u8; 4096];
    let received = tokio::select! {
        _ = cancellation.cancelled() => return Err(ProbeError::Cancelled),
        outcome = tokio::time::timeout(timeout, socket.recv(&mut response)) => match outcome {
            Ok(Ok(received)) => received,
            Ok(Err(error)) => return Err(ProbeError::Io { operation: "receive DNS response", message: error.to_string() }),
            Err(_) => return Ok(Vec::new()),
        }
    };
    response.truncate(received);
    parse_response(transaction, query_type, &response)
}

fn build_query(transaction: u16, name: &str, query_type: u16) -> Result<Vec<u8>, ProbeError> {
    let mut packet = Vec::with_capacity(64);
    packet.extend_from_slice(&transaction.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(ProbeError::InvalidPlan("invalid DNS query label".into()));
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&query_type.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    Ok(packet)
}

fn parse_response(
    transaction: u16,
    query_type: u16,
    bytes: &[u8],
) -> Result<Vec<String>, ProbeError> {
    if bytes.len() < 12 || u16::from_be_bytes([bytes[0], bytes[1]]) != transaction {
        return Err(dns_parse("header or transaction mismatch"));
    }
    if bytes[3] & 0x0f != 0 {
        return Ok(Vec::new());
    }
    let questions = u16::from_be_bytes([bytes[4], bytes[5]]);
    let answers = u16::from_be_bytes([bytes[6], bytes[7]]);
    let mut offset = 12;
    for _ in 0..questions {
        offset = skip_name(bytes, offset)?;
        offset = offset
            .checked_add(4)
            .filter(|value| *value <= bytes.len())
            .ok_or_else(|| dns_parse("truncated question"))?;
    }
    let mut records = Vec::new();
    for _ in 0..answers {
        offset = skip_name(bytes, offset)?;
        if offset + 10 > bytes.len() {
            return Err(dns_parse("truncated answer"));
        }
        let record_type = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        let length = usize::from(u16::from_be_bytes([bytes[offset + 8], bytes[offset + 9]]));
        let data = offset + 10;
        if data + length > bytes.len() {
            return Err(dns_parse("truncated answer data"));
        }
        if record_type == query_type {
            match query_type {
                12 => {
                    let (_, name) = read_name(bytes, data, 0)?;
                    records.push(name);
                }
                16 => {
                    let mut cursor = data;
                    let end = data + length;
                    let mut text = String::new();
                    while cursor < end {
                        let part = usize::from(bytes[cursor]);
                        cursor += 1;
                        if cursor + part > end {
                            return Err(dns_parse("truncated TXT string"));
                        }
                        text.push_str(&String::from_utf8_lossy(&bytes[cursor..cursor + part]));
                        cursor += part;
                    }
                    records.push(text);
                }
                _ => {}
            }
        }
        offset = data + length;
    }
    Ok(records)
}

fn skip_name(bytes: &[u8], offset: usize) -> Result<usize, ProbeError> {
    read_name(bytes, offset, 0).map(|(next, _)| next)
}

fn read_name(bytes: &[u8], mut offset: usize, depth: u8) -> Result<(usize, String), ProbeError> {
    if depth > 16 {
        return Err(dns_parse("compression pointer loop"));
    }
    let mut labels = Vec::new();
    let mut next = None;
    loop {
        let length = *bytes
            .get(offset)
            .ok_or_else(|| dns_parse("truncated name"))?;
        if length & 0xc0 == 0xc0 {
            let low = *bytes
                .get(offset + 1)
                .ok_or_else(|| dns_parse("truncated compression pointer"))?;
            let pointer = (usize::from(length & 0x3f) << 8) | usize::from(low);
            next.get_or_insert(offset + 2);
            let (_, suffix) = read_name(bytes, pointer, depth + 1)?;
            if !suffix.is_empty() {
                labels.push(suffix);
            }
            break;
        }
        offset += 1;
        if length == 0 {
            break;
        }
        let end = offset + usize::from(length);
        if end > bytes.len() {
            return Err(dns_parse("truncated label"));
        }
        labels.push(String::from_utf8_lossy(&bytes[offset..end]).into_owned());
        offset = end;
    }
    Ok((next.unwrap_or(offset), labels.join(".")))
}

fn reverse_pointer(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => format!(
            "{}.{}.{}.{}.in-addr.arpa",
            address.octets()[3],
            address.octets()[2],
            address.octets()[1],
            address.octets()[0]
        ),
        IpAddr::V6(address) => format!(
            "{}.ip6.arpa",
            address
                .octets()
                .iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .rev()
                .map(|nibble| format!("{nibble:x}"))
                .collect::<Vec<_>>()
                .join(".")
        ),
    }
}

fn dns_parse(message: &str) -> ProbeError {
    ProbeError::Io {
        operation: "parse DNS response",
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_recorded_txt_fixture_and_ptr_names() {
        let mut response = build_query(0x1234, "8.8.8.8.origin.asn.cymru.com", 16).unwrap();
        response[2] = 0x81;
        response[3] = 0x80;
        response[6] = 0;
        response[7] = 1;
        response.extend_from_slice(&[0xc0, 0x0c, 0, 16, 0, 1, 0, 0, 0, 60, 0, 18, 17]);
        response.extend_from_slice(b"15169 | US | arin");
        assert_eq!(
            parse_response(0x1234, 16, &response).unwrap(),
            ["15169 | US | arin"]
        );
        assert_eq!(
            reverse_pointer("192.0.2.4".parse().unwrap()),
            "4.2.0.192.in-addr.arpa"
        );
        assert!(reverse_pointer(IpAddr::V6("2001:db8::1".parse().unwrap())).ends_with("ip6.arpa"));
    }
}
