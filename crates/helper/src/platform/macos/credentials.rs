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
const COMPONENTS: [&str; 4] = [
    "Library",
    "Application Support",
    "NetworkLantern",
    "clients",
];
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
    provision_in(&directory(true)?, uid)
}
fn provision_in(directory: &File, uid: u32) -> Result<[u8; 32]> {
    match read_from(directory, uid) {
        Ok(secret) => return Ok(secret),
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let name = CString::new(format!("{uid}.key")).unwrap();
    let temporary = CString::new(format!(".{uid}.{:016x}.key", rand::random::<u64>())).unwrap();
    let published = publish(directory, &temporary, &name, uid);
    // The credential is now either linked under its final name or abandoned.
    unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    published?;
    directory
        .sync_all()
        .map_err(|e| HelperError::transport("flush credential directory", e))?;
    read_from(directory, uid)
}
/// Writes a complete, correctly owned credential under a temporary name, then links it into
/// place. `linkat` fails if the name exists, so a crash never leaves a partial credential.
fn publish(directory: &File, temporary: &CString, name: &CString, uid: u32) -> Result<()> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io("create helper credential"));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut secret = [0; 32];
    rand::rng().fill_bytes(&mut secret);
    file.write_all(&secret)
        .map_err(|e| HelperError::transport("write helper credential", e))?;
    if unsafe { libc::fchown(file.as_raw_fd(), uid, u32::MAX) } != 0 {
        return Err(io("assign helper credential owner"));
    }
    file.sync_all()
        .map_err(|e| HelperError::transport("flush helper credential", e))?;
    if unsafe {
        libc::linkat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
            0,
        )
    } != 0
    {
        return Err(io("publish helper credential"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_publishes_one_complete_credential_and_keeps_it() {
        let temp = tempfile::tempdir().unwrap();
        let directory = File::open(temp.path()).unwrap();
        let uid = unsafe { libc::geteuid() };

        let first = provision_in(&directory, uid).unwrap();
        assert_eq!(provision_in(&directory, uid).unwrap(), first);
        let names: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(format!("{uid}.key"))]);
    }
}
