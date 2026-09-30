use std::io;

/// Failures are explicit so callers can map validation, cancellation, and
/// transient transport errors to stable process status.
#[derive(Debug, thiserror::Error)]
pub enum ThroughputError {
    #[error("invalid throughput configuration: {0}")]
    Validation(String),
    #[error("planned test count {planned} exceeds MaxTotalTests {maximum}")]
    BudgetExceeded { planned: u64, maximum: u64 },
    #[error("throughput run was cancelled")]
    Cancelled,
    #[error("{phase} timed out after {timeout_ms} ms")]
    Timeout {
        phase: &'static str,
        timeout_ms: u64,
    },
    #[error("iperf3 server is busy")]
    ServerBusy,
    #[error("iperf3 server error {iperf_code} (system error {system_code})")]
    Server { iperf_code: i32, system_code: i32 },
    #[error("unexpected iperf3 protocol state {actual}; expected {expected}")]
    UnexpectedState { actual: i8, expected: &'static str },
    #[error("iperf3 control JSON length {actual} exceeds configured limit {maximum}")]
    ControlMessageTooLarge { actual: usize, maximum: usize },
    #[error("invalid iperf3 control JSON: {0}")]
    InvalidControlJson(#[from] serde_json::Error),
    #[error("network I/O failed during {phase}: {source}")]
    Io {
        phase: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("malformed iperf3 result: {0}")]
    MalformedResult(String),
    #[error("all {attempts} attempt(s) failed: {last}")]
    RetriesExhausted { attempts: u8, last: Box<Self> },
}

impl ThroughputError {
    pub(crate) fn io(phase: &'static str, source: io::Error) -> Self {
        Self::Io { phase, source }
    }

    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Io { .. } | Self::Timeout { .. } | Self::ServerBusy
        )
    }
}

pub type Result<T> = std::result::Result<T, ThroughputError>;
