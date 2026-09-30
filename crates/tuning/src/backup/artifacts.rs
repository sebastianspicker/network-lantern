use crate::{Result, TuningError};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryValue {
    Missing,
    Dword(u32),
    String(String),
}

pub type RegistryBackup = BTreeMap<String, BTreeMap<String, RegistryValue>>;

pub fn parse_registry_backup(bytes: &[u8], system_profile: bool) -> Result<RegistryBackup> {
    let text = decode_reg(bytes)?;
    let expected = if system_profile {
        vec![
            (
                "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile",
                vec![
                    ("SystemResponsiveness", false),
                    ("NetworkThrottlingIndex", false),
                ],
            ),
            (
                "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile\\Tasks\\Audio",
                vec![
                    ("Priority", false),
                    ("BackgroundOnly", false),
                    ("Clock Rate", false),
                    ("SchedulingCategory", true),
                    ("SFIOPriority", true),
                ],
            ),
        ]
    } else {
        vec![(
            "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\AFD\\Parameters",
            vec![("FastSendDatagramThreshold", false)],
        )]
    };
    let mut allowed = BTreeMap::new();
    for (section, values) in &expected {
        allowed.insert(*section, values.iter().copied().collect::<BTreeMap<_, _>>());
    }
    let mut result: RegistryBackup = BTreeMap::new();
    let mut current: Option<&str> = None;
    let mut header = false;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if !header {
            if line != "Windows Registry Editor Version 5.00" {
                return invalid("registry backup has an invalid header");
            }
            header = true;
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            if !allowed.contains_key(section) || result.contains_key(section) {
                return invalid("registry backup contains an unknown or duplicate key");
            }
            result.insert(section.to_owned(), BTreeMap::new());
            current = Some(section);
            continue;
        }
        let section = current.ok_or_else(|| {
            TuningError::BackupInvalid("registry value appears before a key".into())
        })?;
        let (quoted_name, data) = line
            .split_once('=')
            .ok_or_else(|| TuningError::BackupInvalid("malformed registry value".into()))?;
        let name = quoted_name
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(|| TuningError::BackupInvalid("malformed registry value name".into()))?;
        let string_type = *allowed
            .get(section)
            .and_then(|values| values.get(name))
            .ok_or_else(|| TuningError::BackupInvalid("unapproved registry value".into()))?;
        let value = if data == "-" {
            RegistryValue::Missing
        } else if string_type {
            RegistryValue::String(parse_reg_string(data)?)
        } else {
            let hex = data
                .strip_prefix("dword:")
                .filter(|value| value.len() == 8 && value.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| TuningError::BackupInvalid("invalid registry DWORD".into()))?;
            RegistryValue::Dword(
                u32::from_str_radix(hex, 16)
                    .map_err(|_| TuningError::BackupInvalid("invalid registry DWORD".into()))?,
            )
        };
        let entries = result.get_mut(section).expect("current section exists");
        if entries.insert(name.to_owned(), value).is_some() {
            return invalid("duplicate registry value");
        }
    }
    if !header || result.len() != allowed.len() {
        return invalid("registry backup is missing a required key");
    }
    for (section, values) in allowed {
        let parsed = result
            .get(section)
            .expect("section count and names validated");
        if parsed.len() != values.len() || values.keys().any(|name| !parsed.contains_key(*name)) {
            return invalid("registry backup is missing required value state");
        }
    }
    Ok(result)
}

fn decode_reg(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return invalid("registry backup has malformed UTF-16");
        }
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words)
            .map_err(|_| TuningError::BackupInvalid("registry backup has invalid UTF-16".into()))
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| TuningError::BackupInvalid("registry backup is not text".into()))
    }
}

fn parse_reg_string(data: &str) -> Result<String> {
    let inner = data
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| TuningError::BackupInvalid("invalid registry string".into()))?;
    let mut result = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                _ => return invalid("registry string has an unsupported escape"),
            }
        } else if matches!(ch, '\0' | '\r' | '\n') {
            return invalid("registry string contains a control character");
        } else {
            result.push(ch);
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "Type")]
pub enum QosPolicySpec {
    Port {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "Protocol")]
        protocol: String,
        #[serde(rename = "Port")]
        port: u16,
        #[serde(rename = "Dscp")]
        dscp: u8,
    },
    App {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "AppPath")]
        app_path: String,
        #[serde(rename = "Dscp")]
        dscp: u8,
    },
}

