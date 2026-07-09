use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{Result, paths, support::private_file};

pub fn load_options() -> Result<AppOptions> {
    Ok(load_options_with_warnings()?.0)
}

pub fn load_options_with_warnings() -> Result<(AppOptions, Vec<String>)> {
    let path = config_path()?;
    load_options_from_path(&path)
}

pub fn load_keymap_options() -> Result<KeymapOptions> {
    Ok(load_keymap_options_with_warnings()?.0)
}

pub fn load_keymap_options_with_warnings() -> Result<(KeymapOptions, Vec<String>)> {
    let path = keymap_path()?;
    load_keymap_options_from_path(&path)
}

pub fn load_ui_state_options() -> Result<UiStateOptions> {
    Ok(load_ui_state_options_with_warnings()?.0)
}

pub fn load_ui_state_options_with_warnings() -> Result<(UiStateOptions, Vec<String>)> {
    let path = state_path()?;
    load_ui_state_options_from_path(&path)
}

pub fn load_theme_options() -> Result<ThemeOptions> {
    Ok(load_theme_options_with_warnings()?.0)
}

pub fn load_theme_options_with_warnings() -> Result<(ThemeOptions, Vec<String>)> {
    let path = theme_path()?;
    load_theme_options_from_path(&path)
}

/// User-facing description of where config lives, e.g. for help text. Falls
/// back to the legacy path string when XDG resolution fails so the message
/// stays readable.
pub fn config_path_display() -> String {
    config_path()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.config/concord/config.toml".to_owned())
}

fn load_options_from_path(path: &Path) -> Result<(AppOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_app_options)
}

fn load_tolerant_options_from_path<T>(
    path: &Path,
    parse: impl FnOnce(&str) -> Result<(T, Vec<String>)>,
) -> Result<(T, Vec<String>)>
where
    T: Default,
{
    match fs::read_to_string(path) {
        Ok(content) => parse(&content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((T::default(), Vec::new()))
        }
        Err(error) => Err(error.into()),
    }
}

fn load_keymap_options_from_path(path: &Path) -> Result<(KeymapOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_keymap_options)
}

fn load_ui_state_options_from_path(path: &Path) -> Result<(UiStateOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_ui_state_options)
}

fn load_theme_options_from_path(path: &Path) -> Result<(ThemeOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_theme_options)
}

pub fn save_options(options: &AppOptions) -> Result<()> {
    let path = config_path()?;
    save_options_to_path(&path, options)
}

pub fn save_ui_state_options(options: &UiStateOptions) -> Result<()> {
    let path = state_path()?;
    save_ui_state_options_to_path(&path, options)
}

fn save_options_to_path(path: &Path, options: &AppOptions) -> Result<()> {
    write_private_toml(path, options)
}

fn save_ui_state_options_to_path(path: &Path, options: &UiStateOptions) -> Result<()> {
    let file_options = UiStateFileOptions {
        ui_state: options.clone(),
    };
    write_private_toml(path, &file_options)
}

fn write_private_toml<T>(path: &Path, value: &T) -> Result<()>
where
    T: serde::Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        private_file::set_private_dir_permissions(parent)?;
    }

    private_file::write_private_file(path, &toml::to_string_pretty(value)?)
}

fn config_path() -> Result<PathBuf> {
    resolved_path(
        paths::config_file(),
        "could not resolve user config directory",
    )
}

fn keymap_path() -> Result<PathBuf> {
    resolved_path(
        paths::keymap_file(),
        "could not resolve user config directory",
    )
}

fn theme_path() -> Result<PathBuf> {
    resolved_path(
        paths::theme_file(),
        "could not resolve user config directory",
    )
}

fn state_path() -> Result<PathBuf> {
    resolved_path(
        paths::state_file(),
        "could not resolve user state directory",
    )
}

fn resolved_path(path: Option<PathBuf>, missing_message: &'static str) -> Result<PathBuf> {
    path.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, missing_message).into())
}

mod options;
mod parse;
#[cfg(test)]
mod tests;

pub use options::*;
use parse::{parse_app_options, parse_keymap_options, parse_theme_options, parse_ui_state_options};
