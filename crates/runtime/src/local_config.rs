//! Read legacy host defaults once, before creating a reviewable pure request.
use lantern_contracts::{Error, Result};
use serde_json::{Value, json};
use std::path::Path;

pub fn prepare_local_request(request: &Value) -> Result<Value> {
    with_hosts_file(request, Path::new("config/hosts.conf"))
}
fn with_hosts_file(request: &Value, path: &Path) -> Result<Value> {
    let capability = request.get("capability").and_then(Value::as_str);
    if !matches!(capability, Some("path_basic" | "path_trace" | "workflow"))
        || !path
            .try_exists()
            .map_err(|e| Error::validation(e.to_string()))?
    {
        return Ok(request.clone());
    }
    let bytes = lantern_platform::read_bounded(path, 64 * 1024)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::validation("hosts.conf must contain UTF-8 text"))?;
    let mut hosts4 = Vec::new();
    let mut hosts6 = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "ipv4" => hosts4.push(value),
            "ipv6" => hosts6.push(value),
            _ => {}
        }
    }
    let mut hosts = serde_json::Map::new();
    if !hosts4.is_empty() {
        hosts.insert("hosts_ipv4".into(), json!(hosts4));
    }
    if !hosts6.is_empty() {
        hosts.insert("hosts_ipv6".into(), json!(hosts6));
    }
    let layer = if capability == Some("workflow") {
        json!({"path":hosts})
    } else {
        Value::Object(hosts)
    };
    let mut prepared = request.clone();
    prepared
        .get_mut("layers")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| Error::validation("Request layers must be an array"))?
        .insert(0, layer);
    Ok(prepared)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_defaults_are_snapshotted_and_explicit_hosts_win() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("hosts.conf");
        std::fs::write(&path, "# comment\n IPV4 = first.invalid\nipv4=first.invalid\ninvalid\nipv6=third.invalid\nunknown=ignored\n").unwrap();
        let request = json!({"capability":"path_basic","layers":[{"hosts_ipv6":["explicit.invalid"],"rounds":["Standard"]}]});
        let prepared = with_hosts_file(&request, &path).unwrap();
        std::fs::write(&path, "ipv4=changed.invalid").unwrap();
        let plan = crate::plan_request(&prepared).unwrap();
        assert_eq!(
            plan["plan"]["ipv4_hosts"],
            json!(["first.invalid", "first.invalid"])
        );
        assert_eq!(plan["plan"]["ipv6_hosts"], json!(["explicit.invalid"]));
        assert_eq!(plan["plan"]["total_items"], 3);
    }
    #[test]
    fn absent_or_empty_legacy_config_preserves_fallbacks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("hosts.conf");
        let request = json!({"capability":"path_trace","layers":[]});
        assert_eq!(with_hosts_file(&request, &path).unwrap(), request);
        std::fs::write(&path, "# empty\nunknown=ignored").unwrap();
        assert_eq!(
            crate::plan_request(&with_hosts_file(&request, &path).unwrap()).unwrap(),
            crate::plan_request(&request).unwrap()
        );
    }
}
