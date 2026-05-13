use std::path::PathBuf;

const APP_DIR: &str = "concord";
const CONFIG_FILE: &str = "config.toml";
const CREDENTIAL_FILE: &str = "credential";
const LOG_FILE: &str = "concord.log";

/// Root directory for all concord-managed files (config, credential, log).
pub fn app_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_DIR))
}

pub fn config_file() -> Option<PathBuf> {
    Some(app_dir()?.join(CONFIG_FILE))
}

pub fn credential_file() -> Option<PathBuf> {
    Some(app_dir()?.join(CREDENTIAL_FILE))
}

pub fn log_file() -> Option<PathBuf> {
    Some(app_dir()?.join(LOG_FILE))
}

pub fn download_dir() -> Option<PathBuf> {
    dirs::download_dir().or_else(|| Some(dirs::home_dir()?.join("Downloads")))
}
