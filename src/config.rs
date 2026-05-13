use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{Result, paths};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayOptions {
    pub disable_image_preview: bool,
    pub show_avatars: bool,
    pub show_images: bool,
    pub image_preview_quality: ImagePreviewQualityPreset,
    pub show_custom_emoji: bool,
    pub desktop_notifications: bool,
    pub server_width: u16,
    pub channel_list_width: u16,
    pub member_list_width: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImagePreviewQualityPreset {
    Efficient,
    #[default]
    Balanced,
    High,
    Original,
}

impl ImagePreviewQualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Efficient => "efficient",
            Self::Balanced => "balanced",
            Self::High => "high",
            Self::Original => "original",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Efficient => Self::Balanced,
            Self::Balanced => Self::High,
            Self::High => Self::Original,
            Self::Original => Self::Efficient,
        }
    }
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            disable_image_preview: false,
            show_avatars: true,
            show_images: true,
            image_preview_quality: ImagePreviewQualityPreset::default(),
            show_custom_emoji: true,
            desktop_notifications: true,
            server_width: 20,
            channel_list_width: 24,
            member_list_width: 26,
        }
    }
}

impl DisplayOptions {
    pub fn avatars_visible(self) -> bool {
        !self.disable_image_preview && self.show_avatars
    }

    pub fn images_visible(self) -> bool {
        !self.disable_image_preview && self.show_images
    }

    pub fn custom_emoji_visible(self) -> bool {
        !self.disable_image_preview && self.show_custom_emoji
    }
}

/// Raw key-binding strings from the config file, e.g. `"ctrl+e"`.
///
/// Each field accepts a key spec of the form `[modifier+]key` where modifier
/// is one of `ctrl`, `alt`, `shift` and key is a single character, `space`,
/// or a special name (`enter`, `esc`, `tab`, `f1`–`f12`, `pageup`, etc.).
/// Unrecognised specs silently fall back to the built-in default.
///
/// Example `~/.config/concord/config.toml`:
/// ```toml
/// [keybindings]
/// move_down       = "ctrl+n"
/// move_up         = "ctrl+p"
/// open_composer   = "a"
/// ```
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyBindingsConfig {
    /// Open the current composer buffer in `$EDITOR`. Default: `ctrl+e`.
    pub open_in_editor: String,
    /// Quit the application. Default: `q`.
    pub quit: String,
    /// Open the message composer. Default: `i`.
    pub open_composer: String,
    /// Open the leader menu. Default: `space`.
    pub open_leader: String,
    /// Open the keybinding help popup. Default: `?`.
    pub open_keymap: String,
    /// Open the pane search/filter bar. Default: `/`.
    pub pane_search: String,
    /// Move selection down. Arrow-down always works as an alternate. Default: `j`.
    pub move_down: String,
    /// Move selection up. Arrow-up always works as an alternate. Default: `k`.
    pub move_up: String,
    /// Jump to the top of the list. Default: `g`.
    pub jump_top: String,
    /// Jump to the bottom of the list. Default: `shift+g` (G).
    pub jump_bottom: String,
    /// Scroll down by half a page. PageDown always works as an alternate. Default: `ctrl+d`.
    pub half_page_down: String,
    /// Scroll up by half a page. PageUp always works as an alternate. Default: `ctrl+u`.
    pub half_page_up: String,
    /// Scroll the message viewport down (Messages pane only). Default: `shift+j` (J).
    pub scroll_viewport_down: String,
    /// Scroll the message viewport up (Messages pane only). Default: `shift+k` (K).
    pub scroll_viewport_up: String,
    /// Scroll the focused pane horizontally left. Default: `shift+h` (H).
    pub scroll_pane_left: String,
    /// Scroll the focused pane horizontally right. Default: `shift+l` (L).
    pub scroll_pane_right: String,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        Self {
            open_in_editor: "ctrl+e".to_owned(),
            quit: "q".to_owned(),
            open_composer: "i".to_owned(),
            open_leader: "space".to_owned(),
            open_keymap: "?".to_owned(),
            pane_search: "/".to_owned(),
            move_down: "j".to_owned(),
            move_up: "k".to_owned(),
            jump_top: "g".to_owned(),
            jump_bottom: "shift+g".to_owned(),
            half_page_down: "ctrl+d".to_owned(),
            half_page_up: "ctrl+u".to_owned(),
            scroll_viewport_down: "shift+j".to_owned(),
            scroll_viewport_up: "shift+k".to_owned(),
            scroll_pane_left: "shift+h".to_owned(),
            scroll_pane_right: "shift+l".to_owned(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct AppConfig {
    display: DisplayOptions,
    keybindings: KeyBindingsConfig,
}

pub fn load_display_options() -> Result<DisplayOptions> {
    let path = config_path()?;
    load_display_options_from_path(&path)
}

pub fn load_key_bindings_config() -> Result<KeyBindingsConfig> {
    let path = config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(toml::from_str::<AppConfig>(&content)?.keybindings),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(KeyBindingsConfig::default())
        }
        Err(error) => Err(error.into()),
    }
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

fn load_display_options_from_path(path: &Path) -> Result<DisplayOptions> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(toml::from_str::<AppConfig>(&content)?.display),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DisplayOptions::default()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_display_options(options: &DisplayOptions) -> Result<()> {
    let path = config_path()?;
    save_display_options_to_path(&path, options)
}

fn save_display_options_to_path(path: &Path, options: &DisplayOptions) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir_permissions(parent)?;
    }

    // Load the existing config so other sections (e.g. [keybindings]) are
    // preserved. Fall back to defaults when the file is missing or unparseable.
    let mut config = match fs::read_to_string(path) {
        Ok(content) => toml::from_str::<AppConfig>(&content).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    };
    config.display = *options;
    write_private_file(path, &toml::to_string_pretty(&config)?)
}

