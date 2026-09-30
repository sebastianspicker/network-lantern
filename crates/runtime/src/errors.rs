//! Maps engine, helper and tuning failures to transport error categories.
use lantern_contracts::{Error, ErrorCategory};
use lantern_throughput::ThroughputError;

pub(crate) fn helper_error(error: lantern_helper::HelperError) -> Error {
    use lantern_helper::HelperError as H;
    let category = match &error {
        H::Validation(_) => ErrorCategory::Validation,
        H::Authorization(_) | H::Authentication | H::Replay | H::Scope(_) => {
            ErrorCategory::Permission
        }
        H::Cancelled => ErrorCategory::Cancelled,
        H::Diagnostic(error) => error.category,
        H::Probe(_) => ErrorCategory::Connectivity,
        _ => ErrorCategory::Prerequisite,
    };
    Error::new(category, error.to_string())
}
pub(crate) fn tuning_error(error: lantern_tuning::TuningError) -> Error {
    use lantern_tuning::TuningError as T;
    let category = match &error {
        T::Validation(_) => ErrorCategory::Validation,
        T::Authorization(_) => ErrorCategory::Permission,
        T::Cancelled => ErrorCategory::Cancelled,
        _ => ErrorCategory::Prerequisite,
    };
    Error::new(category, error.to_string())
}
pub(crate) fn throughput_error(e: ThroughputError) -> Error {
    let category = match &e {
        ThroughputError::Validation(_) | ThroughputError::BudgetExceeded { .. } => {
            ErrorCategory::Validation
        }
        ThroughputError::Cancelled => ErrorCategory::Cancelled,
        ThroughputError::Io { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            ErrorCategory::Permission
        }
        _ => ErrorCategory::Connectivity,
    };
    Error::new(category, e.to_string())
}
pub(crate) fn probe_error(error: lantern_path_io::ProbeError) -> Error {
    use lantern_path_io::ProbeError as P;
    let category = match &error {
        P::Cancelled => ErrorCategory::Cancelled,
        P::Permission { .. } => ErrorCategory::Permission,
        P::Unsupported { .. } => ErrorCategory::Prerequisite,
        P::InvalidHost(_) | P::InvalidPlan(_) => ErrorCategory::Validation,
        _ => ErrorCategory::Connectivity,
    };
    Error::new(category, error.to_string())
}
pub(crate) fn internal(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorCategory::Internal, error.to_string())
}
