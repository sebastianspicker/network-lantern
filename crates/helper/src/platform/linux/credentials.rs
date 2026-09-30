//! Handle-relative credential access. No user-supplied paths enter this boundary.
use crate::{HelperError, Result};
use rand::RngCore;
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};
const COMPONENTS: [&str; 4] = ["var", "lib", "network-lantern", "clients"];

fn io(operation: &'static str) -> HelperError {
    HelperError::transport(operation, std::io::Error::last_os_error())
}
fn directory(create: bool) -> Result<File> {
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io("open credential root"));
    }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    for (index, part) in COMPONENTS.iter().enumerate() {
        let name = CString::new(*part).unwrap();
        if create && index >= 2 {
            let result = unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o755) };
            if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(io("create credential directory"));
            }
        }
        let next = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            return Err(io("open protected credential directory"));
        }
        directory = unsafe { File::from_raw_fd(next) };
        let metadata = directory
            .metadata()
            .map_err(|e| HelperError::transport("inspect credential directory", e))?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(HelperError::Authorization(
                "Credential directories must be root-owned and not writable by other identities"
                    .into(),
            ));
        }
    }
    Ok(directory)
}
pub fn read(uid: u32) -> Result<[u8; 32]> {
    read_from(&directory(false)?, uid)
}

pub fn present(uid: u32) -> Result<bool> {
    let directory = match directory(false) {
        Ok(directory) => directory,
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    present_in(&directory, uid)
}

fn present_in(directory: &File, uid: u32) -> Result<bool> {
    match read_from(directory, uid) {
        Ok(_) => Ok(true),
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn read_from(directory: &File, uid: u32) -> Result<[u8; 32]> {
    let name = CString::new(format!("{uid}.key")).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io("open helper credential"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let meta = file
        .metadata()
        .map_err(|e| HelperError::transport("inspect helper credential", e))?;
    if !meta.is_file()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
        || meta.len() != 32
    {
        return Err(HelperError::Authentication);
    }
    let mut bytes = [0; 32];
    file.read_exact(&mut bytes)
        .map_err(|e| HelperError::transport("read helper credential", e))?;
    Ok(bytes)
}
pub fn provision(uid: u32) -> Result<[u8; 32]> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "Only the authorized root helper can provision credentials".into(),
        ));
    }
    let directory = directory(true)?;
    rotate_in(&directory, uid)
}

fn rotate_in(directory: &File, uid: u32) -> Result<[u8; 32]> {
    let previous = match read_from(directory, uid) {
        Ok(secret) => Some(secret),
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    let destination = CString::new(format!("{uid}.key")).unwrap();
    let temporary = CString::new(format!(".{uid}.{:016x}.key", rand::random::<u64>())).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io("create fresh helper credential"));
    }
    let mut cleanup = TemporaryCredential {
        directory,
        name: &temporary,
        published: false,
    };
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut secret = [0; 32];
    loop {
        rand::rng().fill_bytes(&mut secret);
        if previous != Some(secret) {
            break;
        }
    }
    // Publish complete contents before atomically replacing a validated prior incarnation.
    file.write_all(&secret)
        .map_err(|e| HelperError::transport("write helper credential", e))?;
    file.sync_all()
        .map_err(|e| HelperError::transport("flush helper credential", e))?;
    if unsafe { libc::fchown(file.as_raw_fd(), uid, u32::MAX) } != 0 {
        return Err(io("assign helper credential owner"));
    }
    file.sync_all()
        .map_err(|e| HelperError::transport("flush helper credential owner", e))?;
    if unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
        )
    } != 0
    {
        return Err(io("publish fresh helper credential"));
    }
    cleanup.published = true;
    directory
        .sync_all()
        .map_err(|e| HelperError::transport("flush credential directory", e))?;
    read_from(directory, uid)
}

pub fn revoke(uid: u32) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "Only the authorized root helper can revoke credentials".into(),
        ));
    }
    revoke_in(&directory(false)?, uid)
}

fn revoke_in(directory: &File, uid: u32) -> Result<()> {
    match read_from(directory, uid) {
        Ok(_) => {}
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    let name = CString::new(format!("{uid}.key")).unwrap();
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io("revoke helper credential"));
    }
    directory
        .sync_all()
        .map_err(|e| HelperError::transport("flush credential revocation", e))
}

struct TemporaryCredential<'a> {
    directory: &'a File,
    name: &'a CString,
    published: bool,
}

impl Drop for TemporaryCredential<'_> {
    fn drop(&mut self) {
        if !self.published {
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn each_incarnation_rotates_then_revokes_its_exact_uid_credential() {
        let temp = tempfile::tempdir().unwrap();
        let directory = File::open(temp.path()).unwrap();
        let uid = unsafe { libc::geteuid() };

        let first = rotate_in(&directory, uid).unwrap();
        assert!(present_in(&directory, uid).unwrap());
        assert_eq!(read_from(&directory, uid).unwrap(), first);
        let second = rotate_in(&directory, uid).unwrap();
        assert_ne!(second, first);
        assert_eq!(read_from(&directory, uid).unwrap(), second);

        revoke_in(&directory, uid).unwrap();
        assert!(!present_in(&directory, uid).unwrap());
        assert!(matches!(
            read_from(&directory, uid),
            Err(HelperError::Transport { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound
        ));
        revoke_in(&directory, uid).unwrap();
    }

    #[test]
    fn revocation_refuses_symlink_and_hard_link_substitution() {
        let temp = tempfile::tempdir().unwrap();
        let directory = File::open(temp.path()).unwrap();
        let uid = unsafe { libc::geteuid() };
        let credential = temp.path().join(format!("{uid}.key"));
        let target = temp.path().join("outside");
        std::fs::write(&target, [7; 32]).unwrap();
        symlink(&target, &credential).unwrap();

        assert!(revoke_in(&directory, uid).is_err());
        assert!(present_in(&directory, uid).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), vec![7; 32]);
        std::fs::remove_file(&credential).unwrap();

        rotate_in(&directory, uid).unwrap();
        let alias = temp.path().join("alias");
        std::fs::hard_link(&credential, &alias).unwrap();
        assert!(matches!(
            revoke_in(&directory, uid),
            Err(HelperError::Authentication)
        ));
        assert!(credential.exists());
        assert!(alias.exists());
    }
}
