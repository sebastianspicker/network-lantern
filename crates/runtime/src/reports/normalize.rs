//! Reads native and legacy reports into one normalized shape.
use super::csv_file::read_csv;
use lantern_contracts::{Error, Result};
use lantern_platform::{REPORT_LIMIT, read_bounded, read_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{ops::RangeInclusive, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedReport {
    pub provenance: Value,
    pub timestamp: Option<String>,
    pub status: Option<String>,
    pub total: Option<u64>,
    pub failed: Option<u64>,
    pub elapsed_seconds: Option<f64>,
    pub source_schema: String,
    pub data: Value,
}
pub(super) fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    names.iter().find_map(|name| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}
/// Native record versions these readers understand. Bumping
/// [`lantern_contracts::RECORD_VERSION`] requires extending the readers and this range first,
/// so a new writer never outruns its readers.
const SUPPORTED_RECORD_VERSIONS: RangeInclusive<u64> = 1..=1;
pub(super) fn supported_record_version(version: &Value) -> bool {
    version
        .as_u64()
        .is_some_and(|version| SUPPORTED_RECORD_VERSIONS.contains(&version))
}
fn unsupported_record_version() -> Error {
    let (first, last) = (
        SUPPORTED_RECORD_VERSIONS.start(),
        SUPPORTED_RECORD_VERSIONS.end(),
    );
    Error::validation(if first == last {
        format!("Unsupported or invalid report schema_version; only version {first} is supported")
    } else {
        format!(
            "Unsupported or invalid report schema_version; versions {first} through {last} are supported"
        )
    })
}
pub(super) fn normalize(data: Value) -> Result<NormalizedReport> {
    let schema_version = field(&data, &["schema_version"]);
    if let Some(version) = schema_version {
        if !supported_record_version(version) {
            return Err(unsupported_record_version());
        }
        if let Some(imported) = field(&data, &["imported"]) {
            return serde_json::from_value(imported.clone())
                .map_err(|e| Error::validation(format!("Invalid exported report: {e}")));
        }
    }
    if !data.is_object() && !data.is_array() {
        return Err(Error::validation("Report must be an object or array"));
    }
    let rust = schema_version.is_some();
    let counts = field(&data, &["counts"]);
    let total = counts
        .and_then(|v| field(v, &["total"]))
        .and_then(Value::as_u64);
    let failed = counts
        .and_then(|v| field(v, &["failed"]))
        .and_then(Value::as_u64);
    let provenance = field(&data, &["provenance"])
        .cloned()
        .unwrap_or_else(|| json!({"engine":"legacy","version":null,"os":null}));
    Ok(NormalizedReport {
        provenance,
        timestamp: field(&data, &["timestamp", "started_utc", "startedUtc"])
            .and_then(Value::as_str)
            .map(str::to_owned),
        status: field(&data, &["status"])
            .and_then(Value::as_str)
            .map(str::to_owned),
        elapsed_seconds: field(&data, &["elapsed_seconds", "elapsedSeconds"])
            .and_then(Value::as_f64),
        total,
        failed,
        source_schema: if rust {
            "rust-v1"
        } else if field(&data, &["SummaryVersion"]).is_some() {
            "legacy-throughput-summary"
        } else if field(&data, &["runs"]).is_some() {
            "legacy-run-index"
        } else {
            "legacy-path-or-measurements"
        }
        .into(),
        data,
    })
}
pub fn read(path: &Path) -> Result<NormalizedReport> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("csv") => read_csv(path),
        Some("txt") | Some("log") => {
            let bytes = read_bounded(path, REPORT_LIMIT)?;
            let text = String::from_utf8(bytes)
                .map_err(|e| Error::validation(format!("Invalid UTF-8 report: {e}")))?;
            let mut report = normalize(json!({"results":[{"text":text}]}))?;
            report.source_schema = "legacy-diagnostic-text".into();
            Ok(report)
        }
        _ => normalize(read_json(path, REPORT_LIMIT)?),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{compare, read_page};
    use lantern_platform::atomic_json;
    #[test]
    fn unavailable_metrics_stay_absent() {
        let report=normalize(json!({"SummaryVersion":2,"Timestamp":"fixture","Status":"Success","Counts":{"Total":3,"Failed":0}})).unwrap();
        assert_eq!(report.total, Some(3));
        assert_eq!(report.elapsed_seconds, None);
        assert_eq!(report.provenance["engine"], "legacy");
        let path = normalize(json!([{"Host":"fixture"}])).unwrap();
        assert!(compare(&report, &path)["total_delta"].is_null());
    }
    #[test]
    fn readers_support_the_current_record_version() {
        assert!(SUPPORTED_RECORD_VERSIONS.contains(&u64::from(lantern_contracts::RECORD_VERSION)));
    }
    #[test]
    fn unknown_or_malformed_native_schema_versions_are_rejected() {
        for value in [json!({"schema_version":2}), json!({"schema_version":"1"})] {
            let error = normalize(value).unwrap_err();
            assert!(error.message.contains("only version 1"));
        }
        let legacy = normalize(json!({"SummaryVersion":2,"Status":"Success"})).unwrap();
        assert_eq!(legacy.source_schema, "legacy-throughput-summary");
    }
    #[test]
    fn native_record_versions_outside_the_supported_range_are_rejected_explicitly() {
        let supported =
            normalize(json!({"schema_version":1,"run_id":"fixture","status":"Success"})).unwrap();
        assert_eq!(supported.source_schema, "rust-v1");
        assert_eq!(supported.status.as_deref(), Some("Success"));
        for version in [0, 2] {
            let error =
                normalize(json!({"schema_version":version,"run_id":"fixture"})).unwrap_err();
            assert_eq!(error.category, lantern_contracts::ErrorCategory::Validation);
            assert_eq!(
                error.message,
                "Unsupported or invalid report schema_version; only version 1 is supported"
            );
        }
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        for version in [0, 1, 2] {
            let path = dir.path().join("summary.json");
            atomic_json(
                &path,
                &json!({"schema_version":version,"run_id":"fixture","counts":{"total":0}}),
                REPORT_LIMIT,
            )
            .unwrap();
            assert_eq!(read(&path).is_ok(), version == 1, "read {version}");
            assert_eq!(
                read_page(&path, 0, 1).is_ok(),
                version == 1,
                "page {version}"
            );
        }
    }
}
