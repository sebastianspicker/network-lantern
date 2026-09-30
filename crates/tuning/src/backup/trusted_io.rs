use crate::{Result, TuningError};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    #[cfg(unix)]
    pub(super) device: u64,
    #[cfg(unix)]
    pub(super) inode: u64,
    #[cfg(windows)]
    pub(super) volume: u32,
    #[cfg(windows)]
    pub(super) index: u64,
}

#[derive(Debug)]
pub(crate) struct TrustedDirectory {
    path: PathBuf,
    handle: File,
    identity: FileIdentity,
}

impl TrustedDirectory {
    pub(crate) fn open(path: &Path, operation: &'static str) -> Result<Self> {
        let handle = open_directory(path, operation)?;
        let identity = identity(&handle, operation)?;
        Ok(Self {
            path: path.to_path_buf(),
            handle,
            identity,
        })
    }

    pub(crate) fn assert_path(&self) -> Result<()> {
        let current = open_directory(&self.path, "reopen trusted directory")?;
        if identity(&current, "identify trusted directory")? != self.identity {
            return Err(TuningError::BackupInvalid(format!(
                "trusted directory identity changed: {}",
                self.path.display()
            )));
        }
        Ok(())
    }

    pub(crate) fn matches_path(&self) -> bool {
        self.assert_path().is_ok()
    }

    pub(crate) fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub(crate) fn sync(&self) -> Result<()> {
        #[cfg(windows)]
        {
            // Windows has no portable directory fsync. Publication renames use
            // MoveFileExW(MOVEFILE_WRITE_THROUGH); file contents are flushed
            // through their writable handles before those renames.
            let _held_open = &self.handle;
            Ok(())
        }
        #[cfg(unix)]
        {
            self.handle
                .sync_all()
                .map_err(|source| TuningError::io("flush trusted directory", source))
        }
    }
}

pub(crate) fn directory_identity(path: &Path) -> Result<FileIdentity> {
    Ok(TrustedDirectory::open(path, "identify directory")?.identity())
}

