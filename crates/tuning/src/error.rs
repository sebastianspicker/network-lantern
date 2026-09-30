use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TuningError {
    #[error("invalid tuning request: {0}")]
    Validation(String),
    #[error("tuning is unavailable on this platform: {0}")]
    Unsupported(String),
    #[error("administrator authorization is required: {0}")]
    Authorization(String),
    #[error("backup is missing: {0}")]
    BackupMissing(PathBuf),
    #[error("backup is incompatible: {0}")]
    BackupIncompatible(String),
    #[error("backup validation failed: {0}")]
    BackupInvalid(String),
    #[error("native operation {operation} failed with code {code}: {message}")]
    Native {
        operation: &'static str,
        code: i64,
        message: String,
    },
    #[error("I/O during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("operation cancelled")]
    Cancelled,
    #[error(
        "restore failed: {source}; recovery backup retained at {recovery_backup}; {rollback_status}"
    )]
    RestoreRecovery {
        #[source]
        source: Box<TuningError>,
        recovery_backup: PathBuf,
        rollback_status: String,
    },
}

impl TuningError {
    pub(crate) fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }

    #[cfg(any(windows, test))]
    pub(crate) fn restore_recovery(
        source: TuningError,
        recovery_backup: PathBuf,
        rollback: Result<()>,
    ) -> Self {
        let rollback_status = match rollback {
            Ok(()) => "rollback completed successfully".into(),
            Err(error) => format!("rollback failed: {error}"),
        };
        Self::RestoreRecovery {
            source: Box::new(source),
            recovery_backup,
            rollback_status,
        }
    }
}

pub type Result<T> = std::result::Result<T, TuningError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_recovery_error_keeps_backup_path_and_rollback_failure() {
        let error = TuningError::restore_recovery(
            TuningError::BackupInvalid("restore artifact changed".into()),
            PathBuf::from(r"C:\ProgramData\NetworkLantern-quarantine-test"),
            Err(TuningError::Native {
                operation: "rollback QoS",
                code: 13,
                message: "provider rejected operation".into(),
            }),
        );
        let message = error.to_string();
        assert!(message.contains("NetworkLantern-quarantine-test"));
        assert!(message.contains("rollback failed"));
        assert!(message.contains("code 13"));
    }

    #[test]
    fn restore_recovery_error_reports_successful_rollback() {
        let error = TuningError::restore_recovery(
            TuningError::Cancelled,
            PathBuf::from(r"C:\ProgramData\NetworkLantern-quarantine-test"),
            Ok(()),
        );
        let message = error.to_string();
        assert!(message.contains("recovery backup retained"));
        assert!(message.contains("rollback completed successfully"));
    }
}
