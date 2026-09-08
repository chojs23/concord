use std::{
    collections::VecDeque,
    env,
    fs::OpenOptions,
    io::Write,
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

use chrono::{DateTime, Utc};

use crate::paths;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

const MAX_LOG_TAIL_LINES: usize = 200;
const MAX_LOG_TAIL_BYTES: usize = 256 * 1024;
#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
const NATIVE_STDERR_TARGET: &str = "native-stderr";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LogLine {
    pub(crate) id: u64,
    pub(crate) text: String,
}

#[derive(Debug, Default)]
struct LogTail {
    lines: VecDeque<LogLine>,
    bytes: usize,
    next_id: u64,
}

impl LogTail {
    fn push(&mut self, text: &str) {
        for text in text.split_terminator('\n') {
            let text = text.strip_suffix('\r').unwrap_or(text);
            // Keep a bounded suffix even for a single huge line, without
            // splitting a UTF-8 character. Retained lines never change IDs.
            let mut start = text.len().saturating_sub(MAX_LOG_TAIL_BYTES);
            while !text.is_char_boundary(start) {
                start += 1;
            }
            let text = text[start..].to_owned();
            self.bytes += text.len();
            self.lines.push_back(LogLine {
                id: self.next_id,
                text,
            });
            self.next_id += 1;
            while self.lines.len() > MAX_LOG_TAIL_LINES || self.bytes > MAX_LOG_TAIL_BYTES {
                let removed = self.lines.pop_front().expect("tail exceeds its bound");
                self.bytes -= removed.text.len();
            }
        }
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

#[derive(Debug, Default)]
#[cfg_attr(test, allow(dead_code))]
struct FileLogger {
    path: Option<PathBuf>,
    debug_enabled: bool,
    tail: Mutex<LogTail>,
}

impl FileLogger {
    fn from_env() -> Self {
        Self {
            path: log_path(),
            debug_enabled: debug_enabled(),
            ..Default::default()
        }
    }

    #[cfg(not(test))]
    fn write(&self, level: Level, target: &str, message: &str) {
        self.record(level, target, message);
    }

    fn record(&self, level: Level, target: &str, message: &str) {
        if !self.should_write(level) {
            return;
        }
        let mut line = format_log_line(unix_timestamp_millis(), level, target, message);
        line.push('\n');
        // Capture at the logging call, not by reading back a shared file.
        // Release the buffer lock before disk I/O so snapshots stay responsive.
        self.tail
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(&line);
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn recent_lines(&self) -> Vec<LogLine> {
        self.tail
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .iter()
            .cloned()
            .collect()
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
    logger().write(Level::Error, target, message.as_ref());
}

/// Snapshots the current process's recent log lines without accessing disk.
pub(crate) fn recent_log_lines() -> Vec<LogLine> {
    logger().recent_lines()
}

fn logger() -> &'static FileLogger {
    LOGGER.get_or_init(FileLogger::from_env)
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
        NativeStderrLevel::Error => error(NATIVE_STDERR_TARGET, message),
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
            Err(read_error) => {
                error(
                    NATIVE_STDERR_TARGET,
                    format!("native stderr capture failed: {read_error}"),
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
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        FileLogger, Level, LogTail, MAX_LOG_TAIL_BYTES, MAX_LOG_TAIL_LINES, NativeStderrLevel,
        classify_native_stderr,
    };

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    fn temp_path(name: &str) -> PathBuf {
        let unique = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "concord-logging-{}-{unique}-{name}",
            std::process::id()
        ))
    }

    fn remove_temp(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(path);
    }

    #[test]
    fn process_log_tail_captures_only_its_own_output_with_the_file_log_level_policy() {
        for debug_enabled in [false, true] {
            let path = temp_path("write.log");
            let previous = "previous process log\n";
            fs::write(&path, previous).expect("write previous process log");
            let logger = FileLogger {
                path: Some(path.clone()),
                debug_enabled,
                ..Default::default()
            };
            assert!(
                logger.recent_lines().is_empty(),
                "the current process starts without previous process logs"
            );
            logger.record(Level::Error, "gateway", "first\n\nsecond");
            logger.record(Level::Debug, "media", "decoded image");

            let lines = logger.recent_lines();
            assert_eq!(lines.len(), if debug_enabled { 4 } else { 3 });
            assert!(lines[0].text.contains("[ERROR] gateway: first"));
            assert_eq!(lines[1].text, "");
            assert_eq!(lines[2].text, "second");
            if debug_enabled {
                assert!(lines[3].text.contains("[DEBUG] media: decoded image"));
            }
            assert!(lines.windows(2).all(|pair| pair[0].id < pair[1].id));
            let current = lines
                .iter()
                .map(|line| format!("{}\n", line.text))
                .collect::<String>();
            assert_eq!(
                fs::read_to_string(&path).expect("read written log"),
                format!("{previous}{current}"),
                "file history is preserved and new output matches the panel"
            );

            let other_logger = FileLogger {
                path: Some(path.clone()),
                ..Default::default()
            };
            assert!(other_logger.recent_lines().is_empty());
            other_logger.record(Level::Error, "gateway", "another process");
            assert_eq!(other_logger.recent_lines().len(), 1);
            assert_eq!(
                logger.recent_lines(),
                lines,
                "other writers cannot enter the buffer"
            );
            remove_temp(&path);
        }
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

        let logger = FileLogger {
            path: None,
            debug_enabled: false,
            ..Default::default()
        };
        assert!(!logger.should_write(Level::Debug));
        assert!(logger.should_write(Level::Error));
    }

    #[test]
    fn log_tail_is_bounded_by_line_count_and_bytes() {
        let mut tail = LogTail::default();
        let content = (0..MAX_LOG_TAIL_LINES + 5)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        tail.push(&content);
        assert_eq!(tail.lines.len(), MAX_LOG_TAIL_LINES);
        assert_eq!(tail.lines[0].text, "line 5");
        assert_eq!(tail.lines[0].id, 5);
        assert_eq!(tail.lines[MAX_LOG_TAIL_LINES - 1].text, "line 204");
        tail.push("next\n");
        assert_eq!(tail.lines[0].id, 6, "eviction preserves line identity");

        let mut tail = LogTail::default();
        let half = "x".repeat(MAX_LOG_TAIL_BYTES / 2);
        tail.push(&format!("{half}\n{half}\n"));
        assert_eq!(tail.lines.len(), 2);
        tail.push("new\n");
        assert_eq!(tail.lines.len(), 2);
        assert_eq!(tail.lines[0].id, 1);
        assert!(tail.lines.iter().map(|line| line.text.len()).sum::<usize>() <= MAX_LOG_TAIL_BYTES);

        for oversized in [
            "x".repeat(MAX_LOG_TAIL_BYTES + 10),
            format!("{}끝", "가".repeat(MAX_LOG_TAIL_BYTES / 3 + 2)),
        ] {
            tail.push(&oversized);
            assert_eq!(tail.lines.len(), 1);
            let text = &tail.lines[0].text;
            assert!(text.len() <= MAX_LOG_TAIL_BYTES);
            assert!(text.len() >= MAX_LOG_TAIL_BYTES - 3);
            assert!(oversized.ends_with(text));
            assert!(!text.contains('\u{fffd}'));
        }

        tail.push(&"\n".repeat(MAX_LOG_TAIL_BYTES));
        assert_eq!(tail.lines.len(), MAX_LOG_TAIL_LINES);
        assert!(tail.lines.iter().all(|line| line.text.is_empty()));
    }

    #[test]
    fn process_log_tail_survives_file_changes_and_write_failures() {
        let path = temp_path("updates.log");
        let logger = FileLogger {
            path: Some(path.clone()),
            ..Default::default()
        };
        logger.record(Level::Error, "media", "current process");
        let initial = logger.recent_lines();
        fs::write(&path, "external file content\n").expect("overwrite log file");
        assert_eq!(logger.recent_lines(), initial);

        fs::remove_file(&path).expect("remove log file");
        fs::create_dir(&path).expect("block file writes with a directory");
        logger.record(Level::Error, "media", "still visible");
        let lines = logger.recent_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], initial[0]);
        assert!(lines[1].text.ends_with("still visible"));
        remove_temp(&path);

        let logger = FileLogger::default();
        logger.record(Level::Error, "media", "no log path");
        assert_eq!(logger.recent_lines().len(), 1);
    }
}