pub(crate) fn remove_owned_directory(path: &Path, expected: FileIdentity) -> Result<()> {
    if directory_identity(path)? != expected {
        return Err(TuningError::BackupInvalid(format!(
            "refused to remove replaced directory: {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| TuningError::BackupInvalid("owned directory has no parent".into()))?;
    let disposal = parent.join(format!(
        ".network-lantern-disposal-{}",
        uuid::Uuid::new_v4().simple()
    ));
    if disposal.exists() {
        return Err(TuningError::BackupInvalid(
            "cleanup disposal path collision".into(),
        ));
    }
    std::fs::rename(path, &disposal)
        .map_err(|source| TuningError::io("isolate owned directory for cleanup", source))?;
    if directory_identity(&disposal)? != expected {
        return Err(TuningError::BackupInvalid(
            "owned directory identity changed during cleanup".into(),
        ));
    }
    std::fs::remove_dir_all(&disposal)
        .map_err(|source| TuningError::io("remove owned directory", source))?;
    TrustedDirectory::open(parent, "open cleanup parent")?.sync()
}

pub(crate) fn read_bounded(
    path: &Path,
    maximum: u64,
    operation: &'static str,
    directory: Option<&TrustedDirectory>,
) -> Result<Vec<u8>> {
    read_bounded_with_hook(path, maximum, operation, directory, || {})
}

fn read_bounded_with_hook(
    path: &Path,
    maximum: u64,
    operation: &'static str,
    directory: Option<&TrustedDirectory>,
    after_open: impl FnOnce(),
) -> Result<Vec<u8>> {
    if let Some(directory) = directory {
        directory.assert_path()?;
    }
    let mut file = open_scoped_file(path, operation, directory)?;
    let opened_identity = identity(&file, operation)?;
    let metadata = file
        .metadata()
        .map_err(|source| TuningError::io(operation, source))?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(TuningError::BackupInvalid(format!(
            "{} is not a regular non-reparse file",
            path.display()
        )));
    }
    if metadata.len() > maximum {
        return Err(TuningError::BackupInvalid(format!(
            "{} exceeds {maximum} bytes",
            path.display()
        )));
    }
    after_open();
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.by_ref()
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| TuningError::io(operation, source))?;
    if bytes.len() as u64 > maximum {
        return Err(TuningError::BackupInvalid(format!(
            "{} grew beyond {maximum} bytes",
            path.display()
        )));
    }
    if identity(&file, operation)? != opened_identity {
        return Err(TuningError::BackupInvalid(format!(
            "file identity changed while reading: {}",
            path.display()
        )));
    }
    let current = open_file(path, operation)?;
    if identity(&current, operation)? != opened_identity {
        return Err(TuningError::BackupInvalid(format!(
            "file was replaced while reading: {}",
            path.display()
        )));
    }
    if let Some(directory) = directory {
        directory.assert_path()?;
    }
    Ok(bytes)
}

fn open_scoped_file(
    path: &Path,
    operation: &'static str,
    directory: Option<&TrustedDirectory>,
) -> Result<File> {
    #[cfg(windows)]
    let _ = directory;
    #[cfg(unix)]
    if let Some(directory) = directory {
        use std::{
            ffi::CString,
            os::unix::{
                ffi::OsStrExt,
                io::{AsRawFd, FromRawFd},
            },
        };
        if path.parent() != Some(directory.path.as_path()) {
            return Err(TuningError::BackupInvalid(
                "trusted file is outside its held directory".into(),
            ));
        }
        let name = path
            .file_name()
            .ok_or_else(|| TuningError::BackupInvalid("trusted file has no file name".into()))?;
        let name = CString::new(name.as_bytes())
            .map_err(|_| TuningError::BackupInvalid("trusted file name contains NUL".into()))?;
        let descriptor = unsafe {
            libc::openat(
                directory.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 {
            return Err(map_open_error(
                path,
                operation,
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: openat returned a new owned descriptor on success.
        return Ok(unsafe { File::from_raw_fd(descriptor) });
    }
    open_file(path, operation)
}

pub(crate) fn identity_matches_path(file: &File, path: &Path) -> Result<bool> {
    let expected = identity(file, "identify retained file")?;
    let current = open_file(path, "reopen retained file")?;
    Ok(identity(&current, "identify retained file path")? == expected)
}

pub(crate) fn open_retained_file(path: &Path, operation: &'static str) -> Result<File> {
    open_file(path, operation)
}

#[cfg(windows)]
pub(crate) fn file_identity(file: &File, operation: &'static str) -> Result<FileIdentity> {
    identity(file, operation)
}

pub(crate) fn read_retained_bounded(
    file: &File,
    maximum: u64,
    operation: &'static str,
) -> Result<Vec<u8>> {
    let mut file = file
        .try_clone()
        .map_err(|source| TuningError::io(operation, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| TuningError::io(operation, source))?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > maximum {
        return Err(TuningError::BackupInvalid(
            "retained file has an invalid type or size".into(),
        ));
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))
        .map_err(|source| TuningError::io(operation, source))?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| TuningError::io(operation, source))?;
    if bytes.len() as u64 > maximum {
        return Err(TuningError::BackupInvalid(
            "retained file exceeded its size limit".into(),
        ));
    }
    Ok(bytes)
}

fn open_file(path: &Path, operation: &'static str) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
        .open(path)
        .map_err(|source| map_open_error(path, operation, source))
}

fn open_directory(path: &Path, operation: &'static str) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|source| map_open_error(path, operation, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| TuningError::io(operation, source))?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(TuningError::BackupInvalid(format!(
            "{} is not a regular non-reparse directory",
            path.display()
        )));
    }
    Ok(file)
}

fn map_open_error(path: &Path, operation: &'static str, source: std::io::Error) -> TuningError {
    if source.kind() == std::io::ErrorKind::NotFound {
        TuningError::BackupMissing(path.to_path_buf())
    } else if source.raw_os_error() == Some(libc::ELOOP) {
        TuningError::BackupInvalid(format!("refused linked path: {}", path.display()))
    } else {
        TuningError::io(operation, source)
    }
}

#[cfg(unix)]
fn identity(file: &File, operation: &'static str) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file
        .metadata()
        .map_err(|source| TuningError::io(operation, source))?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn identity(file: &File, operation: &'static str) -> Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::GetLastError,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return Err(TuningError::Native {
            operation,
            code: i64::from(unsafe { GetLastError() }),
            message: "GetFileInformationByHandle failed".into(),
        });
    }
    Ok(FileIdentity {
        volume: information.dwVolumeSerialNumber,
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(unix)]
fn is_reparse(_: &std::fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn bounded_read_rejects_growth_without_unbounded_allocation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large");
        File::create(&path).unwrap().write_all(&[7; 17]).unwrap();
        assert!(matches!(
            read_bounded(&path, 16, "test read", None),
            Err(TuningError::BackupInvalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_read_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::write(&target, b"value").unwrap();
        let link = temp.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(read_bounded(&link, 16, "test read", None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn source_file_swap_after_open_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("source");
        let old = temp.path().join("old");
        std::fs::write(&path, b"approved").unwrap();
        let result = read_bounded_with_hook(&path, 32, "test read", None, || {
            std::fs::rename(&path, &old).unwrap();
            std::fs::write(&path, b"replacement").unwrap();
        });
        assert!(matches!(result, Err(TuningError::BackupInvalid(_))));
    }

    #[cfg(unix)]
    #[test]
    fn source_directory_swap_after_open_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let directory_path = temp.path().join("bundle");
        let moved = temp.path().join("moved");
        std::fs::create_dir(&directory_path).unwrap();
        let path = directory_path.join("source");
        std::fs::write(&path, b"approved").unwrap();
        let directory = TrustedDirectory::open(&directory_path, "test directory").unwrap();
        let result = read_bounded_with_hook(&path, 32, "test read", Some(&directory), || {
            std::fs::rename(&directory_path, &moved).unwrap();
            std::fs::create_dir(&directory_path).unwrap();
            std::fs::write(directory_path.join("source"), b"replacement").unwrap();
        });
        assert!(matches!(result, Err(TuningError::BackupInvalid(_))));
    }

    #[test]
    fn owned_directory_cleanup_refuses_a_replacement_tree() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owned");
        let moved = temp.path().join("moved");
        std::fs::create_dir(&path).unwrap();
        let expected = directory_identity(&path).unwrap();
        std::fs::rename(&path, &moved).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(remove_owned_directory(&path, expected).is_err());
        assert!(path.exists());
        assert!(moved.exists());
    }
}
