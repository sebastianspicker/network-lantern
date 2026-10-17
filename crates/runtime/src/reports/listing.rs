//! Bounded listing of native and legacy runs in a results directory.
use super::normalize::{field, read};
use lantern_contracts::{Error, Result};
use lantern_platform::{PROFILE_LIMIT, REPORT_LIMIT, read_json};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Summaries larger than this are ordered by mtime instead of being parsed just to sort.
const SORT_KEY_LIMIT: usize = 256 * 1024;
const INCOMPLETE_RUN: &str = "Run has measurement records but no summary.json (interrupted?)";

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
            } else if has_measurements(&entry.path()) {
                retain_run_path(&mut paths, &mut path_bytes, entry.path())?;
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
    let mut keyed = paths
        .into_iter()
        .map(|path| (start_millis(&path), path))
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let paths = keyed.into_iter().map(|(_, path)| path);
    let total = paths.len();
    let mut runs = Vec::new();
    for path in paths.skip(offset).take(limit) {
        if path.is_dir() {
            runs.push(json!({"path":path,"status":"Incomplete","error":INCOMPLETE_RUN}));
            continue;
        }
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
/// Whether a run directory without `summary.json` still holds measurement records.
fn has_measurements(directory: &Path) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("measurement-") && name.ends_with(".json")
        })
    })
}
/// Run start in epoch milliseconds: the report's own timestamp, else the file's mtime.
fn start_millis(path: &Path) -> i64 {
    if path.is_file()
        && let Ok(report) = read_json(path, SORT_KEY_LIMIT)
        && let Some(text) =
            field(&report, &["timestamp", "started_utc", "startedUtc"]).and_then(Value::as_str)
        && let Ok(time) = chrono::DateTime::parse_from_rfc3339(text)
    {
        return time.timestamp_millis();
    }
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |age| i64::try_from(age.as_millis()).unwrap_or(i64::MAX))
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
    fn runs_are_listed_newest_first_with_incomplete_runs_included() {
        let dir =
            tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
        // Directory names sort opposite to start time, so path order cannot pass by luck.
        for (name, timestamp) in [
            ("c-oldest", "2026-01-01T00:00:00.000Z"),
            ("a-newest", "2026-03-01T00:00:00.000Z"),
            ("b-middle", "2026-02-01T00:00:00.000Z"),
        ] {
            let run = dir.path().join(name);
            std::fs::create_dir(&run).unwrap();
            std::fs::write(
                run.join("summary.json"),
                serde_json::to_vec(&json!({
                    "schema_version":1,"status":"Success","timestamp":timestamp,
                    "counts":{"total":1,"failed":0}
                }))
                .unwrap(),
            )
            .unwrap();
        }
        // A crashed run has no timestamp, so its fresh mtime places it first.
        let crashed = dir.path().join("d-crashed");
        std::fs::create_dir(&crashed).unwrap();
        std::fs::write(crashed.join("measurement-00000001.json"), "{}").unwrap();
        std::fs::create_dir(dir.path().join("e-empty")).unwrap();
        let listed = list_runs(dir.path(), 0, 20).unwrap();
        assert_eq!(listed["total"], 4);
        let names = listed["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|run| {
                let path = Path::new(run["path"].as_str().unwrap());
                let dir = if path.ends_with("summary.json") {
                    path.parent().unwrap()
                } else {
                    path
                };
                dir.file_name().unwrap().to_string_lossy().into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["d-crashed", "a-newest", "b-middle", "c-oldest"]);
        assert_eq!(listed["runs"][0]["status"], "Incomplete");
        assert_eq!(listed["runs"][0]["error"], INCOMPLETE_RUN);
        let page = list_runs(dir.path(), 1, 2).unwrap();
        assert_eq!(page["runs"].as_array().unwrap().len(), 2);
        assert_eq!(page["has_more"], true);
    }
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
