use super::trusted_io::FileIdentity;
use crate::{Result, TuningError};
use std::path::Path;

pub(crate) trait PublicationFs {
    fn exists(&self, path: &Path) -> bool;
    fn identity(&self, path: &Path) -> Result<FileIdentity>;
    fn validate(&self, path: &Path) -> Result<FileIdentity>;
    fn rename(&self, from: &Path, to: &Path, expected: FileIdentity) -> Result<()>;
    fn remove_if_identity(&self, path: &Path, identity: FileIdentity) -> Result<()>;
    fn sync_parent(&self, parent: &Path) -> Result<()>;
}

struct PublicationPaths<'a> {
    parent: &'a Path,
    destination: &'a Path,
    previous: &'a Path,
    quarantine: &'a Path,
}

/// Replaces `destination` with an already complete staging directory.
///
/// Every destructive step is identity checked by the implementation. A failed
/// validation or durability barrier quarantines the new tree and restores the
/// old tree before returning an error.
pub(crate) fn replace_verified(
    fs: &impl PublicationFs,
    parent: &Path,
    staging: &Path,
    destination: &Path,
    previous: &Path,
    quarantine: &Path,
) -> Result<()> {
    let paths = PublicationPaths {
        parent,
        destination,
        previous,
        quarantine,
    };
    if fs.exists(previous) || fs.exists(quarantine) {
        return Err(TuningError::BackupInvalid(
            "backup publication path collision".into(),
        ));
    }
    let staging_identity = fs.validate(staging)?;
    let prior = if fs.exists(destination) {
        Some(fs.validate(destination)?)
    } else {
        None
    };

    if let Some(prior_identity) = prior {
        fs.rename(destination, previous, prior_identity)?;
        if let Err(error) = fs.sync_parent(parent) {
            return rollback_before_publish(fs, &paths, prior, error);
        }
    }

    if let Err(error) = fs.rename(staging, destination, staging_identity) {
        return rollback_before_publish(fs, &paths, prior, error);
    }

    let published = fs
        .validate(destination)
        .and_then(|identity| {
            if identity != staging_identity {
                Err(TuningError::BackupInvalid(
                    "published backup identity differs from verified staging".into(),
                ))
            } else {
                Ok(())
            }
        })
        .and_then(|()| fs.sync_parent(parent));
    if let Err(error) = published {
        return quarantine_and_restore(fs, &paths, staging_identity, prior, error);
    }

    if let Some(prior_identity) = prior {
        fs.remove_if_identity(previous, prior_identity)?;
        fs.sync_parent(parent)?;
    }
    Ok(())
}

fn rollback_before_publish(
    fs: &impl PublicationFs,
    paths: &PublicationPaths<'_>,
    prior: Option<FileIdentity>,
    cause: TuningError,
) -> Result<()> {
    if !fs.exists(paths.previous) {
        return Err(cause);
    }
    let Some(prior) = prior else {
        return Err(recovery_message(
            cause,
            "unexpected previous backup appeared during rollback",
        ));
    };
    match fs
        .rename(paths.previous, paths.destination, prior)
        .and_then(|()| fs.sync_parent(paths.parent))
    {
        Ok(()) => Err(cause),
        Err(rollback) => Err(recovery_error(cause, "restore previous backup", rollback)),
    }
}

fn quarantine_and_restore(
    fs: &impl PublicationFs,
    paths: &PublicationPaths<'_>,
    staging_identity: FileIdentity,
    prior: Option<FileIdentity>,
    cause: TuningError,
) -> Result<()> {
    let current = fs.identity(paths.destination);
    match current {
        Ok(identity) if identity == staging_identity => {}
        Ok(_) => {
            return Err(recovery_message(
                cause,
                "published path was replaced before quarantine; previous backup retained",
            ));
        }
        Err(error) => {
            return Err(recovery_error(
                cause,
                "revalidate failed publication",
                error,
            ));
        }
    }
    if let Err(error) = fs
        .rename(paths.destination, paths.quarantine, staging_identity)
        .and_then(|()| fs.sync_parent(paths.parent))
    {
        return Err(recovery_error(
            cause,
            "quarantine failed publication",
            error,
        ));
    }
    if fs.exists(paths.previous)
        && let Err(error) = fs
            .rename(
                paths.previous,
                paths.destination,
                prior.ok_or_else(|| {
                    TuningError::BackupInvalid(
                        "unexpected previous backup appeared during recovery".into(),
                    )
                })?,
            )
            .and_then(|()| fs.sync_parent(paths.parent))
    {
        return Err(recovery_error(cause, "restore previous backup", error));
    }
    Err(cause)
}

