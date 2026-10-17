//! Minimal stderr logger that also mirrors each line into the active run's `run.log`.
use log::{LevelFilter, Log, Metadata, Record};
use std::io::Write;

const LEVEL_ENV: &str = "NETWORK_LANTERN_LOG";

struct Logger(LevelFilter);

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.0
    }
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {} {}: {}",
            lantern_contracts::now(),
            record.level(),
            record.target(),
            record.args()
        );
        let _ = writeln!(std::io::stderr().lock(), "{line}");
        lantern_runtime::run_log::append(&line);
    }
    fn flush(&self) {}
}

/// The level from `NETWORK_LANTERN_LOG` (default warn); `--verbose` raises it to at least debug.
fn level(env: Option<&str>, verbose: bool) -> LevelFilter {
    let level = env
        .and_then(|value| value.trim().parse::<LevelFilter>().ok())
        .filter(|level| *level != LevelFilter::Off)
        .unwrap_or(LevelFilter::Warn);
    if verbose {
        level.max(LevelFilter::Debug)
    } else {
        level
    }
}

pub fn init(verbose: bool) {
    let level = level(std::env::var(LEVEL_ENV).ok().as_deref(), verbose);
    static LOGGER: std::sync::OnceLock<Logger> = std::sync::OnceLock::new();
    if log::set_logger(LOGGER.get_or_init(|| Logger(level))).is_ok() {
        log::set_max_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_comes_from_environment_and_verbose_flag() {
        assert_eq!(level(None, false), LevelFilter::Warn);
        assert_eq!(level(Some("garbage"), false), LevelFilter::Warn);
        for (name, expected) in [
            ("error", LevelFilter::Error),
            ("warn", LevelFilter::Warn),
            ("info", LevelFilter::Info),
            ("debug", LevelFilter::Debug),
            ("TRACE", LevelFilter::Trace),
        ] {
            assert_eq!(level(Some(name), false), expected);
        }
        assert_eq!(level(None, true), LevelFilter::Debug);
        assert_eq!(level(Some("error"), true), LevelFilter::Debug);
        assert_eq!(level(Some("trace"), true), LevelFilter::Trace);
    }
}
