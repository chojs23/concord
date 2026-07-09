//! Tolerant config parsing: invalid values are dropped one field (or one
//! keybinding) at a time with a warning instead of discarding the file.

use crate::Result;

use super::{AppOptions, KeymapFileOptions, KeymapOptions, ThemeOptions, UiStateOptions};

/// Parse `config.toml` tolerantly: a value with a wrong type or unknown variant
/// is skipped (its field falls back to default) instead of discarding the whole
/// file. Only real syntax errors fail.
pub(super) fn parse_app_options(content: &str) -> Result<(AppOptions, Vec<String>)> {
    let root: toml::Table = toml::from_str(content)?;
    let mut warnings = Vec::new();

    let options = AppOptions {
        display: section(&root, "display", &mut warnings),
        composer: section(&root, "composer", &mut warnings),
        credentials: section(&root, "credentials", &mut warnings),
        notifications: section(&root, "notifications", &mut warnings),
        voice: section(&root, "voice", &mut warnings),
        presence: section(&root, "presence", &mut warnings),
    };

    Ok((options, warnings))
}

fn one_entry(key: &str, value: toml::Value) -> toml::Table {
    let mut table = toml::Table::new();
    table.insert(key.to_owned(), value);
    table
}

fn section<T>(root: &toml::Table, name: &str, warnings: &mut Vec<String>) -> T
where
    T: serde::de::DeserializeOwned + Default,
{
    let Some(value) = root.get(name) else {
        return T::default();
    };
    let Some(table) = value.as_table() else {
        warnings.push(format!("[{name}] must be a table, using defaults"));
        return T::default();
    };

    let mut clean = toml::Table::new();
    for (key, value) in table {
        let probed: std::result::Result<T, _> =
            toml::Value::Table(one_entry(key, value.clone())).try_into();
        match probed {
            Ok(_) => {
                clean.insert(key.clone(), value.clone());
            }
            Err(error) => {
                warnings.push(format!(
                    "[{name}] {key} is invalid and was ignored: {error}"
                ));
            }
        }
    }

    match toml::Value::Table(clean).try_into() {
        Ok(options) => options,
        Err(error) => {
            warnings.push(format!(
                "[{name}] could not be applied, using defaults: {error}"
            ));
            T::default()
        }
    }
}

pub(super) fn parse_ui_state_options(content: &str) -> Result<(UiStateOptions, Vec<String>)> {
    let root: toml::Table = toml::from_str(content)?;
    let mut warnings = Vec::new();
    let ui_state = section(&root, "ui_state", &mut warnings);
    Ok((ui_state, warnings))
}

pub(super) fn parse_theme_options(content: &str) -> Result<(ThemeOptions, Vec<String>)> {
    let root: toml::Table = toml::from_str(content)?;
    let mut warnings = Vec::new();
    let theme = section(&root, "theme", &mut warnings);
    Ok((theme, warnings))
}

/// Named map fields of `KeymapOptions`. Any other `[keymap]` key flattens into
/// `mappings` and is validated as a top-level binding instead. A test keeps this
/// in sync with the struct's named `BTreeMap` fields.
pub(super) const KEYMAP_ACTION_MAPS: [&str; 7] = [
    "groups",
    "guild_actions",
    "channel_actions",
    "message_actions",
    "member_actions",
    "thread_actions",
    "composer",
];

fn keymap_accepts(keymap: toml::Table) -> bool {
    toml::Value::Table(keymap)
        .try_into::<KeymapOptions>()
        .is_ok()
}

/// Parse `keymap.toml` tolerantly, one keybinding at a time: a bad binding (at
/// the top level or inside an action map like `[keymap.guild_actions]`) is
/// dropped on its own. Only real syntax errors fail.
pub(super) fn parse_keymap_options(content: &str) -> Result<(KeymapOptions, Vec<String>)> {
    let root: toml::Table = toml::from_str(content)?;
    let mut warnings = Vec::new();

    let Some(keymap) = root.get("keymap") else {
        return Ok((KeymapOptions::default(), Vec::new()));
    };
    let Some(table) = keymap.as_table() else {
        warnings.push("[keymap] must be a table, using defaults".to_owned());
        return Ok((KeymapOptions::default(), warnings));
    };

    let mut clean = toml::Table::new();
    for (key, value) in table {
        if KEYMAP_ACTION_MAPS.contains(&key.as_str()) {
            let Some(bindings) = value.as_table() else {
                warnings.push(format!("[keymap.{key}] must be a table, using defaults"));
                continue;
            };
            let mut clean_bindings = toml::Table::new();
            for (name, binding) in bindings {
                let probe = one_entry(key, toml::Value::Table(one_entry(name, binding.clone())));
                if keymap_accepts(probe) {
                    clean_bindings.insert(name.clone(), binding.clone());
                } else {
                    warnings.push(format!("[keymap.{key}] {name} is invalid and was ignored"));
                }
            }
            clean.insert(key.clone(), toml::Value::Table(clean_bindings));
        } else if keymap_accepts(one_entry(key, value.clone())) {
            clean.insert(key.clone(), value.clone());
        } else {
            warnings.push(format!("[keymap] {key} is invalid and was ignored"));
        }
    }

    let file = one_entry("keymap", toml::Value::Table(clean));
    let keymap = match toml::Value::Table(file).try_into::<KeymapFileOptions>() {
        Ok(file) => file.keymap,
        Err(error) => {
            warnings.push(format!(
                "[keymap] could not be applied, using defaults: {error}"
            ));
            KeymapOptions::default()
        }
    };
    Ok((keymap, warnings))
}
