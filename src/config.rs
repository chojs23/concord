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
    pub server_width: u16,
    pub channel_list_width: u16,
    pub member_list_width: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationOptions {
    pub desktop_notifications: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceOptions {
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppOptions {
    pub display: DisplayOptions,
    pub notifications: NotificationOptions,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MicrophoneSensitivityDb(i8);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VoiceVolumePercent(u8);

const MIN_MICROPHONE_SENSITIVITY_DB: i8 = -100;
const MAX_MICROPHONE_SENSITIVITY_DB: i8 = 0;
const DEFAULT_MICROPHONE_SENSITIVITY_DB: i8 = -30;
const MIN_VOICE_VOLUME_PERCENT: u8 = 0;
const MAX_VOICE_VOLUME_PERCENT: u8 = 100;
const DEFAULT_VOICE_VOLUME_PERCENT: u8 = 100;

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

impl<'de> Deserialize<'de> for MicrophoneSensitivityDb {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw_db(i64::deserialize(deserializer)?))
    }
}

impl Default for MicrophoneSensitivityDb {
    fn default() -> Self {
        Self(DEFAULT_MICROPHONE_SENSITIVITY_DB)
    }
}

impl<'de> Deserialize<'de> for VoiceVolumePercent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw_percent(i64::deserialize(deserializer)?))
    }
}

impl Default for VoiceVolumePercent {
    fn default() -> Self {
        Self(DEFAULT_VOICE_VOLUME_PERCENT)
    }
}

impl MicrophoneSensitivityDb {
    pub fn new(value: i8) -> Self {
        Self::from_raw_db(i64::from(value))
    }

    fn from_raw_db(value: i64) -> Self {
        Self(value.clamp(
            i64::from(MIN_MICROPHONE_SENSITIVITY_DB),
            i64::from(MAX_MICROPHONE_SENSITIVITY_DB),
        ) as i8)
    }

    pub fn value(self) -> i8 {
        self.0
    }

    pub fn label(self) -> String {
        format!("{} dB", self.0)
    }

    pub fn adjust(self, delta: i8) -> Self {
        Self::new(self.0.saturating_add(delta))
    }

    pub fn peak_threshold(self) -> i32 {
        let ratio = 10_f64.powf(f64::from(self.0) / 20.0);
        (f64::from(i16::MAX) * ratio).round() as i32
    }
}

impl VoiceVolumePercent {
    pub fn new(value: u8) -> Self {
        Self(value.clamp(MIN_VOICE_VOLUME_PERCENT, MAX_VOICE_VOLUME_PERCENT))
    }

    fn from_raw_percent(value: i64) -> Self {
        Self(value.clamp(
            i64::from(MIN_VOICE_VOLUME_PERCENT),
            i64::from(MAX_VOICE_VOLUME_PERCENT),
        ) as u8)
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn label(self) -> String {
        format!("{}%", self.0)
    }

    pub fn adjust(self, delta: i8) -> Self {
        if delta.is_negative() {
            Self::new(self.0.saturating_sub(delta.unsigned_abs()))
        } else {
            Self::new(self.0.saturating_add(delta as u8))
        }
    }

    pub fn gain(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

impl Default for NotificationOptions {
    fn default() -> Self {
        Self {
            desktop_notifications: true,
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
        AppOptions, DisplayOptions, ImagePreviewQualityPreset, MicrophoneSensitivityDb,
        NotificationOptions, VoiceOptions, VoiceVolumePercent, load_options_from_path,
        save_options_to_path,
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
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[display]\nimage_preview_quality = \"original\"\n",
                false,
                ImagePreviewQualityPreset::Original,
                false,
                false,
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[voice]\nself_mute = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                true,
                false,
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[voice]\nallow_microphone_transmit = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                true,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[voice]\nmicrophone_sensitivity = -70\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityDb::new(-70),
            ),
            (
                "[voice]\nmicrophone_sensitivity = 10\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityDb::new(0),
            ),
            (
                "[voice]\nmicrophone_sensitivity = -500\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityDb::new(-100),
            ),
            (
                "[notifications]\ndesktop_notifications = false\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityDb::default(),
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
            let expected_desktop_notifications =
                !toml.contains("[notifications]\ndesktop_notifications = false");
            assert_eq!(
                config.notifications.desktop_notifications,
                expected_desktop_notifications
            );
            assert_eq!(config.voice.self_mute, self_mute);
            assert_eq!(config.voice.self_deaf, self_deaf);
            assert_eq!(
                config.voice.allow_microphone_transmit,
                allow_microphone_transmit
            );
            assert_eq!(config.voice.microphone_sensitivity, microphone_sensitivity);
            assert_eq!(
                config.voice.microphone_volume,
                VoiceVolumePercent::default()
            );
            assert_eq!(
                config.voice.voice_output_volume,
                VoiceVolumePercent::default()
            );
            assert_eq!(config.display.server_width, 20);
            assert_eq!(config.display.channel_list_width, 24);
            assert_eq!(config.display.member_list_width, 26);
        }
    }

    #[test]
    fn voice_volume_config_values_are_clamped() {
        let config: AppOptions =
            toml::from_str("[voice]\nmicrophone_volume = 150\nvoice_output_volume = -10\n")
                .expect("voice volume config should parse");

        assert_eq!(config.voice.microphone_volume, VoiceVolumePercent::new(100));
        assert_eq!(config.voice.voice_output_volume, VoiceVolumePercent::new(0));
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
                server_width: 12,
                channel_list_width: 30,
                member_list_width: 18,
            },
            notifications: NotificationOptions {
                desktop_notifications: false,
            },
            voice: VoiceOptions {
                self_mute: true,
                self_deaf: true,
                allow_microphone_transmit: true,
                microphone_sensitivity: MicrophoneSensitivityDb::new(-50),
                microphone_volume: VoiceVolumePercent::new(80),
                voice_output_volume: VoiceVolumePercent::new(60),
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
