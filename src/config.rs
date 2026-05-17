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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceOptions {
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub microphone_sensitivity: MicrophoneSensitivityPreset,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppOptions {
    pub display: DisplayOptions,
    pub voice: VoiceOptions,
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MicrophoneSensitivityPreset {
    Off,
    Low,
    #[default]
    Medium,
    High,
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

impl MicrophoneSensitivityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Off,
        }
    }

    pub fn peak_threshold(self) -> i16 {
        match self {
            Self::Off => 0,
            Self::Low => 3500,
            Self::Medium => 1500,
            Self::High => 500,
        }
    }
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            self_mute: false,
            self_deaf: false,
            allow_microphone_transmit: false,
            microphone_sensitivity: MicrophoneSensitivityPreset::default(),
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

pub fn load_options() -> Result<AppOptions> {
    let path = config_path()?;
    load_options_from_path(&path)
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

fn load_options_from_path(path: &Path) -> Result<AppOptions> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(toml::from_str::<AppOptions>(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AppOptions::default()),
        Err(error) => Err(error.into()),
    }
}

pub fn save_options(options: &AppOptions) -> Result<()> {
    let path = config_path()?;
    save_options_to_path(&path, options)
}

fn save_options_to_path(path: &Path, options: &AppOptions) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir_permissions(parent)?;
    }

    write_private_file(path, &toml::to_string_pretty(options)?)
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
        AppOptions, DisplayOptions, ImagePreviewQualityPreset, MicrophoneSensitivityPreset,
        VoiceOptions,
        load_options_from_path, save_options_to_path,
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
    fn app_config_parses_partial_toml_with_defaults() {
        let cases = [
            (
                "[display]\ndisable_image_preview = true\n",
                true,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityPreset::Medium,
            ),
            (
                "[display]\nimage_preview_quality = \"original\"\n",
                false,
                ImagePreviewQualityPreset::Original,
                false,
                false,
                false,
                MicrophoneSensitivityPreset::Medium,
            ),
            (
                "[voice]\nself_mute = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                true,
                false,
                false,
                MicrophoneSensitivityPreset::Medium,
            ),
            (
                "[voice]\nallow_microphone_transmit = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                true,
                MicrophoneSensitivityPreset::Medium,
            ),
            (
                "[voice]\nmicrophone_sensitivity = \"high\"\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityPreset::High,
            ),
        ];

        for (
            toml,
            disable_image_preview,
            image_preview_quality,
            self_mute,
            self_deaf,
            allow_microphone_transmit,
            microphone_sensitivity,
        ) in cases
        {
            let config: AppOptions = toml::from_str(toml).expect("partial config should parse");
            assert_eq!(config.display.disable_image_preview, disable_image_preview);
            assert!(config.display.show_avatars);
            assert!(config.display.show_images);
            assert_eq!(config.display.image_preview_quality, image_preview_quality);
            assert!(config.display.show_custom_emoji);
            assert!(config.display.desktop_notifications);
            assert_eq!(config.voice.self_mute, self_mute);
            assert_eq!(config.voice.self_deaf, self_deaf);
            assert_eq!(
                config.voice.allow_microphone_transmit,
                allow_microphone_transmit
            );
            assert_eq!(config.voice.microphone_sensitivity, microphone_sensitivity);
            assert_eq!(config.display.server_width, 20);
            assert_eq!(config.display.channel_list_width, 24);
            assert_eq!(config.display.member_list_width, 26);
        }
    }

    #[test]
    fn options_save_and_load_round_trip() {
        let path = test_config_path();
        let options = AppOptions {
            display: DisplayOptions {
                disable_image_preview: true,
                show_avatars: false,
                show_images: false,
                image_preview_quality: ImagePreviewQualityPreset::Original,
                show_custom_emoji: false,
                desktop_notifications: false,
                server_width: 12,
                channel_list_width: 30,
                member_list_width: 18,
            },
            voice: VoiceOptions {
                self_mute: true,
                self_deaf: true,
                allow_microphone_transmit: true,
                microphone_sensitivity: MicrophoneSensitivityPreset::Low,
            },
        };

        save_options_to_path(&path, &options).expect("config should save");
        let loaded = load_options_from_path(&path).expect("config should load");

        assert_eq!(loaded, options);
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