impl QosPolicySpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Port { name, .. } | Self::App { name, .. } => name,
        }
    }

    #[cfg(windows)]
    pub(crate) fn port_parts(&self) -> Option<(&str, u16, u8)> {
        match self {
            Self::Port {
                protocol,
                port,
                dscp,
                ..
            } => Some((protocol, *port, *dscp)),
            Self::App { .. } => None,
        }
    }

    #[cfg(windows)]
    pub(crate) fn app_parts(&self) -> Option<(&str, u8)> {
        match self {
            Self::App { app_path, dscp, .. } => Some((app_path, *dscp)),
            Self::Port { .. } => None,
        }
    }

    #[cfg(windows)]
    pub(crate) fn port(name: String, protocol: String, port: u16, dscp: u8) -> Self {
        Self::Port {
            name,
            protocol,
            port,
            dscp,
        }
    }

    #[cfg(windows)]
    pub(crate) fn app(name: String, app_path: String, dscp: u8) -> Self {
        Self::App {
            name,
            app_path,
            dscp,
        }
    }
}

pub fn parse_qos_clixml(bytes: &[u8]) -> Result<Vec<QosPolicySpec>> {
    if bytes.len() as u64 > super::MAX_QOS_BYTES {
        return invalid("QoS CLIXML exceeds 4 MiB");
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TuningError::BackupInvalid("QoS CLIXML must be UTF-8".into()))?;
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    reader.config_mut().trim_text(true);
    let mut stack = Vec::<Vec<u8>>::new();
    let mut item: Option<BTreeMap<String, String>> = None;
    let mut property: Option<(String, Vec<u8>)> = None;
    let mut raw_items = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if stack.len() >= 32 || !allowed_xml_tag(event.name().as_ref()) {
                    return invalid("QoS CLIXML has an unsupported element or depth");
                }
                let name = event.name().as_ref().to_vec();
                if name.as_slice() == b"MS" {
                    if item.is_some() {
                        return invalid("QoS CLIXML has nested property bags");
                    }
                    item = Some(BTreeMap::new());
                }
                if scalar_tag(&name) {
                    let mut prop_name = None;
                    for attribute in event.attributes().with_checks(true) {
                        let attribute = attribute.map_err(|_| {
                            TuningError::BackupInvalid("invalid QoS CLIXML attribute".into())
                        })?;
                        if attribute.key.as_ref() == b"N" {
                            prop_name = Some(
                                attribute
                                    .unescape_value()
                                    .map_err(|_| {
                                        TuningError::BackupInvalid(
                                            "invalid QoS property name".into(),
                                        )
                                    })?
                                    .into_owned(),
                            );
                        }
                    }
                    if let Some(prop_name) = prop_name {
                        if item.is_none() || property.is_some() {
                            return invalid("QoS CLIXML property is outside an item");
                        }
                        property = Some((prop_name, Vec::new()));
                    }
                }
                stack.push(name);
            }
            Ok(Event::Empty(event)) => {
                if !allowed_xml_tag(event.name().as_ref()) {
                    return invalid("QoS CLIXML has an unsupported empty element");
                }
            }
            Ok(Event::Text(value)) => {
                if let Some((_, text)) = property.as_mut() {
                    let value = value.decode().map_err(|_| {
                        TuningError::BackupInvalid("invalid QoS CLIXML text".into())
                    })?;
                    if text.len() + value.len() > 32 * 1024 {
                        return invalid("QoS CLIXML property is too long");
                    }
                    text.extend_from_slice(value.as_bytes());
                }
            }
            Ok(Event::End(event)) => {
                let name = stack
                    .pop()
                    .ok_or_else(|| TuningError::BackupInvalid("unbalanced QoS CLIXML".into()))?;
                if name.as_slice() != event.name().as_ref() {
                    return invalid("unbalanced QoS CLIXML");
                }
                if scalar_tag(&name) && property.is_some() {
                    let (name, value) = property.take().expect("checked");
                    let value = String::from_utf8(value)
                        .map_err(|_| TuningError::BackupInvalid("invalid QoS text".into()))?;
                    if item
                        .as_mut()
                        .expect("property requires item")
                        .insert(name, value)
                        .is_some()
                    {
                        return invalid("QoS CLIXML has duplicate fields");
                    }
                }
                if name.as_slice() == b"MS" {
                    raw_items.push(item.take().expect("MS created item"));
                    if raw_items.len() > 512 {
                        return invalid("QoS CLIXML has more than 512 policies");
                    }
                }
            }
            Ok(Event::Decl(_) | Event::Comment(_)) => {}
            Ok(Event::Eof) => break,
            Ok(_) => return invalid("QoS CLIXML contains unsupported XML content"),
            Err(_) => return invalid("QoS CLIXML could not be parsed"),
        }
    }
    if !stack.is_empty() || item.is_some() || property.is_some() {
        return invalid("QoS CLIXML is incomplete");
    }
    let mut names = HashSet::new();
    raw_items
        .into_iter()
        .map(normalize_qos)
        .map(|result| {
            let spec = result?;
            if !names.insert(spec.name().to_ascii_lowercase()) {
                return invalid("QoS CLIXML has duplicate policy names");
            }
            Ok(spec)
        })
        .collect()
}

