use crate::{ComponentStatus, Result, TuningComponent, TuningError, TuningPlan, TuningResult};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
mod windows;

pub(crate) async fn execute(
    plan: &TuningPlan,
    cancellation: &CancellationToken,
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
        return windows::execute(plan, cancellation).await;
    }
    #[cfg(not(windows))]
    {
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

/// QoS policy prefixes this tool owns; Apply, Backup and Verify touch only these.
const OWNED_QOS_PREFIXES: &[&str] = &[
    "NETWORK_LANTERN_QOS_PORT_",
    "NETWORK_LANTERN_QOS_APP_",
    "NDS_QOS_PORT_",
    "NDS_QOS_APP_",
];
/// Generic legacy prefixes that unrelated policies may share.
const LEGACY_GENERIC_QOS_PREFIXES: &[&str] = &["QoS_UDP_TS_", "QoS_UDP_CS2_", "QoS_APP_"];

/// Whether a QoS policy is ours to read and delete. `restore_names` is the
/// desired set of the backup being restored (empty outside Restore): a
/// legacy-generic name counts only when that exact name is in it.
pub(crate) fn managed_qos_name(name: &str, restore_names: &[String]) -> bool {
    // `get` instead of slicing: a non-ASCII policy name must not panic the helper.
    let has_prefix = |prefixes: &[&str]| {
        prefixes.iter().any(|prefix| {
            name.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        })
    };
    has_prefix(OWNED_QOS_PREFIXES)
        || (has_prefix(LEGACY_GENERIC_QOS_PREFIXES)
            && restore_names
                .iter()
                .any(|restore| restore.eq_ignore_ascii_case(name)))
}

/// Apply keeps the oldest verified backup: `Ok(None)` means none exists and one
/// must be published, `Ok(Some(_))` keeps the verified bundle, and a present but
/// unverifiable folder refuses Apply rather than overwriting it.
#[cfg(any(windows, test))]
pub(crate) fn apply_backup_decision<T>(
    folder: &Path,
    present: bool,
    validate: impl FnOnce() -> Result<T>,
) -> Result<Option<T>> {
    if !present {
        return Ok(None);
    }
    validate().map(Some).map_err(|error| {
        TuningError::BackupInvalid(format!(
            "existing backup at {} could not be verified ({error}); move it aside before applying",
            folder.display()
        ))
    })
}

pub(crate) fn validate_backup_acl(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::validate_protected(path);
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
    return windows::validate_staging_chain(path);
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
    return windows::protect(path);
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|source| TuningError::io("protect directory", source))
    }
}

pub(crate) fn protect_file(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::protect(path);
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|source| TuningError::io("protect file", source))
    }
}

pub(crate) fn validate_staging_path(path: &Path) -> Result<()> {
    #[cfg(windows)]
    return windows::validate_staging_chain(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qos_classification_owns_only_project_prefixes_outside_restore() {
        for owned in ["NETWORK_LANTERN_QOS_PORT_3074", "nds_qos_app_0"] {
            assert!(managed_qos_name(owned, &[]));
        }
        // "QoS_APPé…" puts a char boundary inside the 8-byte "QoS_APP_" prefix.
        for foreign in [
            "QoS_APP_Teams",
            "QoS_UDP_TS_9987",
            "Corporate VoIP",
            "QoS_APPé_x",
        ] {
            assert!(!managed_qos_name(foreign, &[]));
        }
        let restore = vec!["QoS_APP_0".to_owned()];
        assert!(managed_qos_name("QoS_APP_0", &restore));
        // WMI policy names are case-insensitive, so the backup's spelling may differ.
        assert!(managed_qos_name("qos_app_0", &restore));
        assert!(!managed_qos_name("QoS_APP_Teams", &restore));
        assert!(!managed_qos_name(
            "Corporate VoIP",
            &["Corporate VoIP".into()]
        ));
    }

    #[test]
    fn apply_keeps_a_verified_backup_and_refuses_an_unverifiable_one() {
        let folder = Path::new("/backup/NetworkLantern");
        let absent = apply_backup_decision(folder, false, || -> Result<()> {
            panic!("an absent folder is not validated")
        });
        assert!(matches!(absent, Ok(None)));
        assert!(matches!(
            apply_backup_decision(folder, true, || Ok("bundle")),
            Ok(Some("bundle"))
        ));
        let refused = apply_backup_decision(folder, true, || -> Result<()> {
            Err(TuningError::BackupInvalid("digest mismatch".into()))
        });
        let message = refused.unwrap_err().to_string();
        assert!(message.contains("/backup/NetworkLantern") && message.contains("digest mismatch"));
    }
}
