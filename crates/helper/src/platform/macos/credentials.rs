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
    let directory = directory(true)?;
    match read_from(&directory, uid) {
        Ok(secret) => return Ok(secret),
        Err(HelperError::Transport { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let name = CString::new(format!("{uid}.key")).unwrap();
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
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
    // Publish contents before transferring ownership. A failed write stays root-owned and is rejected.
    file.write_all(&secret)
        .map_err(|e| HelperError::transport("write helper credential", e))?;
    file.sync_all()
        .map_err(|e| HelperError::transport("flush helper credential", e))?;
    if unsafe { libc::fchown(file.as_raw_fd(), uid, u32::MAX) } != 0 {
        return Err(io("assign helper credential owner"));
    }
    directory
        .sync_all()
        .map_err(|e| HelperError::transport("flush credential directory", e))?;
    read_from(&directory, uid)
}
