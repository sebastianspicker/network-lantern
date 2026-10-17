//! Per-run diagnostic log: the CLI logger mirrors its lines into the active run's `run.log`.
use std::{fs::File, io::Write, path::Path, sync::Mutex};

static RUN_LOG: Mutex<Option<File>> = Mutex::new(None);

/// Holds `run.log` open for one execution; dropping it stops mirroring.
pub(crate) struct RunLog;
impl RunLog {
    pub(crate) fn open(path: &Path) -> Self {
        match open_private(path) {
            Ok(file) => {
                if let Ok(mut slot) = RUN_LOG.lock() {
                    *slot = Some(file);
                }
            }
            Err(error) => log::warn!("cannot open {}: {error}", path.display()),
        }
        Self
    }
}
impl Drop for RunLog {
    fn drop(&mut self) {
        if let Ok(mut slot) = RUN_LOG.lock() {
            *slot = None;
        }
    }
}
/// Owner-only, never through a link or into a FIFO, like the other run artifacts.
fn open_private(path: &Path) -> std::io::Result<File> {
    lantern_platform::check_path(path).map_err(|e| std::io::Error::other(e.message))?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("run.log is not a regular file"));
    }
    Ok(file)
}
/// One physical line per record: control characters (including newlines) are escaped.
fn single_line(line: &str) -> String {
    line.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
/// Appends one formatted log line to the active run's `run.log`, if an execution is running.
pub fn append(line: &str) {
    if let Ok(mut slot) = RUN_LOG.lock()
        && let Some(file) = slot.as_mut()
    {
        let _ = writeln!(file, "{}", single_line(line));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_stay_on_one_line_and_links_are_refused() {
        assert_eq!(single_line("a\nforged INFO x"), "a\\nforged INFO x");
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let log = root.join("run.log");
        let file = open_private(&log).unwrap();
        #[cfg(not(unix))]
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
            let link = root.join("link.log");
            std::os::unix::fs::symlink(&log, &link).unwrap();
            assert!(open_private(&link).is_err());
        }
    }
}
