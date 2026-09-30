//! Bounded report pages for the desktop and `runs show --offset/--limit`.
use super::{
    csv_file::read_csv_page,
    normalize::{NormalizedReport, field, read, supported_record_version},
};
use lantern_contracts::{Error, Result};
use lantern_platform::{REPORT_LIMIT, read_json};
use serde_json::{Value, json};
use std::path::Path;

pub(super) const PAGE_RESPONSE_LIMIT: usize = 1024 * 1024;

pub(super) fn bounded_page(
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
pub(crate) fn page(data: &Value, offset: usize, limit: usize) -> Result<Vec<Value>> {
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
#[cfg(test)]
mod tests {
    use super::*;
    use lantern_platform::atomic_json;
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
