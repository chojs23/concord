use std::{env, ffi::OsString, path::PathBuf};

const APP_DIR: &str = "concord";
const CONFIG_FILE: &str = "config.toml";
const CREDENTIAL_FILE: &str = "credential";
const LOG_FILE: &str = "concord.log";

/// Root directory for all concord-managed files (config, credential, log).
pub fn app_dir() -> Option<PathBuf> {
    Some(config_base_dir()?.join(APP_DIR))
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

fn config_base_dir() -> Option<PathBuf> {
    xdg_config_home_from_env(env::var_os("XDG_CONFIG_HOME")).or_else(dirs::config_dir)
}

fn xdg_config_home_from_env(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::xdg_config_home_from_env;

    #[test]
    fn xdg_config_home_accepts_absolute_paths() {
        let path = std::env::temp_dir().join("concord-xdg-config-home");

        assert_eq!(
            xdg_config_home_from_env(Some(path.clone().into())),
            Some(path)
        );
    }

    #[test]
    fn xdg_config_home_ignores_relative_paths() {
        assert_eq!(xdg_config_home_from_env(Some("relative/path".into())), None);
    }

    #[test]
    fn xdg_config_home_ignores_missing_values() {
        assert_eq!(xdg_config_home_from_env(None), None);
    }
}
