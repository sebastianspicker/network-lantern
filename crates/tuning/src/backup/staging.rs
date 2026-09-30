use super::{
    BackupBundle, authorize_artifact, sha256_upper,
    trusted_io::{self, TrustedDirectory},
};
use crate::{Result, TuningError};
use rand::RngCore;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct StagedBackup {
    path: PathBuf,
    nonce: [u8; 32],
    sentinel: Option<File>,
    directory: Option<TrustedDirectory>,
    manifest: super::BackupManifest,
}

struct CleanupGuard {
    path: PathBuf,
    directory: Option<TrustedDirectory>,
}

impl CleanupGuard {
    fn take_directory(&mut self) -> TrustedDirectory {
        self.directory.take().expect("staging directory is owned")
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if self
            .directory
            .as_ref()
            .is_some_and(TrustedDirectory::matches_path)
        {
            let identity = self
                .directory
                .as_ref()
                .expect("checked staging directory")
                .identity();
            drop(self.directory.take());
            let _ = trusted_io::remove_owned_directory(&self.path, identity);
        }
    }
}

impl BackupBundle {
    pub fn stage_verified(&self) -> Result<StagedBackup> {
        self.verify_in_memory()?;
        let parent = crate::native::staging_parent()?;
        crate::native::validate_staging_parent(&parent)?;
        let parent_directory = TrustedDirectory::open(&parent, "open restore staging parent")?;
        let path = parent.join(format!(
            "NetworkLantern-Restore-{}",
            uuid::Uuid::new_v4().simple()
        ));
        create_private_dir(&path)?;
        let mut cleanup = CleanupGuard {
            path: path.clone(),
            directory: Some(TrustedDirectory::open(
                &path,
                "open restore staging directory",
            )?),
        };
        parent_directory.assert_path()?;
        parent_directory.sync()?;
        crate::native::protect_directory(&path)?;
        let mut nonce = [0_u8; 32];
        rand::rng().fill_bytes(&mut nonce);
        let sentinel_path = path.join(".restore-session");
        let mut sentinel = create_private_file(&sentinel_path)?;
        sentinel
            .write_all(&nonce)
            .and_then(|()| sentinel.sync_all())
            .map_err(|source| TuningError::io("write restore sentinel", source))?;
        crate::native::protect_file(&sentinel_path)?;
        drop(sentinel);
        let sentinel =
            trusted_io::open_retained_file(&sentinel_path, "reopen protected restore sentinel")?;
        for (kind, bytes) in &self.artifacts {
            let destination = path.join(kind.file_name());
            let mut file = create_private_file(&destination)?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|source| TuningError::io("stage backup artifact", source))?;
            crate::native::protect_file(&destination)?;
            drop(file);
            let exact = trusted_io::read_bounded(
                &destination,
                kind.max_bytes(),
                "verify written staging artifact",
                cleanup.directory.as_ref(),
            )?;
            let expected = self
                .manifest
                .artifact_digests
                .get(kind.file_name())
                .expect("validated digest");
            if sha256_upper(&exact) != *expected {
                return Err(TuningError::BackupInvalid(format!(
                    "digest mismatch while staging {}",
                    kind.file_name()
                )));
            }
            authorize_artifact(*kind, &exact)?;
        }
        cleanup
            .directory
            .as_ref()
            .expect("staging directory is owned")
            .sync()?;
        let staged = StagedBackup {
            path,
            nonce,
            sentinel: Some(sentinel),
            directory: Some(cleanup.take_directory()),
            manifest: self.manifest.clone(),
        };
        staged.verify()?;
        Ok(staged)
    }

    fn verify_in_memory(&self) -> Result<()> {
        let expected = self.manifest.expected_artifacts();
        if expected.len() != self.artifacts.len()
            || expected.len() != self.manifest.artifact_digests.len()
        {
            return Err(TuningError::BackupInvalid(
                "in-memory backup does not match enabled components".into(),
            ));
        }
        for kind in expected {
            let bytes = self.artifacts.get(&kind).ok_or_else(|| {
                TuningError::BackupInvalid(format!("missing in-memory {}", kind.file_name()))
            })?;
            let digest = self
                .manifest
                .artifact_digests
                .get(kind.file_name())
                .ok_or_else(|| {
                    TuningError::BackupInvalid(format!("missing digest for {}", kind.file_name()))
                })?;
            if sha256_upper(bytes) != *digest {
                return Err(TuningError::BackupInvalid(format!(
                    "in-memory digest mismatch for {}",
                    kind.file_name()
                )));
            }
            authorize_artifact(kind, bytes)?;
        }
        Ok(())
    }
}

impl StagedBackup {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn verify(&self) -> Result<()> {
        self.directory().assert_path()?;
        crate::native::validate_staging_path(&self.path)?;
        let sentinel_path = self.path.join(".restore-session");
        crate::native::validate_staging_path(&sentinel_path)?;
        if !trusted_io::identity_matches_path(self.sentinel(), &sentinel_path)? {
            return Err(TuningError::BackupInvalid(
                "restore staging sentinel was replaced".into(),
            ));
        }
        let value =
            trusted_io::read_retained_bounded(self.sentinel(), 32, "read restore sentinel")?;
        if value != self.nonce {
            return Err(TuningError::BackupInvalid(
                "restore staging sentinel changed".into(),
            ));
        }
        for kind in self.manifest.expected_artifacts() {
            self.read_verified(kind)?;
        }
        self.directory().assert_path()?;
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) fn read(&self, kind: super::ArtifactKind) -> Result<Vec<u8>> {
        self.verify()?;
        self.read_verified(kind)
    }

