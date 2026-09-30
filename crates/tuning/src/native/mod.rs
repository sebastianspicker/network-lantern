use crate::{ComponentStatus, Result, TuningComponent, TuningError, TuningPlan, TuningResult};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
mod windows;

pub(crate) async fn execute(
    plan: &TuningPlan,
    cancellation: &CancellationToken,
    inspect_only: bool,
) -> Result<TuningResult> {
    if cancellation.is_cancelled() {
        return Err(TuningError::Cancelled);
    }
    if plan.config.dry_run {
        let mut result = TuningResult::empty(&plan.config);
        for step in &plan.steps {
            result
                .components
                .insert(component_for_step(step), ComponentStatus::Skipped);
        }
        result
            .warnings
            .push("dry run: native state was not read or changed".into());
        return Ok(result);
    }
    #[cfg(windows)]
    {
        return windows::execute(plan, cancellation, inspect_only).await;
    }
    #[cfg(not(windows))]
    {
        let _ = inspect_only;
        Err(TuningError::Unsupported(
            "Windows tuning requires the elevated Windows helper".into(),
        ))
    }
}

fn component_for_step(step: &crate::TuningStep) -> TuningComponent {
    use crate::TuningStep::*;
    match step {
        ValidateBackupDestination | WriteAndVerifyBackup | ValidateAndStageRestore => {
            TuningComponent::Backup
        }
        EnableLocalQos | RestoreRegistry => TuningComponent::Registry,
        ReconcilePortPolicies { .. } => TuningComponent::QosPortPolicies,
        ReconcileAppPolicies { .. } => TuningComponent::QosAppPolicies,
        DisableNicPowerSaving | RestoreNicAdvanced => TuningComponent::NicAdvanced,
        RestoreRsc => TuningComponent::Rsc,
        SetPowerPlan { .. } | RestorePowerPlan => TuningComponent::PowerPlan,
        RestoreQos => TuningComponent::QosPolicies,
        InspectManagedState => TuningComponent::Manifest,
    }
}

pub(crate) fn validate_backup_acl(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::validate_backup_acl(path);
    #[cfg(not(windows))]
    {
        validate_unix_private_path(path)
    }
}

#[cfg(windows)]
pub(crate) fn validate_backup_parent(path: &Path) -> Result<()> {
    windows::validate_backup_parent(path)
}

pub(crate) fn staging_parent() -> Result<PathBuf> {
    #[cfg(windows)]
    return windows::staging_parent();
    #[cfg(not(windows))]
    {
        Ok(std::env::temp_dir())
    }
}

pub(crate) fn validate_staging_parent(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::validate_staging_parent(path);
    #[cfg(not(windows))]
    {
        if !path.is_dir() {
            return Err(TuningError::BackupInvalid(
                "restore staging parent is not a directory".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn protect_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::protect_directory(path);
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|source| TuningError::io("protect directory", source))
    }
}

pub(crate) fn protect_file(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::protect_file(path);
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|source| TuningError::io("protect file", source))
    }
}

pub(crate) fn validate_staging_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::validate_staging_path(path);
    #[cfg(not(windows))]
    {
        validate_unix_private_path(path)
    }
}

#[cfg(not(windows))]
fn validate_unix_private_path(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|source| TuningError::io("validate protected path", source))?;
    if metadata.file_type().is_symlink() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(TuningError::BackupInvalid(
            "protected path is linked or owned by another user".into(),
        ));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(TuningError::BackupInvalid(
            "protected path grants access to group or other users".into(),
        ));
    }
    Ok(())
}
