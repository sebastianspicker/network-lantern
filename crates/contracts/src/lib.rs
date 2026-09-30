//! Shared transport contracts. Each measurement engine owns its own plan and metrics.
use serde::{Deserialize, Serialize};

pub const RECORD_VERSION: u32 = 1;
pub const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Validation,
    Prerequisite,
    Connectivity,
    PartialFailure,
    TotalFailure,
    Internal,
    Permission,
    Cancelled,
    Busy,
}
/// Process status policy shared by every adapter. Throughput keeps its legacy status table;
/// other capabilities collapse failures to [`exit::FAILURE`].
pub mod exit {
    use super::ErrorCategory;

    pub const SUCCESS: u8 = 0;
    pub const FAILURE: u8 = 1;
    pub const INVALID_INPUT: u8 = 11;
    pub const PREREQUISITE: u8 = 12;
    pub const CONNECTIVITY: u8 = 13;
    pub const PARTIAL_FAILURE: u8 = 14;
    pub const TOTAL_FAILURE: u8 = 15;
    pub const INTERNAL: u8 = 16;
    /// SIGINT or an operator cancellation.
    pub const CANCELLED: u8 = 130;
    /// SIGTERM on Unix.
    pub const TERMINATED: u8 = 143;

    /// The throughput status for a failure category.
    pub fn throughput(category: ErrorCategory) -> u8 {
        match category {
            ErrorCategory::Validation => INVALID_INPUT,
            ErrorCategory::Prerequisite | ErrorCategory::Permission => PREREQUISITE,
            ErrorCategory::Connectivity => CONNECTIVITY,
            ErrorCategory::PartialFailure => PARTIAL_FAILURE,
            ErrorCategory::TotalFailure => TOTAL_FAILURE,
            ErrorCategory::Internal | ErrorCategory::Busy => INTERNAL,
            ErrorCategory::Cancelled => CANCELLED,
        }
    }
    pub fn is_interrupted(code: u8) -> bool {
        matches!(code, CANCELLED | TERMINATED)
    }
    /// Maps a throughput-table status to the process status of the executed capability.
    pub fn for_capability(throughput: bool, code: u8) -> u8 {
        if throughput || code == SUCCESS || is_interrupted(code) {
            code
        } else {
            FAILURE
        }
    }
    /// The process status for an error returned before or instead of a run summary.
    pub fn for_error(throughput: bool, category: ErrorCategory) -> u8 {
        for_capability(throughput, self::throughput(category))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub category: ErrorCategory,
    pub message: String,
}
impl Error {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: bounded_text(&message.into(), MAX_DIAGNOSTIC_BYTES),
        }
    }
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Validation, message)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCategory::Prerequisite, e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn bounded_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub engine: String,
    pub version: String,
    pub os: String,
}
impl Default for Provenance {
    fn default() -> Self {
        Self {
            engine: "network-lantern-rust".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
        }
    }
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn new_run_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_utf8_bounded() {
        let s = "é".repeat(9000);
        assert_eq!(bounded_text(&s, 16383).len(), 16382);
    }
    #[test]
    fn process_status_policy() {
        assert_eq!(exit::for_error(true, ErrorCategory::Validation), 11);
        assert_eq!(exit::for_error(false, ErrorCategory::Validation), 1);
        assert_eq!(exit::for_error(true, ErrorCategory::Busy), 16);
        assert_eq!(exit::for_error(false, ErrorCategory::Cancelled), 130);
        assert_eq!(exit::for_capability(false, 14), 1);
        assert_eq!(exit::for_capability(false, 143), 143);
        assert_eq!(exit::for_capability(true, 15), 15);
    }
}
