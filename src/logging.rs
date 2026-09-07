use std::{
    env,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
use std::{
    io::{BufRead, BufReader},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    thread,
};

use chrono::{DateTime, Utc};

use crate::paths;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

const MAX_LOG_TAIL_LINES: usize = 200;
const MAX_LOG_TAIL_BYTES: u64 = 256 * 1024;
#[cfg(all(target_os = "linux", feature = "stream-broadcast", not(test)))]
const NATIVE_STDERR_TARGET: &str = "native-stderr";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LogFileLine {
    pub(crate) offset: u64,
    pub(crate) text: String,
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
        self.write_to_file(level, target, message);
    }

    fn write_to_file(&self, level: Level, target: &str, message: &str) {
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
    logger().write(Level::Error, target, message.as_ref());
}

/// Reads the recent physical lines from the same file used by the logger.
///
/// The file is reopened for each snapshot so replacement and truncation are
/// visible. Reads stay bounded even when the configured path points at a large
/// file. Non-file paths are rejected before opening so a FIFO cannot block the
/// UI's background reader.
pub(crate) fn read_log_tail() -> std::io::Result<Vec<LogFileLine>> {
    read_log_tail_from_path(logger().path.as_deref())
}

fn logger() -> &'static FileLogger {
    LOGGER.get_or_init(FileLogger::from_env)
}

