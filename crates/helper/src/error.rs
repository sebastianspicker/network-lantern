use std::io;

#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    #[error("invalid helper request: {0}")]
    Validation(String),
    #[error("helper authorization denied: {0}")]
    Authorization(String),
    #[error("helper is unavailable: {0}")]
    Unavailable(String),
    #[error("helper request authentication failed")]
    Authentication,
    #[error("helper request nonce was already used")]
    Replay,
    #[error("helper request is outside the reviewed plan: {0}")]
    Scope(String),
    #[error("helper protocol error: {0}")]
    Protocol(String),
    #[error("helper transport during {operation}: {source}")]
    Transport {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("helper operation cancelled")]
    Cancelled,
    #[error("Windows tuning failed: {0}")]
    Tuning(String),
    #[error("diagnostic probe failed: {0}")]
    Probe(String),
    #[error(transparent)]
    Diagnostic(#[from] lantern_contracts::Error),
}

impl From<lantern_tuning::TuningError> for HelperError {
    fn from(error: lantern_tuning::TuningError) -> Self {
        match error {
            lantern_tuning::TuningError::Cancelled => Self::Cancelled,
            lantern_tuning::TuningError::Authorization(message) => Self::Authorization(message),
            lantern_tuning::TuningError::Validation(message) => Self::Validation(message),
            other => Self::Tuning(other.to_string()),
        }
    }
}

impl HelperError {
    pub(crate) fn transport(operation: &'static str, source: io::Error) -> Self {
        Self::Transport { operation, source }
    }
}

pub type Result<T> = std::result::Result<T, HelperError>;
