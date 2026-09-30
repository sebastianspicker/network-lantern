//! Bounded listing of native and legacy runs in a results directory.
use super::normalize::read;
use lantern_contracts::{Error, Result};
use lantern_platform::{PROFILE_LIMIT, REPORT_LIMIT, read_json};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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
}