fn read_log_tail_from_path(path: Option<&Path>) -> std::io::Result<Vec<LogFileLine>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Concord log path is not a regular file",
        ));
    }

    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    let start = length.saturating_sub(MAX_LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;

    let mut bytes = Vec::with_capacity((length - start) as usize);
    file.take(MAX_LOG_TAIL_BYTES).read_to_end(&mut bytes)?;

    // A bounded tail can begin inside a UTF-8 scalar. Logger output is UTF-8,
    // so skipping continuation bytes preserves every complete scalar in range.
    let leading_continuations = bytes
        .iter()
        .take_while(|byte| (**byte & 0b1100_0000) == 0b1000_0000)
        .count();
    let mut bytes = &bytes[leading_continuations..];
    let mut content_start = start + leading_continuations as u64;

    // Select the retained line range before constructing Strings. This keeps a
    // newline-dense tail from allocating one temporary entry per byte.
    let line_count = bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'));
    let lines_to_skip = line_count.saturating_sub(MAX_LOG_TAIL_LINES);
    if lines_to_skip > 0 {
        let retained_start = bytes
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'\n')
            .nth(lines_to_skip - 1)
            .map_or(0, |(index, _)| index + 1);
        bytes = &bytes[retained_start..];
        content_start += retained_start as u64;
    }

    let lines = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .scan(content_start, |offset, line| {
            let line_offset = *offset;
            *offset += line.len() as u64;
            let text = line.strip_suffix(b"\n").unwrap_or(line);
            let text = text.strip_suffix(b"\r").unwrap_or(text);
            Some(LogFileLine {
                offset: line_offset,
                text: String::from_utf8_lossy(text).into_owned(),
            })
        })
        .collect::<Vec<_>>();
    debug_assert!(lines.len() <= MAX_LOG_TAIL_LINES);
    Ok(lines)
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
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        FileLogger, Level, MAX_LOG_TAIL_BYTES, MAX_LOG_TAIL_LINES, NativeStderrLevel,
        classify_native_stderr, read_log_tail_from_path,
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
    fn file_logger_and_tail_reader_share_debug_filtering_and_text() {
        let path = temp_path("write.log");
        let normal_logger = FileLogger {
            path: Some(path.clone()),
            debug_enabled: false,
        };
        normal_logger.write_to_file(Level::Debug, "gateway", "not recorded");
        normal_logger.write_to_file(Level::Error, "gateway", "first\nsecond");

        let debug_logger = FileLogger {
            path: Some(path.clone()),
            debug_enabled: true,
        };
        debug_logger.write_to_file(Level::Debug, "media", "decoded image");

        let lines = read_log_tail_from_path(Some(&path)).expect("read written log");
        assert_eq!(lines.len(), 3);
        assert!(lines[0].text.contains("[ERROR] gateway: first"));
        assert_eq!(lines[1].text, "second");
        assert!(lines[2].text.contains("[DEBUG] media: decoded image"));
        assert!(lines.windows(2).all(|pair| pair[0].offset < pair[1].offset));
        assert!(!lines.iter().any(|line| line.text.contains("not recorded")));

        remove_temp(&path);
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
        };
        assert!(!logger.should_write(Level::Debug));
        assert!(logger.should_write(Level::Error));
    }

    #[test]
    fn log_tail_is_bounded_by_line_count_and_bytes() {
        let line_path = temp_path("line-limit.log");
        let content = (0..MAX_LOG_TAIL_LINES + 5)
            .map(|index| format!("line {index}\n"))
            .collect::<String>();
        fs::write(&line_path, content).expect("write line-limited log");

        let lines = read_log_tail_from_path(Some(&line_path)).expect("read line-limited log");
        assert_eq!(lines.len(), MAX_LOG_TAIL_LINES);
        assert_eq!(lines[0].text, "line 5");
        assert_eq!(lines[MAX_LOG_TAIL_LINES - 1].text, "line 204");

        let byte_path = temp_path("byte-limit.log");
        let prefix = "discarded\n";
        let oversized = "x".repeat(MAX_LOG_TAIL_BYTES as usize);
        fs::write(&byte_path, format!("{prefix}{oversized}")).expect("write byte-limited log");

        let byte_lines = read_log_tail_from_path(Some(&byte_path)).expect("read byte-limited log");
        assert_eq!(byte_lines.len(), 1);
        assert_eq!(byte_lines[0].offset, prefix.len() as u64);
        assert_eq!(byte_lines[0].text.len(), MAX_LOG_TAIL_BYTES as usize);

        let dense_path = temp_path("dense-lines.log");
        fs::write(&dense_path, "\n".repeat(MAX_LOG_TAIL_BYTES as usize))
            .expect("write newline-dense log");
        let dense_lines =
            read_log_tail_from_path(Some(&dense_path)).expect("read newline-dense log");
        assert_eq!(dense_lines.len(), MAX_LOG_TAIL_LINES);
        assert!(dense_lines.iter().all(|line| line.text.is_empty()));

        remove_temp(&line_path);
        remove_temp(&byte_path);
        remove_temp(&dense_path);
    }

    #[test]
    fn log_tail_reopens_for_append_truncation_and_replacement() {
        let path = temp_path("updates.log");
        fs::write(&path, "one\ntwo").expect("write initial log");
        let initial = read_log_tail_from_path(Some(&path)).expect("read initial log");
        assert_eq!(initial[1].offset, 4);
        assert_eq!(initial[1].text, "two");

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open log for append");
        file.write_all(b" extended\nthree\n").expect("append log");
        drop(file);
        let appended = read_log_tail_from_path(Some(&path)).expect("read appended log");
        assert_eq!(appended[1].offset, initial[1].offset);
        assert_eq!(appended[1].text, "two extended");

        fs::write(&path, "short\n").expect("truncate log");
        let truncated = read_log_tail_from_path(Some(&path)).expect("read truncated log");
        assert_eq!(
            truncated,
            vec![super::LogFileLine {
                offset: 0,
                text: "short".to_owned()
            }]
        );

        let replacement = temp_path("replacement.log");
        fs::write(&replacement, "replacement\n").expect("write replacement log");
        fs::rename(&replacement, &path).expect("replace log");
        let replaced = read_log_tail_from_path(Some(&path)).expect("read replaced log");
        assert_eq!(replaced[0].offset, 0);
        assert_eq!(replaced[0].text, "replacement");

        remove_temp(&path);
    }

    #[test]
    fn log_tail_handles_utf8_cut_missing_and_non_file_paths() {
        let path = temp_path("utf8.log");
        let content = format!("{}끝", "가".repeat(MAX_LOG_TAIL_BYTES as usize / 3 + 2));
        fs::write(&path, content).expect("write UTF-8 log");
        let lines = read_log_tail_from_path(Some(&path)).expect("read UTF-8 tail");
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].text.starts_with('\u{fffd}'));
        assert!(lines[0].text.ends_with("끝"));

        let missing = temp_path("missing.log");
        assert!(
            read_log_tail_from_path(None)
                .expect("read unconfigured log")
                .is_empty()
        );
        assert!(
            read_log_tail_from_path(Some(&missing))
                .expect("read missing log")
                .is_empty()
        );

        let directory = temp_path("directory");
        fs::create_dir(&directory).expect("create non-file log path");
        let error = read_log_tail_from_path(Some(&directory)).expect_err("reject directory");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);

        remove_temp(&path);
        remove_temp(&directory);
    }
}