fn normalize_qos(mut values: BTreeMap<String, String>) -> Result<QosPolicySpec> {
    let name = values
        .remove("Name")
        .filter(|value| managed_name(value))
        .ok_or_else(|| TuningError::BackupInvalid("QoS policy has an unapproved name".into()))?;
    let dscp = take_number(&mut values, &["Dscp", "DSCPAction", "DSCPValue"])?;
    if dscp > 63 {
        return invalid("QoS DSCP must be in 0..=63");
    }
    let kind = values.remove("Type");
    let port = values
        .remove("Port")
        .or_else(|| values.remove("IPPortMatchCondition"));
    let app = values
        .remove("AppPath")
        .or_else(|| values.remove("AppPathNameMatchCondition"));
    let result = if kind.as_deref() == Some("Port") || (kind.is_none() && port.is_some()) {
        let protocol = values
            .remove("Protocol")
            .or_else(|| values.remove("IPProtocolMatchCondition"))
            .ok_or_else(|| TuningError::BackupInvalid("QoS protocol is missing".into()))?;
        if !matches!(protocol.as_str(), "TCP" | "UDP") {
            return invalid("QoS protocol must be TCP or UDP");
        }
        let port = port
            .ok_or_else(|| TuningError::BackupInvalid("QoS port is missing".into()))?
            .parse::<u16>()
            .map_err(|_| TuningError::BackupInvalid("QoS port is invalid".into()))?;
        if port == 0 {
            return invalid("QoS port must be in 1..=65535");
        }
        QosPolicySpec::Port {
            name,
            protocol,
            port,
            dscp,
        }
    } else if kind.as_deref() == Some("App") || (kind.is_none() && app.is_some()) {
        let app_path =
            app.ok_or_else(|| TuningError::BackupInvalid("QoS app path is missing".into()))?;
        if !valid_windows_exe(&app_path) {
            return invalid("QoS app path must be an absolute local .exe path");
        }
        QosPolicySpec::App {
            name,
            app_path,
            dscp,
        }
    } else {
        return invalid("QoS policy has no supported match condition");
    };
    if !values.is_empty() {
        return invalid("QoS policy has unknown fields");
    }
    Ok(result)
}

fn take_number(values: &mut BTreeMap<String, String>, names: &[&str]) -> Result<u8> {
    let mut found = None;
    for name in names {
        if let Some(value) = values.remove(*name) {
            if found.is_some() {
                return invalid("QoS policy has duplicate DSCP representations");
            }
            found = Some(value);
        }
    }
    found
        .ok_or_else(|| TuningError::BackupInvalid("QoS DSCP is missing".into()))?
        .parse::<u8>()
        .map_err(|_| TuningError::BackupInvalid("invalid QoS DSCP".into()))
}

fn managed_name(name: &str) -> bool {
    [
        "NETWORK_LANTERN_QOS_PORT_",
        "NETWORK_LANTERN_QOS_APP_",
        "NDS_QOS_PORT_",
        "NDS_QOS_APP_",
        "QoS_UDP_TS_",
        "QoS_UDP_CS2_",
        "QoS_APP_",
    ]
    .iter()
    .any(|prefix| name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix))
}

fn valid_windows_exe(path: &str) -> bool {
    path.len() >= 7
        && path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes().get(2) == Some(&b'\\')
        && path.to_ascii_lowercase().ends_with(".exe")
        && !path.starts_with("\\\\")
        && !path.starts_with("\\\\?\\")
        && !path.contains('\0')
}

fn allowed_xml_tag(name: &[u8]) -> bool {
    matches!(
        name,
        b"Objs"
            | b"Obj"
            | b"TN"
            | b"T"
            | b"TNRef"
            | b"LST"
            | b"MS"
            | b"S"
            | b"I32"
            | b"I64"
            | b"U32"
            | b"U64"
            | b"B"
    )
}

