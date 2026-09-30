//! Resolve defaults < named profile < configuration < explicit overrides.
use lantern_contracts::{Error, Result};
use serde_json::{Map, Value};

pub fn merge_layers(
    defaults: Value,
    layers: &[Value],
    strict: bool,
) -> Result<(Value, Vec<String>)> {
    let mut merged = defaults
        .as_object()
        .cloned()
        .ok_or_else(|| Error::validation("Defaults must be an object"))?;
    let known = merged.keys().cloned().collect::<Vec<_>>();
    let mut warnings = Vec::new();
    for layer in layers {
        let layer = layer
            .as_object()
            .ok_or_else(|| Error::validation("Configuration must be an object"))?;
        for (key, value) in layer {
            let Some(canonical) = known
                .iter()
                .find(|candidate| candidate.eq_ignore_ascii_case(key))
            else {
                let warning = format!("Unknown configuration key '{key}' ignored");
                if strict {
                    return Err(Error::validation(warning));
                }
                warnings.push(warning);
                continue;
            };
            merged.insert(canonical.clone(), value.clone());
        }
    }
    Ok((Value::Object(merged), warnings))
}
/// Translate stored PowerShell keys without mutating the original record.
pub fn legacy_throughput(input: &Value) -> Result<Value> {
    let map = input
        .as_object()
        .ok_or_else(|| Error::validation("Throughput configuration must be an object"))?;
    let mut out = Map::new();
    for (key, value) in map {
        let lower = key.to_ascii_lowercase();
        let translated = match lower.as_str() {
            "skipreachabilitycheck" => "skip_reachability_check",
            "disablemtuprobe" => "disable_mtu_probe",
            "mtusizes" => "mtu_sizes",
            "thresholdminthroughputmbps" => "min_throughput_mbps",
            "thresholdmaxlosspct" => "max_loss_pct",
            "thresholdmaxjitterms" => "max_jitter_ms",
            "duration" => "duration_secs",
            "omit" => "omit_secs",
            "singletest" => "single_test",
            "udpstart" => "udp_start_bps",
            "udpmax" => "udp_max_bps",
            "udpstep" => "udp_step_bps",
            "udplossthreshold" => "udp_loss_threshold_pct",
            "tcpstreams" => "tcp_streams",
            "tcpwindows" => "tcp_windows_bytes",
            "dscpclasses" => "dscp_classes",
            "ipversion" => "ip_version",
            "retrycount" => "retry",
            "maxtotaltests" => "max_total_tests",
            "connecttimeoutms" => "connect_timeout_ms",
            "target" => "target",
            "port" => "port",
            "protocol" => "protocol",
            _ => key,
        };
        let value = match lower.as_str() {
            "udpstart" | "udpmax" | "udpstep" => Value::from(parse_rate(value)?),
            "tcpwindows" => Value::Array(
                value
                    .as_array()
                    .ok_or_else(|| Error::validation("TcpWindows must be an array"))?
                    .iter()
                    .map(|v| {
                        if v.as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case("default"))
                        {
                            Ok(Value::Null)
                        } else {
                            parse_window(v).map(Value::from)
                        }
                    })
                    .collect::<Result<_>>()?,
            ),
            "retrycount" => serde_json::json!({"max_retries":value,"backoff_ms":2000}),
            "protocol" | "ipversion" => Value::String(
                value
                    .as_str()
                    .ok_or_else(|| Error::validation("Expected a string"))?
                    .to_ascii_lowercase(),
            ),
            _ => value.clone(),
        };
        out.insert(translated.into(), value);
    }
    Ok(Value::Object(out))
}
fn parse_window(value: &Value) -> Result<u64> {
    parse_quantity(value, 1024)
}
fn parse_rate(value: &Value) -> Result<u64> {
    parse_quantity(value, 1000)
}
fn parse_quantity(value: &Value, base: u64) -> Result<u64> {
    if let Some(n) = value.as_u64() {
        return Ok(n);
    }
    let text = value
        .as_str()
        .ok_or_else(|| Error::validation("Expected rate/size string"))?
        .trim();
    let (digits, multiplier) = match text.as_bytes().last().map(u8::to_ascii_lowercase) {
        Some(b'k') => (&text[..text.len() - 1], base),
        Some(b'm') => (&text[..text.len() - 1], base.pow(2)),
        Some(b'g') => (&text[..text.len() - 1], base.pow(3)),
        _ => (text, 1),
    };
    let number: f64 = digits
        .parse()
        .map_err(|_| Error::validation("Invalid rate or size"))?;
    let scaled = number * multiplier as f64;
    if !scaled.is_finite() || scaled < 0.0 || scaled >= u64::MAX as f64 {
        return Err(Error::validation("Rate/size outside supported range"));
    }
    Ok(scaled as u64)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn precedence_zero_repetitions_warnings() {
        let (value, warnings) = merge_layers(
            json!({"budget":5,"streams":[1]}),
            &[
                json!({"budget":3}),
                json!({"budget":2,"unknown":true}),
                json!({"budget":0,"streams":[2,2]}),
            ],
            false,
        )
        .unwrap();
        assert_eq!(value, json!({"budget":0,"streams":[2,2]}));
        assert_eq!(warnings.len(), 1);
        assert!(merge_layers(json!({"budget":5}), &[json!({"unknown":1})], true).is_err());
    }
    #[test]
    fn legacy_units() {
        let value=legacy_throughput(&json!({"UdpStart":"10M","TcpWindows":["default","256K"],"MaxTotalTests":0,"Protocol":"Both"})).unwrap();
        assert_eq!(value["udp_start_bps"], 10_000_000);
        assert_eq!(value["tcp_windows_bytes"], json!([null, 262144]));
        assert_eq!(value["max_total_tests"], 0);
    }
}
