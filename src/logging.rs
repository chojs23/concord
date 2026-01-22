use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

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
    logger().write(Level::Error, target, message.as_ref());
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
    format!("{timestamp_millis} [{}] {target}: {message}", level.label())
}

#[cfg(test)]
mod tests {
    use super::{Level, flag_enabled, format_log_line};

    #[test]
    fn parses_enabled_flags() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            assert!(flag_enabled(value));
        }
        for value in ["", "0", "false", "off", "no"] {
            assert!(!flag_enabled(value));
        }
    }

    #[test]
    fn formats_log_line_with_level_and_target() {
        assert_eq!(
            format_log_line(42, Level::Error, "gateway", "boom"),
            "42 [ERROR] gateway: boom"
        );
    }
}
