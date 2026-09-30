//! Report comparison and standalone JSON export.
use super::normalize::{NormalizedReport, read, supported_record_version};
use lantern_contracts::{Error, Provenance, RECORD_VERSION, Result};
use lantern_platform::{REPORT_LIMIT, atomic_json, read_json};
use serde_json::{Value, json};
use std::path::Path;

pub fn compare(baseline: &NormalizedReport, current: &NormalizedReport) -> Value {
    json!({
        "baseline_timestamp":baseline.timestamp,"current_timestamp":current.timestamp,
        "baseline_status":baseline.status,"current_status":current.status,
        "status_changed": baseline.status.zip_ref(&current.status).map(|(a,b)|a!=b),
        "failed_delta":baseline.failed.zip(current.failed).map(|(a,b)|i128::from(b)-i128::from(a)),
        "total_delta":baseline.total.zip(current.total).map(|(a,b)|i128::from(b)-i128::from(a)),
        "baseline_elapsed":baseline.elapsed_seconds,"current_elapsed":current.elapsed_seconds,
        "baseline_provenance":baseline.provenance,"current_provenance":current.provenance
    })
}
trait ZipRef<T> {
    fn zip_ref<'a>(&'a self, other: &'a Option<T>) -> Option<(&'a T, &'a T)>;
}
impl<T> ZipRef<T> for Option<T> {
    fn zip_ref<'a>(&'a self, other: &'a Option<T>) -> Option<(&'a T, &'a T)> {
        self.as_ref().zip(other.as_ref())
    }
}

pub(super) fn export(report: &NormalizedReport, destination: &Path) -> Result<()> {
    atomic_json(
        destination,
        &json!({"schema_version":RECORD_VERSION,"provenance":Provenance::default(),"imported":report}),
        REPORT_LIMIT,
    )
}

/// Export a native run together with its persisted measurements, preserving partial failures.
/// The standalone JSON format shares the reader's explicit 16 MiB file limit.
pub fn export_file(source: &Path, destination: &Path) -> Result<()> {
    let mut report = read(source)?;
    if report
        .data
        .get("schema_version")
        .is_some_and(supported_record_version)
        && report.data.get("run_id").is_some_and(Value::is_string)
        && source
            .file_name()
            .is_some_and(|name| name == "summary.json")
    {
        let total = report.total.unwrap_or(0);
        let mut bytes = serde_json::to_vec(&report)
            .map_err(|e| Error::validation(e.to_string()))?
            .len();
        let mut rows = Vec::new();
        for index in 1..=total {
            let path = source
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("measurement-{index:08}.json"));
            let row = match read_json(&path, REPORT_LIMIT) {
                Ok(row) => row,
                Err(error) => json!({"measurement":index,"error":error}),
            };
            bytes = bytes.saturating_add(
                serde_json::to_vec(&row)
                    .map_err(|e| Error::validation(e.to_string()))?
                    .len(),
            );
            if bytes > REPORT_LIMIT {
                return Err(Error::validation(
                    "Run exceeds the standalone JSON export limit of 16 MiB; preserve the run directory for the complete artifacts",
                ));
            }
            rows.push(row);
        }
        report.data["results"] = Value::Array(rows);
    }
    export(&report, destination)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::read_page;
    #[test]
    fn legacy_csv_and_export_preserve_original_values() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("path.csv");
        std::fs::write(
            &source,
            "Host,PingOk,OverallStatus\r\nfixture.invalid,True,Success\r\n",
        )
        .unwrap();
        let report = read(&source).unwrap();
        assert_eq!(report.source_schema, "legacy-csv");
        assert_eq!(report.data[0]["PingOk"], "True");
        assert!(report.total.is_none());
        let destination = dir.path().join("export.json");
        export(&report, &destination).unwrap();
        let imported = read(&destination).unwrap();
        assert_eq!(imported.data, report.data);
        assert_eq!(imported.provenance, report.provenance);
    }
    #[test]
    fn native_export_includes_measurements_and_missing_artifact_evidence() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("summary.json");
        atomic_json(
            &source,
            &json!({"schema_version":1,"run_id":"fixture","counts":{"total":2,"failed":1}}),
            REPORT_LIMIT,
        )
        .unwrap();
        atomic_json(
            &dir.path().join("measurement-00000001.json"),
            &json!({"measurement":{"bytes":42}}),
            REPORT_LIMIT,
        )
        .unwrap();
        let destination = dir.path().join("export.json");
        export_file(&source, &destination).unwrap();
        let page = read_page(&destination, 0, 20).unwrap();
        assert_eq!(page["total"], 2);
        assert_eq!(page["rows"][0]["measurement"]["bytes"], 42);
        assert!(page["rows"][1]["error"].is_object());
    }
}