    #[cfg(windows)]
    pub(crate) fn expected_artifacts(&self) -> Vec<super::ArtifactKind> {
        self.manifest.expected_artifacts()
    }

    fn read_verified(&self, kind: super::ArtifactKind) -> Result<Vec<u8>> {
        if !self.manifest.expected_artifacts().contains(&kind) {
            return Err(TuningError::BackupInvalid(format!(
                "staged artifact is not enabled: {}",
                kind.file_name()
            )));
        }
        self.directory().assert_path()?;
        let path = self.path.join(kind.file_name());
        crate::native::validate_staging_path(&path)?;
        let bytes = trusted_io::read_bounded(
            &path,
            kind.max_bytes(),
            "read staged artifact",
            Some(self.directory()),
        )?;
        let digest = self
            .manifest
            .artifact_digests
            .get(kind.file_name())
            .ok_or_else(|| {
                TuningError::BackupInvalid(format!(
                    "staged digest is missing for {}",
                    kind.file_name()
                ))
            })?;
        if sha256_upper(&bytes) != *digest {
            return Err(TuningError::BackupInvalid(format!(
                "staged digest mismatch for {}",
                kind.file_name()
            )));
        }
        authorize_artifact(kind, &bytes)?;
        self.directory().assert_path()?;
        Ok(bytes)
    }

    fn directory(&self) -> &TrustedDirectory {
        self.directory.as_ref().expect("staging directory is owned")
    }

    fn sentinel(&self) -> &File {
        self.sentinel.as_ref().expect("staging sentinel is owned")
    }
}

impl Drop for StagedBackup {
    fn drop(&mut self) {
        let sentinel_path = self.path.join(".restore-session");
        let owned = self.directory().matches_path()
            && trusted_io::identity_matches_path(self.sentinel(), &sentinel_path).unwrap_or(false);
        if owned {
            let identity = self.directory().identity();
            drop(self.sentinel.take());
            drop(self.directory.take());
            let _ = trusted_io::remove_owned_directory(&self.path, identity);
        }
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    builder
        .create(path)
        .map_err(|source| TuningError::io("create restore staging directory", source))
}

fn create_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .map_err(|source| TuningError::io("create protected file", source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{ArtifactKind, BackupManifest, manifest::BackupManifestComponents};
    use std::collections::BTreeMap;

    #[test]
    fn staged_artifact_tampering_is_detected_before_consumption() {
        let bytes = b"8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".to_vec();
        let bundle = BackupBundle {
            folder: PathBuf::from("unused"),
            manifest: BackupManifest {
                schema_version: 3,
                tool_name: "network-lantern".into(),
                machine_name: Some("test".into()),
                platform: Some("test".into()),
                os_version: Some("test".into()),
                module_version: Some("test".into()),
                timestamp: Some("2026-01-01T00:00:00Z".into()),
                components: BackupManifestComponents {
                    system_profile: false,
                    afd_parameters: false,
                    qos_policies: false,
                    nic_advanced: false,
                    nic_rsc: false,
                    power_plan: true,
                },
                artifact_digests: BTreeMap::from([(
                    ArtifactKind::PowerPlan.file_name().into(),
                    sha256_upper(&bytes),
                )]),
            },
            artifacts: BTreeMap::from([(ArtifactKind::PowerPlan, bytes)]),
        };
        let staged = bundle.stage_verified().unwrap();
        std::fs::write(
            staged.path().join(ArtifactKind::PowerPlan.file_name()),
            b"tampered",
        )
        .unwrap();
        assert!(matches!(
            staged.verify(),
            Err(TuningError::BackupInvalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn sentinel_replacement_is_detected_and_cleanup_preserves_replacement_tree() {
        let bundle = power_plan_bundle();
        let staged = bundle.stage_verified().unwrap();
        let path = staged.path().to_path_buf();
        let original = path.with_file_name("original-staging-tree");
        std::fs::rename(&path, &original).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join(".restore-session"), [0_u8; 32]).unwrap();

        assert!(staged.verify().is_err());
        drop(staged);
        assert!(path.exists(), "drop must not delete a replacement tree");
        assert!(
            original.exists(),
            "the original moved tree must be retained"
        );
        std::fs::remove_dir_all(path).unwrap();
        std::fs::remove_dir_all(original).unwrap();
    }

    #[test]
    fn early_staging_failure_removes_only_the_owned_partial_tree() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("partial");
        create_private_dir(&path).unwrap();
        let guard = CleanupGuard {
            path: path.clone(),
            directory: Some(
                TrustedDirectory::open(&path, "open partial staging directory").unwrap(),
            ),
        };
        std::fs::write(path.join("partial-artifact"), b"partial").unwrap();
        drop(guard);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    fn power_plan_bundle() -> BackupBundle {
        let bytes = b"8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".to_vec();
        BackupBundle {
            folder: PathBuf::from("unused"),
            manifest: BackupManifest {
                schema_version: 3,
                tool_name: "network-lantern".into(),
                machine_name: Some("test".into()),
                platform: Some("test".into()),
                os_version: Some("test".into()),
                module_version: Some("test".into()),
                timestamp: Some("2026-01-01T00:00:00Z".into()),
                components: BackupManifestComponents {
                    system_profile: false,
                    afd_parameters: false,
                    qos_policies: false,
                    nic_advanced: false,
                    nic_rsc: false,
                    power_plan: true,
                },
                artifact_digests: BTreeMap::from([(
                    ArtifactKind::PowerPlan.file_name().into(),
                    sha256_upper(&bytes),
                )]),
            },
            artifacts: BTreeMap::from([(ArtifactKind::PowerPlan, bytes)]),
        }
    }
}
