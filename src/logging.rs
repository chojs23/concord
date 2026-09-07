use std::{
    collections::VecDeque,
    env,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
use std::{
    fs::File,
    io::{BufRead, BufReader},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    thread,
};
#[cfg(not(test))]
use std::{fs::OpenOptions, io::Write};

use chrono::{DateTime, Utc};

use crate::paths;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();
static ERROR_LOG: OnceLock<Mutex<VecDeque<ErrorLogEntry>>> = OnceLock::new();

const MAX_ERROR_LOG_ENTRIES: usize = 200;
#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
const NATIVE_STDERR_TARGET: &str = "native-stderr";

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
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Error => "ERROR",
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

    /// Tests exercise logging with synthetic entries, so they must not write to
    /// the user's real log file.
    #[cfg(test)]
    fn write(&self, _level: Level, _target: &str, _message: &str) {}

    #[cfg_attr(test, allow(dead_code))]
    fn should_write(&self, level: Level) -> bool {
        match level {
            Level::Error => true,
            Level::Debug => self.debug_enabled,
        }
    }
}

pub fn debug_logging_enabled() -> bool {
    logger().debug_enabled
}

pub fn debug(target: &str, message: impl AsRef<str>) {
    logger().write(Level::Debug, target, message.as_ref());
}

pub fn error(target: &str, message: impl AsRef<str>) {
    let message = message.as_ref();
    push_error_entry(target, message);
    logger().write(Level::Error, target, message);
}

/// Records a native library failure without presenting it as an application
/// error in the TUI. Native backends often report a recoverable hardware
/// failure before Concord falls back to another backend.
#[cfg(any(feature = "stream-broadcast", test))]
pub fn file_error(target: &str, message: impl AsRef<str>) {
    logger().write(Level::Error, target, message.as_ref());
}

pub fn error_entries() -> Vec<ErrorLogEntry> {
    error_log()
        .lock()
        .map(|entries| entries.iter().cloned().collect())
        .unwrap_or_default()
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
    paths::log_file()
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

#[cfg(any(all(target_os = "linux", feature = "stream-broadcast"), test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeStderrLevel {
    Debug,
    Error,
}

#[cfg(any(all(target_os = "linux", feature = "stream-broadcast"), test))]
fn classify_native_stderr(message: &str) -> NativeStderrLevel {
    let message = message.trim_start().to_ascii_lowercase();
    let informational_prefixes = [
        "libva info:",
        "info:",
        "debug:",
        "trace:",
        "warning:",
        "warn:",
        "[info]",
        "[debug]",
        "[trace]",
        "[warning]",
        "[warn]",
    ];

    if informational_prefixes
        .iter()
        .any(|prefix| message.starts_with(prefix))
    {
        NativeStderrLevel::Debug
    } else {
        // stderr has no standard severity metadata. Unknown output stays in the
        // normal log so a native failure is not silently discarded.
        NativeStderrLevel::Error
    }
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
fn record_native_stderr(message: &str) {
    let message = message.trim_end_matches(['\r', '\n']);
    if message.is_empty() {
        return;
    }

    match classify_native_stderr(message) {
        NativeStderrLevel::Debug => debug(NATIVE_STDERR_TARGET, message),
        NativeStderrLevel::Error => file_error(NATIVE_STDERR_TARGET, message),
    }
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
pub struct StderrCaptureGuard {
    original_stderr: OwnedFd,
}

#[cfg(not(all(target_os = "linux", feature = "stream-broadcast", not(test))))]
pub struct StderrCaptureGuard;

/// Redirects process stderr away from Ratatui and into Concord's file logger.
///
/// The Linux stream-broadcast build includes native media libraries that can
/// write directly to file descriptor 2. Other builds use a no-op guard.
pub fn capture_stderr() -> std::io::Result<StderrCaptureGuard> {
    #[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
    {
        StderrCaptureGuard::install()
    }

    #[cfg(not(all(target_os = "linux", feature = "stream-broadcast", not(test))))]
    {
        Ok(StderrCaptureGuard)
    }
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
impl StderrCaptureGuard {
    fn install() -> std::io::Result<Self> {
        prepare_native_log_file()?;

        let mut pipe_fds = [-1; 2];
        if unsafe { libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: pipe2 initialized both descriptors and ownership is moved
        // into OwnedFd exactly once.
        let read_fd = unsafe { OwnedFd::from_raw_fd(pipe_fds[0]) };
        // SAFETY: Same ownership argument as read_fd.
        let write_fd = unsafe { OwnedFd::from_raw_fd(pipe_fds[1]) };
        let original_stderr = unsafe {
            libc::fcntl(
                libc::STDERR_FILENO,
                libc::F_DUPFD_CLOEXEC,
                libc::STDERR_FILENO + 1,
            )
        };
        if original_stderr == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: fcntl returned a new owned descriptor.
        let original_stderr = unsafe { OwnedFd::from_raw_fd(original_stderr) };

        let reader = thread::Builder::new()
            .name("native-stderr-log".to_owned())
            .spawn(move || drain_native_stderr(read_fd))?;

        if unsafe { libc::dup2(write_fd.as_raw_fd(), libc::STDERR_FILENO) } == -1 {
            let error = std::io::Error::last_os_error();
            drop(write_fd);
            let _ = reader.join();
            return Err(error);
        }
        drop(write_fd);

        // The reader intentionally stays detached. A launched browser or other
        // child can inherit stderr and keep the pipe open after Concord starts
        // shutting down, so joining here could prevent the process from
        // exiting. The operating system ends the reader with the process.
        drop(reader);

        Ok(Self { original_stderr })
    }
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
impl Drop for StderrCaptureGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::dup2(self.original_stderr.as_raw_fd(), libc::STDERR_FILENO) };
    }
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
fn prepare_native_log_file() -> std::io::Result<()> {
    let path = logger().path.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Concord log path is unavailable",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
fn drain_native_stderr(read_fd: OwnedFd) {
    let mut reader = BufReader::new(File::from(read_fd));
    let mut bytes = Vec::new();

    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => record_native_stderr(&String::from_utf8_lossy(&bytes)),
            Err(error) => {
                file_error(
                    NATIVE_STDERR_TARGET,
                    format!("native stderr capture failed: {error}"),
                );
                break;
            }
        }
    }
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

    use super::{
        FileLogger, Level, NativeStderrLevel, classify_native_stderr, error, error_entries,
        error_log, file_error,
    };

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

    #[test]
    fn native_stderr_levels_follow_debug_and_error_policy() {
        for (message, expected) in [
            (
                "libva info: VA-API version 1.23.0",
                NativeStderrLevel::Debug,
            ),
            (
                "warning: optional encoder unavailable",
                NativeStderrLevel::Debug,
            ),
            (
                "libva error: driver initialization failed",
                NativeStderrLevel::Error,
            ),
            ("unclassified native failure", NativeStderrLevel::Error),
        ] {
            assert_eq!(classify_native_stderr(message), expected, "{message}");
        }

        let normal_logger = FileLogger {
            path: None,
            debug_enabled: false,
        };
        assert!(!normal_logger.should_write(Level::Debug));
        assert!(normal_logger.should_write(Level::Error));
    }

    #[test]
    fn file_only_native_error_does_not_enter_tui_error_log() {
        let _guard = test_lock().lock().expect("logging test mutex");
        clear_error_log();

        file_error("native-stderr", "libva error: driver initialization failed");

        assert!(error_entries().is_empty());
    }
}
