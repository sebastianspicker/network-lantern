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
impl ErrorCategory {
    pub fn throughput_exit_code(self) -> u8 {
        match self {
            Self::Validation => 11,
            Self::Prerequisite | Self::Permission => 12,
            Self::Connectivity => 13,
            Self::PartialFailure => 14,
            Self::TotalFailure => 15,
            Self::Internal | Self::Busy => 16,
            Self::Cancelled => 130,
        }
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
}
