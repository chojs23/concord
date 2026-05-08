use std::{
    collections::VecDeque,
    env,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(not(test))]
use std::{fs::OpenOptions, io::Write};

use chrono::{DateTime, Utc};

static LOGGER: OnceLock<FileLogger> = OnceLock::new();
static ERROR_LOG: OnceLock<Mutex<VecDeque<ErrorLogEntry>>> = OnceLock::new();

const MAX_ERROR_LOG_ENTRIES: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorLogEntry {
    timestamp_millis: u128,
    target: String,
    message: String,
}

impl ErrorLogEntry {
    pub fn line(&self) -> String {
        format_log_line(
            self.timestamp_millis,
            Level::Error,
            &self.target,
            &self.message,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Level {
    Debug,
    Error,
    Timing,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Error => "ERROR",
            Self::Timing => "TIMING",
        }
    }
}

#[derive(Debug)]
#[cfg_attr(test, allow(dead_code))]
struct FileLogger {
    path: Option<PathBuf>,
    debug_enabled: bool,
}

impl FileLogger {
    fn from_env() -> Self {
        Self {
            path: log_path(),
            debug_enabled: debug_enabled(),
        }
    }

    #[cfg(not(test))]
    fn write(&self, level: Level, target: &str, message: &str) {
        if !self.should_write(level) {
            return;
        }
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(
                file,
                "{}",
                format_log_line(unix_timestamp_millis(), level, target, message)
            );
        }
    }

    /// During `cargo test` we never want to touch the real log file because
    /// the unit tests below intentionally call `error(...)` with synthetic
    /// targets ("test", "history") to exercise the in-memory queue. Without
    /// this guard those entries would be appended to `$HOME/.concord/concord.log`
    /// and pollute the user's debug-log popup on the next run.
    #[cfg(test)]
    fn write(&self, _level: Level, _target: &str, _message: &str) {}

    #[cfg_attr(test, allow(dead_code))]
    fn should_write(&self, level: Level) -> bool {
        match level {
            Level::Error => true,
            Level::Debug | Level::Timing => self.debug_enabled,
        }
    }
}

pub fn debug(target: &str, message: impl AsRef<str>) {
    logger().write(Level::Debug, target, message.as_ref());
}

pub fn error(target: &str, message: impl AsRef<str>) {
    let message = message.as_ref();
    push_error_entry(target, message);
    logger().write(Level::Error, target, message);
}

pub fn error_entries() -> Vec<ErrorLogEntry> {
    error_log()
        .lock()
        .map(|entries| entries.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn timing(target: &str, message: impl AsRef<str>, duration: Duration) {
    logger().write(
        Level::Timing,
        target,
        &format!(
            "{} duration={:.2}ms",
            message.as_ref(),
            duration.as_secs_f64() * 1_000.0
        ),
    );
}

fn logger() -> &'static FileLogger {
    LOGGER.get_or_init(FileLogger::from_env)
}

fn error_log() -> &'static Mutex<VecDeque<ErrorLogEntry>> {
    ERROR_LOG.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn push_error_entry(target: &str, message: &str) {
    let Ok(mut entries) = error_log().lock() else {
        return;
    };
    if entries.len() >= MAX_ERROR_LOG_ENTRIES {
        entries.pop_front();
    }
    entries.push_back(ErrorLogEntry {
        timestamp_millis: unix_timestamp_millis(),
        target: target.to_owned(),
        message: message.to_owned(),
    });
}

fn log_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CONCORD_LOG_FILE") {
        return Some(PathBuf::from(path));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".concord").join("concord.log"))
}

fn debug_enabled() -> bool {
    env_flag("CONCORD_DEBUG")
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| flag_enabled(&value))
        .unwrap_or(false)
}

fn flag_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn format_log_line(timestamp_millis: u128, level: Level, target: &str, message: &str) -> String {
    format!(
        "{} [{}] {target}: {message}",
        format_log_timestamp(timestamp_millis),
        level.label(),
    )
}

/// Renders a millisecond Unix timestamp as `YYYY-MM-DD HH:MM:SS UTC` so the
/// debug log popup is human-readable. Falls back to the raw value if the
/// timestamp does not fit in `i64` (essentially never, but keeps the logger
/// infallible).
fn format_log_timestamp(timestamp_millis: u128) -> String {
    i64::try_from(timestamp_millis)
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| timestamp_millis.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::{error, error_entries, error_log};

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_lock() -> &'static Mutex<()> {
        TEST_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_error_log() {
        error_log().lock().expect("error log mutex").clear();
    }

    #[test]
    fn error_records_current_process_entry() {
        let _guard = test_lock().lock().expect("logging test mutex");
        clear_error_log();

        error("history", "request failed with status 403");

        let entries = error_entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].line().contains("[ERROR] history"));
        assert!(entries[0].line().contains("request failed with status 403"));
    }

    #[test]
    fn error_entries_are_bounded_to_recent_entries() {
        let _guard = test_lock().lock().expect("logging test mutex");
        clear_error_log();

        for index in 0..205 {
            error("test", format!("entry {index}"));
        }

        let entries = error_entries();
        assert_eq!(entries.len(), 200);
        assert!(entries[0].line().contains("entry 5"));
        assert!(entries[199].line().contains("entry 204"));
    }
}
