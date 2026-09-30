//! Legacy CSV reports: bounded full reads and streamed pages.
use super::{
    normalize::{NormalizedReport, normalize},
    paging::{PAGE_RESPONSE_LIMIT, bounded_page},
};
use lantern_contracts::{Error, Result};
use lantern_platform::{REPORT_LIMIT, read_bounded};
use serde_json::Value;
use std::{collections::HashSet, io::Cursor, path::Path};

const CSV_COLUMN_LIMIT: usize = 256;
const CSV_ROW_LIMIT: usize = 256 * 1024;
const CSV_FULL_ROW_LIMIT: usize = 100_000;
const CSV_FULL_CELL_LIMIT: usize = 1_000_000;

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

pub(super) fn read_csv(path: &Path) -> Result<NormalizedReport> {
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

pub(super) fn read_csv_page(path: &Path, offset: usize, limit: usize) -> Result<Value> {
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{read, read_page};
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
}
