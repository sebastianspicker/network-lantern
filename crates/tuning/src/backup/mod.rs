mod artifacts;
mod manifest;
#[cfg(any(windows, test))]
pub(crate) mod persist;
#[cfg(any(windows, test))]
pub(crate) mod provider;
#[cfg(any(windows, test))]
pub(crate) mod publication;
mod staging;
pub(crate) mod trusted_io;

#[cfg(windows)]
pub(crate) use artifacts::{NicAdvancedRow, QosPolicySpec, RegistryValue, RscRow};
pub(crate) use artifacts::{parse_nic_csv, parse_qos_clixml, parse_registry_backup, parse_rsc_csv};
pub use manifest::{
    ArtifactKind, BackupManifest, BackupManifestComponents, parse_manifest_bounded,
};
pub use staging::StagedBackup;

use crate::{Result, TuningError};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAX_REGISTRY_BYTES: u64 = 1024 * 1024;
pub const MAX_CSV_BYTES: u64 = 1024 * 1024;
pub const MAX_QOS_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleStatus {
    Ok,
    Missing,
    Invalid,
    Incompatible,
}

#[derive(Debug)]
pub struct BackupBundle {
    folder: PathBuf,
    manifest: BackupManifest,
    artifacts: BTreeMap<ArtifactKind, Vec<u8>>,
}

impl BackupBundle {
    pub fn folder(&self) -> &Path {
        &self.folder
    }
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }
    pub fn artifact(&self, kind: ArtifactKind) -> Option<&[u8]> {
        self.artifacts.get(&kind).map(Vec::as_slice)
    }
}

pub fn validate_backup_bundle(folder: &Path) -> Result<BackupBundle> {
    validate_folder(folder)?;
    let directory = trusted_io::TrustedDirectory::open(folder, "open backup directory")?;
    let manifest_path = folder.join("backup_manifest.json");
    crate::native::validate_backup_acl(&manifest_path)?;
    let bytes = trusted_io::read_bounded(
        &manifest_path,
        MAX_MANIFEST_BYTES,
        "read backup manifest",
        Some(&directory),
    )?;
    let manifest = parse_manifest_bounded(&bytes)?;
    if manifest.schema_version > 3 {
        return Err(TuningError::BackupIncompatible(format!(
            "schema {} is newer than supported schema 3",
            manifest.schema_version
        )));
    }
    if !matches!(
        manifest.tool_name.as_str(),
        "network-lantern" | "network-diagnostics-suite"
    ) {
        return Err(TuningError::BackupIncompatible(
            "ToolName is not compatible with Network Lantern".into(),
        ));
    }

    let expected = manifest.expected_artifacts();
    if manifest.artifact_digests.len() != expected.len() {
        return Err(TuningError::BackupInvalid(
            "ArtifactDigests must match enabled Components exactly".into(),
        ));
    }
    let mut artifacts = BTreeMap::new();
    for kind in expected {
        let name = kind.file_name();
        let expected_hash = manifest
            .artifact_digests
            .get(name)
            .ok_or_else(|| TuningError::BackupInvalid(format!("missing digest for {name}")))?;
        let path = folder.join(name);
        reject_link(&path, false)?;
        crate::native::validate_backup_acl(&path)?;
        let bytes = trusted_io::read_bounded(
            &path,
            kind.max_bytes(),
            "read backup artifact",
            Some(&directory),
        )?;
        let actual = sha256_upper(&bytes);
        if &actual != expected_hash {
            return Err(TuningError::BackupInvalid(format!(
                "digest mismatch for {name}"
            )));
        }
        authorize_artifact(kind, &bytes)?;
        artifacts.insert(kind, bytes);
    }
    for kind in ArtifactKind::ALL {
        directory.assert_path()?;
        if fs::symlink_metadata(folder.join(kind.file_name())).is_ok()
            && !artifacts.contains_key(&kind)
        {
            return Err(TuningError::BackupInvalid(format!(
                "unexpected artifact for disabled component: {}",
                kind.file_name()
            )));
        }
    }
    directory.assert_path()?;
    Ok(BackupBundle {
        folder: folder.to_path_buf(),
        manifest,
        artifacts,
    })
}

fn authorize_artifact(kind: ArtifactKind, bytes: &[u8]) -> Result<()> {
    match kind {
        ArtifactKind::SystemProfile => {
            parse_registry_backup(bytes, true)?;
        }
        ArtifactKind::AfdParameters => {
            parse_registry_backup(bytes, false)?;
        }
        ArtifactKind::QosPolicies => {
            parse_qos_clixml(bytes)?;
        }
        ArtifactKind::NicAdvanced => {
            parse_nic_csv(bytes)?;
        }
        ArtifactKind::NicRsc => {
            parse_rsc_csv(bytes)?;
        }
        ArtifactKind::PowerPlan => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| TuningError::BackupInvalid("power plan is not UTF-8".into()))?;
            uuid::Uuid::parse_str(
                text.trim_matches(|c: char| c.is_whitespace() || c == '{' || c == '}'),
            )
            .map_err(|_| TuningError::BackupInvalid("power plan must contain one GUID".into()))?;
        }
    }
    Ok(())
}

pub(crate) fn sha256_upper(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

fn validate_folder(folder: &Path) -> Result<()> {
    if !folder.is_absolute() {
        return Err(TuningError::BackupInvalid(
            "backup folder must be absolute".into(),
        ));
    }
    #[cfg(windows)]
    {
        let text = folder.as_os_str().to_string_lossy();
        if text.starts_with("\\\\") || text.starts_with("\\\\?\\") {
            return Err(TuningError::BackupInvalid(
                "backup folder must be a local path".into(),
            ));
        }
    }
    let mut cursor = Some(folder);
    while let Some(path) = cursor {
        if path.exists() {
            reject_link(path, true)?;
        }
        cursor = path.parent();
    }
    #[cfg(windows)]
    if let Some(parent) = folder.parent() {
        crate::native::validate_backup_parent(parent)?;
    }
    crate::native::validate_backup_acl(folder)
}

pub(crate) fn reject_link(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| TuningError::io("inspect backup path", source))?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(TuningError::BackupInvalid(format!(
            "backup path has an invalid type or link: {}",
            path.display()
        )));
    }
    Ok(())
}
