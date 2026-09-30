//! Bounded data IO for unprivileged applications. Privileged recovery has a separate trust boundary.
use lantern_contracts::{Error, ErrorCategory, Result};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicFault {
    PartialWrite,
    StagedSync,
    Replace,
    #[cfg(unix)]
    SymlinkSwap,
}

#[cfg(test)]
thread_local! {
    static NEXT_ATOMIC_FAULT: Cell<Option<AtomicFault>> = const { Cell::new(None) };
}

#[cfg(test)]
fn take_atomic_fault(expected: AtomicFault) -> bool {
    NEXT_ATOMIC_FAULT.with(|slot| {
        if slot.get() == Some(expected) {
            slot.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
fn injected_atomic_error(stage: &str) -> Error {
    Error::new(
        ErrorCategory::Internal,
        format!("Injected atomic write failure at {stage}"),
    )
}

pub const PROFILE_LIMIT: usize = 1024 * 1024;
pub const REPORT_LIMIT: usize = 16 * 1024 * 1024;

/// Refuse links throughout an existing path, including dangling links.
/// This is unprivileged best-effort revalidation, not a privileged recovery primitive.
pub fn check_path(path: &Path) -> Result<()> {
    let full = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut walked = std::path::PathBuf::new();
    for component in full.components() {
        if matches!(component, Component::ParentDir) {
            return Err(Error::validation("Parent path components are not allowed"));
        }
        walked.push(component);
        match fs::symlink_metadata(&walked) {
            Ok(meta) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(Error::validation("Reparse paths are not allowed"));
                    }
                }
                if meta.file_type().is_symlink() {
                    return Err(Error::validation("Symbolic links are not allowed"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    check_path(path)?;
    let mut file = OpenOptions::new();
    file.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = file.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::validation("Expected a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::validation(format!("File exceeds {limit} bytes")));
    }
    Ok(bytes)
}
pub fn read_json(path: &Path, limit: usize) -> Result<Value> {
    let bytes = read_bounded(path, limit)?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes).map_err(|e| Error::validation(format!("Invalid JSON: {e}")))
}
pub fn atomic_json<T: Serialize>(path: &Path, value: &T, limit: usize) -> Result<()> {
    let mut output = BoundedJson {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer_pretty(&mut output, value) {
        return Err(if output.exceeded {
            Error::validation(format!("Record exceeds {limit} bytes"))
        } else {
            Error::new(ErrorCategory::Internal, error.to_string())
        });
    }
    atomic_bytes(path, &output.bytes)
}
struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(std::io::Error::other("JSON size limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    check_path(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    check_path(path)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(test)]
    if take_atomic_fault(AtomicFault::PartialWrite) {
        staged.write_all(&bytes[..bytes.len() / 2])?;
        return Err(injected_atomic_error("partial write"));
    }
    staged.write_all(bytes)?;
    #[cfg(test)]
    if take_atomic_fault(AtomicFault::StagedSync) {
        return Err(injected_atomic_error("staged file sync"));
    }
    staged.as_file().sync_all()?;
    #[cfg(all(test, unix))]
    if take_atomic_fault(AtomicFault::SymlinkSwap) {
        let original = path.with_extension("swap-original");
        let destination = path.with_extension("swap-destination");
        fs::rename(path, original)?;
        std::os::unix::fs::symlink(destination, path)?;
    }
    check_path(path)?;
    #[cfg(test)]
    if take_atomic_fault(AtomicFault::Replace) {
        return Err(injected_atomic_error("replace"));
    }
    staged.persist(path).map_err(|e| Error::from(e.error))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
/// Keep the sidecar inode stable across atomic data replacement. Drop releases the OS lock.
pub struct SidecarLock {
    _file: File,
}
impl SidecarLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        check_path(path)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        check_path(path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(Error::validation("Lock must be a regular file"));
        }
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < timeout => {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(e) => {
                    return Err(Error::new(
                        ErrorCategory::Busy,
                        format!("Could not acquire sidecar lock: {e}"),
                    ));
                }
            }
        }
    }
}

/// Read process elevation without authorizing, launching or mutating anything.
pub fn is_elevated() -> Result<bool> {
    #[cfg(unix)]
    {
        Ok(unsafe { libc::geteuid() } == 0)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = std::ptr::null_mut();
        // These calls write only into initialized, correctly sized local values.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut returned = 0;
            let success = GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            );
            let error = std::io::Error::last_os_error();
            CloseHandle(token);
            if success == 0 {
                return Err(error.into());
            }
            Ok(elevation.TokenIsElevated != 0)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(Error::new(
            ErrorCategory::Prerequisite,
            "Process elevation cannot be verified on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
    }

    fn with_atomic_fault(fault: AtomicFault, operation: impl FnOnce() -> Result<()>) -> Result<()> {
        struct ResetFault;
        impl Drop for ResetFault {
            fn drop(&mut self) {
                NEXT_ATOMIC_FAULT.with(|slot| slot.set(None));
            }
        }

        NEXT_ATOMIC_FAULT.with(|slot| {
            assert!(slot.replace(Some(fault)).is_none());
        });
        let _reset = ResetFault;
        operation()
    }

    fn assert_only_entry(dir: &Path, expected: &str) {
        let mut entries = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, [std::ffi::OsString::from(expected)]);
    }

    fn assert_fault_preserves_original(fault: AtomicFault) {
        let dir = tempdir();
        let file = dir.path().join("store.json");
        let original = br#"{"generation":"original"}"#;
        atomic_bytes(&file, original).unwrap();

        let result = with_atomic_fault(fault, || {
            atomic_bytes(&file, br#"{"generation":"replacement"}"#)
        });

        assert!(result.is_err(), "injected failure must not report success");
        assert_eq!(fs::read(&file).unwrap(), original);
        assert_only_entry(dir.path(), "store.json");
    }

    #[test]
    fn bounded_reads_and_preserve_on_serialization_failure() {
        let dir = tempdir();
        let file = dir.path().join("store.json");
        atomic_json(&file, &serde_json::json!({"ok":true}), 100).unwrap();
        let original = fs::read(&file).unwrap();
        assert!(atomic_json(&file, &"x".repeat(100), 5).is_err());
        assert_eq!(fs::read(&file).unwrap(), original);
        assert!(read_bounded(&file, 1).is_err());
    }

    #[test]
    fn partial_write_failure_preserves_original() {
        assert_fault_preserves_original(AtomicFault::PartialWrite);
    }

    #[test]
    fn staged_sync_failure_preserves_original() {
        assert_fault_preserves_original(AtomicFault::StagedSync);
    }

    #[test]
    fn bounded_serialization_stops_before_allocating_or_replacing_oversized_output() {
        let dir = tempdir();
        let target = dir.path().join("store.json");
        fs::write(&target, b"original").unwrap();
        let result = atomic_json(&target, &vec!["amplified"; 100_000], 128);
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert_only_entry(dir.path(), "store.json");
        let mut writer = BoundedJson {
            bytes: Vec::new(),
            limit: 128,
            exceeded: false,
        };
        assert!(serde_json::to_writer_pretty(&mut writer, &vec!["amplified"; 100_000]).is_err());
        assert!(writer.bytes.len() <= 128);
        assert!(writer.bytes.capacity() <= 256);
    }

    #[test]
    fn replace_failure_preserves_original() {
        assert_fault_preserves_original(AtomicFault::Replace);
    }

    #[test]
    fn target_directory_collision_reports_failure_without_damage() {
        let dir = tempdir();
        let target = dir.path().join("store.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("original"), b"unchanged").unwrap();

        let result = atomic_bytes(&target, b"replacement");

        assert!(result.is_err(), "target collision must not report success");
        assert_eq!(fs::read(target.join("original")).unwrap(), b"unchanged");
        assert_only_entry(dir.path(), "store.json");
    }

    #[test]
    fn sidecar_lock_serializes_contenders_and_releases_on_drop() {
        let dir = tempdir();
        let lock_path = dir.path().join("file.lock");
        fs::write(&lock_path, b"owner-token").unwrap();
        let lock = SidecarLock::acquire(&lock_path, Duration::ZERO).unwrap();
        assert_eq!(fs::read(&lock_path).unwrap(), b"owner-token");
        let contender_path = lock_path.clone();
        let contender =
            thread::spawn(move || SidecarLock::acquire(&contender_path, Duration::from_millis(75)));
        let error = contender.join().unwrap().err().unwrap();
        assert_eq!(error.category, ErrorCategory::Busy);
        drop(lock);
        assert!(SidecarLock::acquire(&lock_path, Duration::ZERO).is_ok());
        assert_eq!(fs::read(lock_path).unwrap(), b"owner-token");
    }

    #[cfg(unix)]
    #[test]
    fn new_sidecar_lock_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir();
        let lock_path = dir.path().join("file.lock");
        let _lock = SidecarLock::acquire(&lock_path, Duration::ZERO).unwrap();

        assert_eq!(
            fs::metadata(lock_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_refused_for_read_write_lock() {
        let dir = tempdir();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path().join("missing"), &link).unwrap();
        assert!(read_bounded(&link, 10).is_err());
        assert!(atomic_bytes(&link, b"no").is_err());
        assert!(SidecarLock::acquire(&link, Duration::ZERO).is_err());
        assert!(!dir.path().join("missing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_swap_before_revalidation_is_refused() {
        let dir = tempdir();
        let file = dir.path().join("store.json");
        let original = br#"{"generation":"original"}"#;
        let destination = file.with_extension("swap-destination");
        fs::write(&file, original).unwrap();
        fs::write(&destination, b"sentinel").unwrap();

        let result = with_atomic_fault(AtomicFault::SymlinkSwap, || {
            atomic_bytes(&file, br#"{"generation":"replacement"}"#)
        });

        assert!(result.is_err(), "symlink swap must not report success");
        assert_eq!(
            fs::read(file.with_extension("swap-original")).unwrap(),
            original
        );
        assert_eq!(fs::read(destination).unwrap(), b"sentinel");
        assert!(fs::symlink_metadata(file).unwrap().file_type().is_symlink());
    }
}