fn recovery_error(cause: TuningError, operation: &str, recovery: TuningError) -> TuningError {
    recovery_message(cause, &format!("{operation}: {recovery}"))
}

fn recovery_message(cause: TuningError, recovery: &str) -> TuningError {
    TuningError::BackupInvalid(format!(
        "backup publication failed: {cause}; recovery incomplete: {recovery}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
    };

    #[derive(Default)]
    struct FakeFs {
        entries: RefCell<BTreeMap<PathBuf, FileIdentity>>,
        fail: RefCell<BTreeSet<&'static str>>,
        sync_calls: Cell<usize>,
    }

    impl FakeFs {
        fn id(value: u64) -> FileIdentity {
            #[cfg(unix)]
            return FileIdentity {
                device: 1,
                inode: value,
            };
            #[cfg(windows)]
            return FileIdentity {
                volume: 1,
                index: value,
            };
        }

        fn with(prior: bool) -> Self {
            let mut entries = BTreeMap::from([(PathBuf::from("stage"), Self::id(2))]);
            if prior {
                entries.insert(PathBuf::from("final"), Self::id(1));
            }
            Self {
                entries: RefCell::new(entries),
                ..Self::default()
            }
        }

        fn fail(&self, point: &'static str) {
            self.fail.borrow_mut().insert(point);
        }

        fn take_fail(&self, point: &'static str) -> bool {
            self.fail.borrow_mut().remove(point)
        }

        fn has(&self, path: &str, id: u64) -> bool {
            self.entries.borrow().get(Path::new(path)) == Some(&Self::id(id))
        }
    }

    impl PublicationFs for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            self.entries.borrow().contains_key(path)
        }
        fn validate(&self, path: &Path) -> Result<FileIdentity> {
            let identity = self
                .entries
                .borrow()
                .get(path)
                .copied()
                .ok_or_else(|| TuningError::BackupInvalid("missing fake path".into()))?;
            if path == Path::new("final")
                && identity == Self::id(2)
                && self.fail.borrow().contains("validate-final")
            {
                return Err(TuningError::BackupInvalid("injected validation".into()));
            }
            Ok(identity)
        }
        fn identity(&self, path: &Path) -> Result<FileIdentity> {
            self.entries
                .borrow()
                .get(path)
                .copied()
                .ok_or_else(|| TuningError::BackupInvalid("missing fake path".into()))
        }
        fn rename(&self, from: &Path, to: &Path, expected: FileIdentity) -> Result<()> {
            let point = match (from.to_str(), to.to_str()) {
                (Some("stage"), Some("final")) => "rename-publish",
                (Some("previous"), Some("final")) => "rename-rollback",
                (Some("final"), Some("quarantine")) => "rename-quarantine",
                _ => "rename-other",
            };
            if self.take_fail(point) {
                return Err(TuningError::BackupInvalid(format!("injected {point}")));
            }
            if point == "rename-publish" && self.take_fail("replace-source") {
                self.entries
                    .borrow_mut()
                    .insert(from.to_path_buf(), Self::id(99));
            }
            let value = self
                .entries
                .borrow_mut()
                .remove(from)
                .ok_or_else(|| TuningError::BackupInvalid("missing rename source".into()))?;
            if value != expected {
                self.entries.borrow_mut().insert(from.to_path_buf(), value);
                return Err(TuningError::BackupInvalid(
                    "rename source identity changed".into(),
                ));
            }
            if self.entries.borrow().contains_key(to) {
                return Err(TuningError::BackupInvalid("rename collision".into()));
            }
            self.entries.borrow_mut().insert(to.to_path_buf(), value);
            Ok(())
        }
        fn remove_if_identity(&self, path: &Path, identity: FileIdentity) -> Result<()> {
            if self.take_fail("remove") {
                return Err(TuningError::BackupInvalid("injected remove".into()));
            }
            if self.take_fail("replace-before-remove") {
                self.entries
                    .borrow_mut()
                    .insert(path.to_path_buf(), Self::id(99));
            }
            if self.entries.borrow().get(path) != Some(&identity) {
                return Err(TuningError::BackupInvalid(
                    "identity changed before removal".into(),
                ));
            }
            self.entries.borrow_mut().remove(path);
            Ok(())
        }
        fn sync_parent(&self, _: &Path) -> Result<()> {
            self.sync_calls.set(self.sync_calls.get() + 1);
            if self.take_fail("sync") {
                Err(TuningError::BackupInvalid("injected sync".into()))
            } else {
                Ok(())
            }
        }
    }

    fn replace(fs: &FakeFs) -> Result<()> {
        replace_verified(
            fs,
            Path::new("parent"),
            Path::new("stage"),
            Path::new("final"),
            Path::new("previous"),
            Path::new("quarantine"),
        )
    }

    #[test]
    fn publish_success_removes_only_original_previous_identity() {
        let fs = FakeFs::with(true);
        replace(&fs).unwrap();
        assert!(fs.has("final", 2));
        assert!(!fs.exists(Path::new("previous")));
        assert!(fs.sync_calls.get() >= 3);
    }

    #[test]
    fn publish_rename_failure_restores_previous() {
        let fs = FakeFs::with(true);
        fs.fail("rename-publish");
        assert!(replace(&fs).is_err());
        assert!(fs.has("final", 1));
        assert!(fs.has("stage", 2));
    }

    #[test]
    fn failed_final_validation_quarantines_new_and_restores_previous() {
        let fs = FakeFs::with(true);
        fs.fail("validate-final");
        assert!(replace(&fs).is_err());
        assert!(fs.has("final", 1));
        assert!(fs.has("quarantine", 2));
    }

    #[test]
    fn quarantine_failure_keeps_previous_and_reports_incomplete_recovery() {
        let fs = FakeFs::with(true);
        fs.fail("validate-final");
        fs.fail("rename-quarantine");
        let error = replace(&fs).unwrap_err().to_string();
        assert!(error.contains("recovery incomplete"));
        assert!(fs.has("previous", 1));
        assert!(fs.has("final", 2));
    }

    #[test]
    fn rollback_failure_is_reported_and_previous_is_retained() {
        let fs = FakeFs::with(true);
        fs.fail("rename-publish");
        fs.fail("rename-rollback");
        let error = replace(&fs).unwrap_err().to_string();
        assert!(error.contains("recovery incomplete"));
        assert!(fs.has("previous", 1));
    }

    #[test]
    fn collision_is_rejected_before_moving_destination() {
        let fs = FakeFs::with(true);
        fs.entries
            .borrow_mut()
            .insert(PathBuf::from("previous"), FakeFs::id(3));
        assert!(replace(&fs).is_err());
        assert!(fs.has("final", 1));
        assert!(fs.has("stage", 2));
    }

    #[test]
    fn durability_failure_rolls_back_and_quarantines_new_tree() {
        let fs = FakeFs::with(false);
        fs.fail("sync");
        assert!(replace(&fs).is_err());
        assert!(!fs.exists(Path::new("final")));
        assert!(fs.has("quarantine", 2));
    }

    #[test]
    fn source_identity_swap_before_publish_is_rejected_and_prior_restored() {
        let fs = FakeFs::with(true);
        fs.fail("replace-source");
        assert!(replace(&fs).is_err());
        assert!(fs.has("final", 1));
        assert!(fs.has("stage", 99));
    }

    #[test]
    fn replaced_previous_tree_is_never_deleted() {
        let fs = FakeFs::with(true);
        fs.fail("replace-before-remove");
        assert!(replace(&fs).is_err());
        assert!(fs.has("final", 2));
        assert!(fs.has("previous", 99));
    }
}
