//! Native Windows tuning, recovery validation, and pure planning.

mod backup;
mod error;
mod native;
mod plan;
mod types;
#[cfg(any(windows, test))]
mod verification;

pub use backup::{
    ArtifactKind, BackupBundle, BackupManifest, BackupManifestComponents, BundleStatus,
    StagedBackup, parse_manifest_bounded, validate_backup_bundle,
};
pub use error::{Result, TuningError};
pub use plan::plan;
pub use types::{
    ComponentStatus, PowerPlan, TuningAction, TuningComponent, TuningConfig, TuningPlan,
    TuningProfile, TuningResult, TuningStep,
};

use tokio_util::sync::CancellationToken;

/// Executes an already validated plan through native platform APIs.
#[derive(Debug, Default, Clone, Copy)]
pub struct TuningExecutor;

impl TuningExecutor {
    pub const fn new() -> Self {
        Self
    }

    pub async fn inspect(&self, plan: &TuningPlan) -> Result<TuningResult> {
        native::execute(plan, &CancellationToken::new(), true).await
    }

    pub async fn execute(
        &self,
        plan: &TuningPlan,
        cancellation: &CancellationToken,
    ) -> Result<TuningResult> {
        native::execute(plan, cancellation, false).await
    }
}
