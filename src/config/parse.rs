//! Tolerant config parsing: invalid values are dropped one field (or one
//! keybinding) at a time with a warning instead of discarding the file.

use crate::Result;

use super::{
    AppOptions, BorderShape, BorderSurface, HighlightGroup, HighlightLinkOptions,
    KeymapFileOptions, KeymapOptions, ThemeOptions, UiStateOptions,
};

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
    let mut parser = ThemeLeafParser::default();
    for (section, value) in &root {
        match section.as_str() {
            "highlight" => {
                if let Some(table) = value.as_table() {
                    parser.parse_highlights(table);
                } else {
                    parser
                        .warnings
                        .push("[highlight] must be a table and was ignored".to_owned());
                }
            }
            "ui" => {
                if let Some(table) = value.as_table() {
                    parser.parse_ui(table);
                } else {
                    parser
                        .warnings
                        .push("[ui] must be a table and was ignored".to_owned());
                }
            }
            _ => parser
                .warnings
                .push(format!("[{section}] is unknown and was ignored")),
        }
    }
    Ok((parser.options, parser.warnings))
}

#[derive(Default)]
struct ThemeLeafParser {
    options: ThemeOptions,
    warnings: Vec<String>,
}

impl ThemeLeafParser {
    fn parse_ui(&mut self, table: &toml::Table) {
        for (field, value) in table {
            if field != "border" {
                self.warnings
                    .push(format!("[ui] {field} is unknown and was ignored"));
                continue;
            }
            let Some(fields) = value.as_table() else {
                self.warnings
                    .push("[ui.border] must be a table and was ignored".to_owned());
                continue;
            };
            self.parse_border_shapes(fields);
        }
    }

    fn parse_border_shapes(&mut self, fields: &toml::Table) {
        for (field, value) in fields {
            let surface = if field == "default" {
                None
            } else {
                match BorderSurface::from_name(field) {
                    Some(surface) => Some(surface),
                    None => {
                        self.warnings
                            .push(format!("[ui.border] {field} is unknown and was ignored"));
                        continue;
                    }
                }
            };
            let Some(raw) = value.as_str() else {
                self.warnings.push(format!(
                    "[ui.border] {field} must be a string and was ignored"
                ));
                continue;
            };
            let Some(shape) = BorderShape::from_name(raw) else {
                self.warnings.push(format!(
                    "[ui.border] {field} = \"{raw}\" is not a supported border shape and was ignored"
                ));
                continue;
            };
            match surface {
                Some(surface) => self.options.border_shapes_mut().set(surface, shape),
                None => {
                    self.options.border_shapes_mut().default = Some(shape);
                }
            }
        }
    }

    fn parse_highlights(&mut self, table: &toml::Table) {
        for (name, value) in table {
            let Some(group) = HighlightGroup::from_name(name) else {
                self.warnings
                    .push(format!("[highlight] {name} is unknown and was ignored"));
                continue;
            };
            let Some(fields) = value.as_table() else {
                self.warnings.push(format!(
                    "[highlight.{name}] must be a table and was ignored"
                ));
                continue;
            };
            self.parse_highlight_fields(group, fields);
        }
    }

    fn parse_highlight_fields(&mut self, group: HighlightGroup, fields: &toml::Table) {
        for (field, value) in fields {
            match field.as_str() {
                "link" => match value.as_str() {
                    Some("none") => {
                        self.options.highlight_mut(group).link =
                            Some(HighlightLinkOptions::Detached);
                    }
                    Some(name) => match HighlightGroup::from_name(name) {
                        Some(link) => {
                            self.options.highlight_mut(group).link =
                                Some(HighlightLinkOptions::Inherit(link));
                        }
                        None => self.warnings.push(format!(
                            "[highlight.{}] link references unknown group {name} and was ignored",
                            group.name()
                        )),
                    },
                    None => self.highlight_type_warning(group, field, "a string"),
                },
                "foreground" => match value.as_str() {
                    Some(raw) => {
                        self.options.highlight_mut(group).foreground = Some(raw.to_owned());
                    }
                    None => self.highlight_type_warning(group, field, "a string"),
                },
                "background" => match value.as_str() {
                    Some(raw) => {
                        self.options.highlight_mut(group).background = Some(raw.to_owned());
                    }
                    None => self.highlight_type_warning(group, field, "a string"),
                },
                "bold" => match value.as_bool() {
                    Some(enabled) => self.options.highlight_mut(group).bold = Some(enabled),
                    None => self.highlight_type_warning(group, field, "a boolean"),
                },
                "italic" => match value.as_bool() {
                    Some(enabled) => self.options.highlight_mut(group).italic = Some(enabled),
                    None => self.highlight_type_warning(group, field, "a boolean"),
                },
                "dim" => match value.as_bool() {
                    Some(enabled) => self.options.highlight_mut(group).dim = Some(enabled),
                    None => self.highlight_type_warning(group, field, "a boolean"),
                },
                "underline" => match value.as_bool() {
                    Some(enabled) => self.options.highlight_mut(group).underline = Some(enabled),
                    None => self.highlight_type_warning(group, field, "a boolean"),
                },
                "strikethrough" => match value.as_bool() {
                    Some(enabled) => {
                        self.options.highlight_mut(group).strikethrough = Some(enabled);
                    }
                    None => self.highlight_type_warning(group, field, "a boolean"),
                },
                _ => self.warnings.push(format!(
                    "[highlight.{}] {field} is unknown and was ignored",
                    group.name()
                )),
            }
        }
    }

    fn highlight_type_warning(&mut self, group: HighlightGroup, field: &str, expected: &str) {
        self.warnings.push(format!(
            "[highlight.{}] {field} must be {expected} and was ignored",
            group.name()
        ));
    }
}

/// Named map fields of `KeymapOptions`. Any other `[keymap]` key flattens into
/// `mappings` and is validated as a top-level binding instead. A test keeps this
/// in sync with the struct's named `BTreeMap` fields.
pub(super) const KEYMAP_ACTION_MAPS: [&str; 8] = [
    "groups",
    "guild_actions",
    "channel_actions",
    "message_actions",
    "member_actions",
    "thread_actions",
    "notification_inbox_actions",
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
