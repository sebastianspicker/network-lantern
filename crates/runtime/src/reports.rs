use lantern_contracts::{Error, Provenance, RECORD_VERSION, Result};
use lantern_platform::{PROFILE_LIMIT, REPORT_LIMIT, atomic_json, read_bounded, read_json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::Cursor,
    path::{Path, PathBuf},
};

const CSV_COLUMN_LIMIT: usize = 256;
const CSV_ROW_LIMIT: usize = 256 * 1024;
const CSV_FULL_ROW_LIMIT: usize = 100_000;
const CSV_FULL_CELL_LIMIT: usize = 1_000_000;
const PAGE_RESPONSE_LIMIT: usize = 1024 * 1024;

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
fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    names.iter().find_map(|name| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}
/// Native records are readable from version 1 through the current [`RECORD_VERSION`].
fn supported_record_version(version: &Value) -> bool {
    version
        .as_u64()
        .is_some_and(|version| (1..=u64::from(RECORD_VERSION)).contains(&version))
}
fn unsupported_record_version() -> Error {
    Error::validation(match RECORD_VERSION {
        1 => "Unsupported or invalid report schema_version; only version 1 is supported".into(),
        latest => format!(
            "Unsupported or invalid report schema_version; versions 1 through {latest} are supported"
        ),
    })
}
pub fn normalize(data: Value) -> Result<NormalizedReport> {
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

fn open_csv(path: &Path) -> Result<(csv::Reader<Cursor<Vec<u8>>>, csv::StringRecord)> {
    let bytes = read_bounded(path, REPORT_LIMIT)?;
    let mut input = Cursor::new(bytes);
    if input.get_ref().starts_with(&[0xef, 0xbb, 0xbf]) {
        input.set_position(3);
    }
    let mut reader = csv::ReaderBuilder::new().from_reader(input);
    let headers = reader
        .headers()
        .map_err(|error| Error::validation(format!("Invalid CSV: {error}")))?
        .clone();
    validate_csv_headers(&headers)?;
    Ok((reader, headers))
}

fn validate_csv_headers(headers: &csv::StringRecord) -> Result<()> {
    let mut unique = HashSet::new();
    if headers.is_empty()
        || headers.len() > CSV_COLUMN_LIMIT
        || headers
            .iter()
            .any(|header| header.is_empty() || !unique.insert(header))
    {
        return Err(Error::validation(
            "CSV requires 1–256 unique nonempty column names",
        ));
    }
    if headers.as_slice().len() > CSV_ROW_LIMIT {
        return Err(Error::validation("CSV header exceeds 256 KiB"));
    }
    Ok(())
}

fn csv_record_value(
    headers: &csv::StringRecord,
    record: &csv::StringRecord,
) -> Result<(Value, usize)> {
    if record.as_slice().len() > CSV_ROW_LIMIT {
        return Err(Error::validation("CSV row exceeds 256 KiB"));
    }
    let value = Value::Object(
        headers
            .iter()
            .zip(record.iter())
            .map(|(header, cell)| (header.to_owned(), Value::String(cell.to_owned())))
            .collect(),
    );
    let size = serde_json::to_vec(&value)
        .map_err(|error| Error::validation(error.to_string()))?
        .len();
    if size > CSV_ROW_LIMIT {
        return Err(Error::validation(
            "CSV row exceeds 256 KiB when represented as report data",
        ));
    }
    Ok((value, size))
}

fn check_full_csv_limits(rows: usize, cells: usize) -> Result<()> {
    if rows > CSV_FULL_ROW_LIMIT || cells > CSV_FULL_CELL_LIMIT {
        return Err(Error::validation(
            "CSV exceeds full-read limits of 100000 rows or 1000000 cells; use runs show --offset/--limit",
        ));
    }
    Ok(())
}

fn csv_report(data: Value) -> Result<NormalizedReport> {
    let mut report = normalize(data)?;
    report.source_schema = "legacy-csv".into();
    Ok(report)
}

fn read_csv(path: &Path) -> Result<NormalizedReport> {
    let (mut reader, headers) = open_csv(path)?;
    let mut rows = Vec::new();
    let mut row_count = 0usize;
    let mut cell_count = 0usize;
    let mut representation_bytes = 2usize;
    let mut record = csv::StringRecord::new();
    while reader
        .read_record(&mut record)
        .map_err(|error| Error::validation(format!("Invalid CSV: {error}")))?
    {
        row_count = row_count.saturating_add(1);
        cell_count = cell_count.saturating_add(record.len());
        check_full_csv_limits(row_count, cell_count)?;
        let (value, size) = csv_record_value(&headers, &record)?;
        representation_bytes = representation_bytes
            .checked_add(size)
            .and_then(|bytes| bytes.checked_add(usize::from(row_count > 1)))
            .ok_or_else(|| Error::validation("CSV report representation size overflow"))?;
        if representation_bytes > REPORT_LIMIT {
            return Err(Error::validation(
                "CSV exceeds the 16 MiB full-read representation limit; use runs show --offset/--limit",
            ));
        }
        rows.push(value);
    }
    csv_report(Value::Array(rows))
}

fn read_csv_page(path: &Path, offset: usize, limit: usize) -> Result<Value> {
    let (mut reader, headers) = open_csv(path)?;
    let mut rows = Vec::with_capacity(limit);
    let mut response_bytes = 0usize;
    let mut total = 0usize;
    let mut response_full = false;
    let mut record = csv::StringRecord::new();
    while reader
        .read_record(&mut record)
        .map_err(|error| Error::validation(format!("Invalid CSV: {error}")))?
    {
        if total >= offset && rows.len() < limit && !response_full {
            let (value, size) = csv_record_value(&headers, &record)?;
            if response_bytes.saturating_add(size) > PAGE_RESPONSE_LIMIT {
                response_full = true;
            } else {
                response_bytes += size;
                rows.push(value);
            }
        } else if record.as_slice().len() > CSV_ROW_LIMIT {
            return Err(Error::validation("CSV row exceeds 256 KiB"));
        }
        total = total.saturating_add(1);
    }
    let metadata = csv_report(Value::Array(Vec::new()))?;
    bounded_page(metadata, rows, offset, limit, total)
}

fn bounded_page(
    mut metadata: NormalizedReport,
    rows: Vec<Value>,
    offset: usize,
    limit: usize,
    total: usize,
) -> Result<Value> {
    metadata.data = Value::Null;
    let next = offset.saturating_add(rows.len()).min(total);
    let mut page = json!({
        "metadata": metadata,
        "rows": rows,
        "offset": offset,
        "limit": limit,
        "next_offset": next,
        "total": total,
        "has_more": next < total,
    });
    loop {
        if serde_json::to_vec(&page)
            .map_err(|error| Error::validation(error.to_string()))?
            .len()
            <= PAGE_RESPONSE_LIMIT
        {
            return Ok(page);
        }
        let page_rows = page["rows"].as_array_mut().expect("page rows are an array");
        if page_rows.pop().is_none() {
            return Err(Error::validation(
                "Report page metadata exceeds the 1 MiB response limit",
            ));
        }
        let next = offset.saturating_add(page_rows.len()).min(total);
        page["next_offset"] = Value::from(next);
        page["has_more"] = Value::from(next < total);
    }
}
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

pub fn export(report: &NormalizedReport, destination: &Path) -> Result<()> {
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
pub fn page(data: &Value, offset: usize, limit: usize) -> Result<Vec<Value>> {
    if limit == 0 || limit > 100 {
        return Err(Error::validation("Page size must be 1–100"));
    }
    let rows = data
        .as_array()
        .or_else(|| field(data, &["runs", "results", "measurements"]).and_then(Value::as_array));
    Ok(rows
        .map(|rows| rows.iter().skip(offset).take(limit).cloned().collect())
        .unwrap_or_default())
}
/// A bounded desktop page. Versioned run summaries locate their own sequential artifacts.
pub fn read_page(path: &Path, offset: usize, limit: usize) -> Result<Value> {
    if limit == 0 || limit > 100 {
        return Err(Error::validation("Page size must be 1–100"));
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
    {
        return read_csv_page(path, offset, limit);
    }
    let mut report = read(path)?;
    let original = std::mem::replace(&mut report.data, Value::Null);
    let native = original
        .get("schema_version")
        .is_some_and(supported_record_version)
        && original.get("run_id").is_some_and(Value::is_string)
        && path.file_name().is_some_and(|n| n == "summary.json");
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    let total = if native {
        report.total.unwrap_or(0) as usize
    } else {
        original
            .as_array()
            .or_else(|| {
                field(&original, &["runs", "results", "measurements"]).and_then(Value::as_array)
            })
            .map(Vec::len)
            .unwrap_or(0)
    };
    for index in offset..offset.saturating_add(limit).min(total) {
        let value = if native {
            let artifact = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("measurement-{:08}.json", index + 1));
            match read_json(&artifact, REPORT_LIMIT) {
                Ok(value) => value,
                Err(error) => json!({"error":error,"measurement":index+1}),
            }
        } else {
            page(&original, index, 1)?
                .into_iter()
                .next()
                .unwrap_or(Value::Null)
        };
        let size = serde_json::to_vec(&value)
            .map_err(|e| Error::validation(e.to_string()))?
            .len();
        if size > 256 * 1024 {
            rows.push(json!({"measurement":index+1,"error":"Record exceeds desktop row size; inspect or export with the CLI"}));
            continue;
        }
        if bytes + size > 1024 * 1024 {
            break;
        }
        bytes += size;
        rows.push(value);
    }
    bounded_page(report, rows, offset, limit, total)
}

/// A bounded page of native run summaries and legacy throughput summaries in `directory`.
pub fn list_runs(directory: &Path, offset: usize, limit: usize) -> Result<Value> {
    if limit == 0 || limit > 100 {
        return Err(Error::validation("Page size must be 1–100"));
    }
    if !directory.try_exists()? {
        return Ok(json!({"runs":[],"offset":offset,"limit":limit,"total":0,"has_more":false}));
    }
    lantern_platform::check_path(directory)?;
    let mut paths = Vec::new();
    let mut path_bytes = 0usize;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let summary = entry.path().join("summary.json");
            if summary.is_file() {
                retain_run_path(&mut paths, &mut path_bytes, summary)?;
            }
        } else if entry
            .file_name()
            .to_string_lossy()
            .starts_with("iperf3_summary_")
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            retain_run_path(&mut paths, &mut path_bytes, entry.path())?;
        }
    }
    paths.sort();
    paths.reverse();
    let total = paths.len();
    let mut runs = Vec::new();
    for path in paths.into_iter().skip(offset).take(limit) {
        match read(&path) {
            Ok(mut summary) => {
                summary.data = Value::Null;
                let row = json!({"path":path,"summary":summary});
                if serde_json::to_vec(&row)
                    .map_err(|error| Error::validation(error.to_string()))?
                    .len()
                    > 4096
                {
                    runs.push(json!({
                        "path":path,"error":"Summary metadata exceeds the listing limit; open the report directly"
                    }));
                } else {
                    runs.push(row);
                }
            }
            Err(error) => runs.push(json!({"path":path,"error":error})),
        }
    }
    let legacy = directory.join("iperf3_run_index.json");
    let index = if legacy.is_file() {
        match read_json(&legacy, PROFILE_LIMIT) {
            Ok(value) => {
                if serde_json::to_vec(&value)
                    .map_err(|error| Error::validation(error.to_string()))?
                    .len()
                    > 512 * 1024
                {
                    Some(json!({
                        "path":legacy,"error":"Legacy index exceeds listing limit; use runs show to read it directly"
                    }))
                } else {
                    Some(value)
                }
            }
            Err(error) => Some(json!({"path":legacy,"error":error})),
        }
    } else {
        None
    };
    let page = json!({
        "runs":runs,"legacy_index":index,"offset":offset,"limit":limit,"total":total,
        "has_more":offset.saturating_add(limit)<total
    });
    if serde_json::to_vec(&page)
        .map_err(|error| Error::validation(error.to_string()))?
        .len()
        > 1024 * 1024
    {
        return Err(Error::validation(
            "Run listing exceeds 1 MiB; request fewer runs per page",
        ));
    }
    Ok(page)
}
fn retain_run_path(paths: &mut Vec<PathBuf>, bytes: &mut usize, path: PathBuf) -> Result<()> {
    let next = bytes.saturating_add(path.as_os_str().len());
    if paths.len() >= 100_000 || next > REPORT_LIMIT {
        return Err(Error::validation(
            "Run directory exceeds listing limits of 100000 records or 16 MiB of paths; open a report directly or select a smaller directory",
        ));
    }
    *bytes = next;
    paths.push(path);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_run_listing_does_not_duplicate_csv_sidecars() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        std::fs::write(
            dir.path().join("iperf3_summary_fixture.json"),
            r#"{"SummaryVersion":2,"Status":"Success","Counts":{"Total":1,"Failed":0}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("iperf3_summary_fixture.csv"),
            "No,Status\n1,Success\n",
        )
        .unwrap();
        let listed = list_runs(dir.path(), 0, 20).unwrap();
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["runs"][0]["summary"]["status"], "Success");
    }
    #[test]
    fn unreadable_legacy_index_preserves_valid_run_listing() {
        let directory =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        std::fs::write(
            directory.path().join("iperf3_summary_fixture.json"),
            r#"{"SummaryVersion":2,"Status":"Success","Counts":{"Total":1,"Failed":0}}"#,
        )
        .unwrap();
        let index = directory.path().join("iperf3_run_index.json");
        for contents in [b"broken".to_vec(), vec![b' '; PROFILE_LIMIT + 1]] {
            std::fs::write(&index, contents).unwrap();
            let page = list_runs(directory.path(), 0, 20).unwrap();
            assert_eq!(page["total"], 1);
            assert_eq!(page["runs"][0]["summary"]["status"], "Success");
            assert_eq!(page["legacy_index"]["path"], json!(index));
            assert!(page["legacy_index"]["error"]["message"].is_string());
        }
    }
    #[test]
    fn directory_listing_bounds_retained_paths_before_push() {
        let mut paths = Vec::new();
        let mut bytes = REPORT_LIMIT;
        assert!(retain_run_path(&mut paths, &mut bytes, PathBuf::from("one")).is_err());
        assert!(paths.is_empty());
        assert_eq!(bytes, REPORT_LIMIT);
        bytes = 0;
        for _ in 0..100_000 {
            retain_run_path(&mut paths, &mut bytes, PathBuf::from("x")).unwrap();
        }
        assert!(retain_run_path(&mut paths, &mut bytes, PathBuf::from("x")).is_err());
        assert_eq!(paths.len(), 100_000);
    }
    #[test]
    fn directory_listing_does_not_accumulate_oversized_summary_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        for index in 0..3 {
            std::fs::write(
                directory.join(format!("iperf3_summary_{index}.json")),
                serde_json::to_vec(
                    &json!({"provenance":{"detail":"x".repeat(100_000)},"status":"Success"}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        let page = list_runs(&directory, 0, 20).unwrap();
        assert_eq!(page["total"], 3);
        assert_eq!(page["runs"].as_array().unwrap().len(), 3);
        assert!(
            page["runs"][0]["error"]
                .as_str()
                .unwrap()
                .contains("open the report")
        );
        assert!(serde_json::to_vec(&page).unwrap().len() < 4096);
    }
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
    fn csv_paging_counts_a_large_file_without_full_materialization() {
        use std::fmt::Write;

        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("large.csv");
        let mut contents = String::from("id\n");
        for index in 0..=CSV_FULL_ROW_LIMIT {
            writeln!(&mut contents, "{index}").unwrap();
        }
        std::fs::write(&source, contents).unwrap();

        let page = read_page(&source, CSV_FULL_ROW_LIMIT - 2, 10).unwrap();
        assert_eq!(page["total"], CSV_FULL_ROW_LIMIT + 1);
        assert_eq!(page["rows"].as_array().unwrap().len(), 3);
        assert_eq!(page["rows"][0]["id"], (CSV_FULL_ROW_LIMIT - 2).to_string());
        assert_eq!(page["next_offset"], CSV_FULL_ROW_LIMIT + 1);
        assert_eq!(page["has_more"], false);

        let error = read(&source).unwrap_err();
        assert!(error.message.contains("runs show --offset/--limit"));
    }

    #[test]
    fn full_csv_read_bounds_repeated_long_headers() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("long-header.csv");
        let header = "h".repeat(200 * 1024);
        let mut contents = format!("{header}\n");
        for _ in 0..100 {
            contents.push_str("x\n");
        }
        assert!(contents.len() < 256 * 1024);
        std::fs::write(&source, contents).unwrap();

        let page = read_page(&source, 99, 1).unwrap();
        assert_eq!(page["total"], 100);
        assert_eq!(page["rows"][0][&header], "x");
        let error = read(&source).unwrap_err();
        assert!(error.message.contains("representation limit"));
        assert!(error.message.contains("--offset/--limit"));
    }

    #[test]
    fn full_csv_limits_cover_rows_and_cells() {
        check_full_csv_limits(CSV_FULL_ROW_LIMIT, CSV_FULL_CELL_LIMIT).unwrap();
        assert!(check_full_csv_limits(CSV_FULL_ROW_LIMIT + 1, 1).is_err());
        assert!(check_full_csv_limits(1, CSV_FULL_CELL_LIMIT + 1).is_err());
    }

    #[test]
    fn csv_page_response_is_bounded_without_skipping_the_next_row() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("wide.csv");
        let row = "x".repeat(200_000);
        let mut contents = String::from("value\n");
        for _ in 0..8 {
            contents.push_str(&row);
            contents.push('\n');
        }
        std::fs::write(&source, contents).unwrap();

        let page = read_page(&source, 0, 8).unwrap();
        let retained = page["rows"].as_array().unwrap().len();
        assert!((1..8).contains(&retained));
        assert_eq!(page["next_offset"], retained);
        assert_eq!(page["total"], 8);
        assert_eq!(page["has_more"], true);
        assert!(serde_json::to_vec(&page).unwrap().len() <= PAGE_RESPONSE_LIMIT);
    }

    #[test]
    fn json_page_envelope_is_bounded_without_skipping_rows() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("near-limit.json");
        let rows = (0..6)
            .map(|index| format!("{index}:{}", "x".repeat(209_698)))
            .collect::<Vec<_>>();
        std::fs::write(&source, serde_json::to_vec(&rows).unwrap()).unwrap();

        let first = read_page(&source, 0, 6).unwrap();
        assert_eq!(first["rows"].as_array().unwrap().len(), 4);
        assert_eq!(first["next_offset"], 4);
        assert!(serde_json::to_vec(&first).unwrap().len() <= PAGE_RESPONSE_LIMIT);
        let second = read_page(&source, 4, 6).unwrap();
        assert_eq!(second["rows"].as_array().unwrap().len(), 2);
        assert!(second["rows"][0].as_str().unwrap().starts_with("4:"));
        assert_eq!(second["next_offset"], 6);
    }

    #[test]
    fn json_page_rejects_metadata_larger_than_the_response_limit() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let source = dir.path().join("large-metadata.json");
        std::fs::write(
            &source,
            serde_json::to_vec(&json!({
                "provenance":{"detail":"x".repeat(PAGE_RESPONSE_LIMIT + 1)},
                "results":[{"id":1}]
            }))
            .unwrap(),
        )
        .unwrap();

        let error = read_page(&source, 0, 1).unwrap_err();
        assert!(error.message.contains("metadata exceeds"));
    }

    #[test]
    fn csv_pages_reject_bad_headers_malformed_records_and_oversized_rows() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        for (name, contents) in [
            ("empty.csv", ",id\nvalue,1\n".to_owned()),
            ("duplicate.csv", "id,id\n1,2\n".to_owned()),
            ("malformed.csv", "id,value\n1\n".to_owned()),
            (
                "columns.csv",
                format!(
                    "{}\n{}\n",
                    (0..=CSV_COLUMN_LIMIT)
                        .map(|index| format!("column{index}"))
                        .collect::<Vec<_>>()
                        .join(","),
                    (0..=CSV_COLUMN_LIMIT)
                        .map(|_| "value")
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, contents).unwrap();
            assert!(read_page(&path, 0, 10).is_err(), "{name} should fail");
        }

        let oversized = dir.path().join("oversized.csv");
        std::fs::write(
            &oversized,
            format!("value\n{}\n", "x".repeat(CSV_ROW_LIMIT + 1)),
        )
        .unwrap();
        let error = read_page(&oversized, 0, 10).unwrap_err();
        assert!(error.message.contains("256 KiB"));
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
    #[test]
    fn native_pages_limit_response_and_retain_missing_artifact_errors() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        let path = dir.path().join("summary.json");
        atomic_json(
            &path,
            &json!({"schema_version":1,"run_id":"fixture","counts":{"total":2}}),
            REPORT_LIMIT,
        )
        .unwrap();
        atomic_json(
            &dir.path().join("measurement-00000001.json"),
            &json!({"bytes":42}),
            REPORT_LIMIT,
        )
        .unwrap();
        let page = read_page(&path, 0, 1).unwrap();
        assert_eq!(page["rows"][0]["bytes"], 42);
        assert_eq!(page["has_more"], true);
        assert!(page["metadata"]["data"].is_null());
        let missing = read_page(&path, 1, 1).unwrap();
        assert!(missing["rows"][0]["error"].is_object());
        assert_eq!(missing["has_more"], false);
    }
    #[test]
    fn pagination_bounded() {
        assert_eq!(page(&json!({"runs":[1,2,3]}), 1, 1).unwrap(), [json!(2)]);
        assert!(page(&json!([]), 0, 101).is_err());
    }
}