fn scalar_tag(name: &[u8]) -> bool {
    matches!(name, b"S" | b"I32" | b"I64" | b"U32" | b"U64" | b"B")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct NicAdvancedRow {
    pub adapter: String,
    pub display_name: String,
    pub registry_keyword: String,
    pub display_value: String,
    pub registry_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct RscRow {
    pub name: String,
    #[serde(deserialize_with = "deserialize_legacy_bool")]
    pub ipv4_enabled: bool,
    #[serde(deserialize_with = "deserialize_legacy_bool")]
    pub ipv6_enabled: bool,
}

pub fn parse_nic_csv(bytes: &[u8]) -> Result<Vec<NicAdvancedRow>> {
    let rows = parse_csv::<NicAdvancedRow>(bytes, 512, "NIC advanced")?;
    if rows.is_empty() {
        return invalid("NIC advanced CSV must contain at least one row");
    }
    for row in &rows {
        let has_keyword = !row.registry_keyword.trim().is_empty();
        let has_display = !row.display_name.trim().is_empty();
        if row.adapter.trim().is_empty()
            || (!has_keyword && !has_display)
            || (has_keyword && row.registry_value.trim().is_empty())
            || (has_display && row.display_value.trim().is_empty())
        {
            return invalid("NIC advanced CSV contains an incomplete row");
        }
    }
    Ok(rows)
}

pub fn parse_rsc_csv(bytes: &[u8]) -> Result<Vec<RscRow>> {
    let rows = parse_csv::<RscRow>(bytes, 64, "RSC")?;
    if rows.is_empty() || rows.iter().any(|row| row.name.trim().is_empty()) {
        return invalid("RSC CSV contains an invalid row");
    }
    Ok(rows)
}

fn parse_csv<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
    maximum: usize,
    label: &str,
) -> Result<Vec<T>> {
    if bytes.len() as u64 > super::MAX_CSV_BYTES {
        return invalid(&format!("{label} CSV exceeds 1 MiB"));
    }
    let mut reader = csv::ReaderBuilder::new().flexible(false).from_reader(bytes);
    let mut rows = Vec::new();
    for value in reader.deserialize() {
        rows.push(value.map_err(|error| {
            TuningError::BackupInvalid(format!("invalid {label} CSV: {error}"))
        })?);
        if rows.len() > maximum {
            return invalid(&format!("{label} CSV exceeds {maximum} rows"));
        }
    }
    Ok(rows)
}

fn deserialize_legacy_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<bool, D::Error> {
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "True" => Ok(true),
        "False" => Ok(false),
        _ => Err(serde::de::Error::custom(
            "boolean must be exactly True or False",
        )),
    }
}

fn invalid<T>(message: &str) -> Result<T> {
    Err(TuningError::BackupInvalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(fields: &str) -> Vec<u8> {
        format!("<Objs><LST><Obj><MS>{fields}</MS></Obj></LST></Objs>").into_bytes()
    }

    #[test]
    fn qos_policy_requires_name_dscp_and_protocol() {
        let cases = [
            r#"<S N="Type">Port</S><S N="Protocol">UDP</S><U32 N="Port">3074</U32><U32 N="Dscp">46</U32>"#,
            r#"<S N="Name">NETWORK_LANTERN_QOS_PORT_3074</S><S N="Type">Port</S><S N="Protocol">UDP</S><U32 N="Port">3074</U32>"#,
            r#"<S N="Name">NETWORK_LANTERN_QOS_PORT_3074</S><S N="Type">Port</S><U32 N="Port">3074</U32><U32 N="Dscp">46</U32>"#,
        ];
        for fields in cases {
            assert!(matches!(
                parse_qos_clixml(&policy(fields)),
                Err(TuningError::BackupInvalid(_))
            ));
        }
    }

    #[test]
    fn qos_policy_accepts_complete_tcp_and_udp_provider_values() {
        for protocol in ["TCP", "UDP"] {
            let fields = format!(
                r#"<S N="Name">NETWORK_LANTERN_QOS_PORT_3074</S><S N="Type">Port</S><S N="Protocol">{protocol}</S><U32 N="Port">3074</U32><U32 N="Dscp">46</U32>"#
            );
            let parsed = parse_qos_clixml(&policy(&fields)).unwrap();
            assert_eq!(parsed.len(), 1);
        }
    }
}