fn config_path() -> Result<PathBuf> {
    paths::config_file().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not resolve user config directory",
        )
        .into()
    })
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_private_file(path: &Path, content: &str) -> Result<()> {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())?;

    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        AppConfig, DisplayOptions, ImagePreviewQualityPreset, KeyBindingsConfig,
        load_display_options_from_path, save_display_options_to_path,
    };

    #[test]
    fn display_options_default_to_all_media_enabled() {
        let options = DisplayOptions::default();

        assert!(options.avatars_visible());
        assert!(options.images_visible());
        assert!(options.custom_emoji_visible());
        assert_eq!(
            options.image_preview_quality,
            ImagePreviewQualityPreset::Balanced
        );
    }

    #[test]
    fn global_disable_overrides_individual_toggles() {
        let options = DisplayOptions {
            disable_image_preview: true,
            show_avatars: true,
            show_images: true,
            image_preview_quality: ImagePreviewQualityPreset::Balanced,
            show_custom_emoji: true,
            desktop_notifications: true,
            server_width: 20,
            channel_list_width: 24,
            member_list_width: 26,
        };

        assert!(!options.avatars_visible());
        assert!(!options.images_visible());
        assert!(!options.custom_emoji_visible());
    }

    #[test]
    fn display_config_parses_partial_toml_with_defaults() {
        let cases = [
            (
                "[display]\ndisable_image_preview = true\n",
                true,
                ImagePreviewQualityPreset::Balanced,
            ),
            (
                "[display]\nimage_preview_quality = \"original\"\n",
                false,
                ImagePreviewQualityPreset::Original,
            ),
        ];

        for (toml, disable_image_preview, image_preview_quality) in cases {
            let config: AppConfig = toml::from_str(toml).expect("partial config should parse");
            assert_eq!(config.display.disable_image_preview, disable_image_preview);
            assert!(config.display.show_avatars);
            assert!(config.display.show_images);
            assert_eq!(config.display.image_preview_quality, image_preview_quality);
            assert!(config.display.show_custom_emoji);
            assert!(config.display.desktop_notifications);
            assert_eq!(config.display.server_width, 20);
            assert_eq!(config.display.channel_list_width, 24);
            assert_eq!(config.display.member_list_width, 26);
        }
    }

    #[test]
    fn display_options_save_and_load_round_trip() {
        let path = test_config_path();
        let options = DisplayOptions {
            disable_image_preview: true,
            show_avatars: false,
            show_images: false,
            image_preview_quality: ImagePreviewQualityPreset::Original,
            show_custom_emoji: false,
            desktop_notifications: false,
            server_width: 12,
            channel_list_width: 30,
            member_list_width: 18,
        };

        save_display_options_to_path(&path, &options).expect("config should save");
        let loaded = load_display_options_from_path(&path).expect("config should load");

        assert_eq!(loaded, options);
        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn keybindings_default_to_ctrl_e() {
        let config = KeyBindingsConfig::default();
        assert_eq!(config.open_in_editor, "ctrl+e");
    }

    #[test]
    fn keybindings_config_parses_from_toml() {
        let toml = "[keybindings]\nopen_in_editor = \"ctrl+o\"\n";
        let config: AppConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(config.keybindings.open_in_editor, "ctrl+o");
    }

    #[test]
    fn keybindings_config_defaults_when_section_absent() {
        let toml = "[display]\ndisable_image_preview = true\n";
        let config: AppConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(config.keybindings.open_in_editor, "ctrl+e");
    }

    #[test]
    fn save_display_options_preserves_keybindings() {
        let path = test_config_path();

        // Write a config that has a custom keybinding.
        let initial = "[keybindings]\nopen_in_editor = \"alt+e\"\n";
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("should create dir");
        }
        fs::write(&path, initial).expect("should write initial config");

        // Save display options — must not clobber the keybinding.
        let options = DisplayOptions::default();
        save_display_options_to_path(&path, &options).expect("should save");

        let content = fs::read_to_string(&path).expect("should read back");
        let config: AppConfig = toml::from_str(&content).expect("should parse back");
        assert_eq!(
            config.keybindings.open_in_editor, "alt+e",
            "keybinding should survive a display-options save"
        );

        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    fn test_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("concord-config-test-{unique}"))
            .join("config.toml")
    }
}
