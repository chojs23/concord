use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{Result, paths};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayOptions {
    pub disable_image_preview: bool,
    pub show_avatars: bool,
    pub show_images: bool,
    pub image_preview_quality: ImagePreviewQualityPreset,
    pub show_custom_emoji: bool,
    pub circular_avatars: bool,
    pub server_width: u16,
    pub channel_list_width: u16,
    pub member_list_width: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ComposerOptions {
    pub emojis_as_links: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationOptions {
    pub desktop_notifications: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_join_sound: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_leave_sound: Option<PathBuf>,
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct KeymapOptions {
    pub leader: Option<String>,
    pub groups: BTreeMap<String, String>,
    pub guild_actions: BTreeMap<String, KeymapBinding>,
    pub channel_actions: BTreeMap<String, KeymapBinding>,
    pub message_actions: BTreeMap<String, KeymapBinding>,
    pub member_actions: BTreeMap<String, KeymapBinding>,
    pub composer: BTreeMap<String, KeymapBinding>,
    #[serde(flatten)]
    pub mappings: BTreeMap<String, KeymapBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KeymapBinding {
    pub keys: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum KeymapBindingInput {
    Simple(String),
    Structured {
        keys: KeymapKeysInput,
        description: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum KeymapKeysInput {
    One(String),
    Many(Vec<String>),
}

impl KeymapBinding {
    pub fn one(key: impl Into<String>) -> Self {
        Self {
            keys: vec![key.into()],
            description: None,
        }
    }
}

impl<'de> Deserialize<'de> for KeymapBinding {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match KeymapBindingInput::deserialize(deserializer)? {
            KeymapBindingInput::Simple(key) => Ok(Self::one(key)),
            KeymapBindingInput::Structured { keys, description } => {
                let keys = match keys {
                    KeymapKeysInput::One(key) => vec![key],
                    KeymapKeysInput::Many(keys) => keys,
                };
                Ok(Self { keys, description })
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppOptions {
    pub display: DisplayOptions,
    pub composer: ComposerOptions,
    pub notifications: NotificationOptions,
    pub voice: VoiceOptions,
    pub ui_state: UiStateOptions,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(default)]
struct KeymapFileOptions {
    keymap: KeymapOptions,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct UiStateOptions {
    pub collapsed_channel_categories: Vec<Id<ChannelMarker>>,
    pub collapsed_server_folder_ids: Vec<u64>,
    pub collapsed_server_folder_guilds: Vec<Vec<Id<GuildMarker>>>,
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
            notification_icon: None,
            voice_join_sound: None,
            voice_leave_sound: None,
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
            circular_avatars: false,
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

pub fn load_keymap_options() -> Result<KeymapOptions> {
    let path = keymap_path()?;
    load_keymap_options_from_path(&path)
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

fn load_keymap_options_from_path(path: &Path) -> Result<KeymapOptions> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(toml::from_str::<KeymapFileOptions>(&content)?.keymap),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(KeymapOptions::default()),
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

fn keymap_path() -> Result<PathBuf> {
    paths::keymap_file().ok_or_else(|| {
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
        AppOptions, ComposerOptions, DisplayOptions, ImagePreviewQualityPreset, KeymapFileOptions,
        KeymapOptions, MicrophoneSensitivityDb, NotificationOptions, VoiceOptions,
        VoiceVolumePercent, load_keymap_options_from_path, load_options_from_path,
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
            circular_avatars: false,
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
            (
                "[notifications]\nvoice_join_sound = \"/tmp/join.wav\"\nvoice_leave_sound = \"/tmp/leave.wav\"\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[composer]\nemojis_as_links = true\n",
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
            assert!(!config.display.circular_avatars);
            let expected_desktop_notifications =
                !toml.contains("[notifications]\ndesktop_notifications = false");
            assert_eq!(
                config.notifications.desktop_notifications,
                expected_desktop_notifications
            );
            if toml.contains("voice_join_sound") {
                assert_eq!(
                    config.notifications.voice_join_sound.as_deref(),
                    Some(std::path::Path::new("/tmp/join.wav"))
                );
                assert_eq!(
                    config.notifications.voice_leave_sound.as_deref(),
                    Some(std::path::Path::new("/tmp/leave.wav"))
                );
            } else {
                assert!(config.notifications.voice_join_sound.is_none());
                assert!(config.notifications.voice_leave_sound.is_none());
            }
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
            assert_eq!(
                config.composer.emojis_as_links,
                toml.contains("emojis_as_links")
            );
        }
    }

    #[test]
    fn config_options_ignore_keymap_sections() {
        let config: AppOptions = toml::from_str(
            "[keymap]\nStartComposer = { keys = [\"c\"] }\n\n[display]\nshow_avatars = false\n",
        )
        .expect("config should ignore keymap table");

        assert!(!config.display.show_avatars);
    }

    #[test]
    fn keymap_options_parse_partial_toml() {
        let keymap = parse_keymap_options(
            "[keymap]\nleader = \"space\"\nStartComposer = \"<leader>e\"\nReplyMessage = \"<leader>m r\"\n\n[keymap.groups]\n\"<C-w>\" = \"Window\"\n",
        );

        assert_eq!(keymap.leader.as_deref(), Some("space"));
        assert_eq!(
            keymap.mappings.get("StartComposer"),
            Some(&crate::config::KeymapBinding::one("<leader>e"))
        );
        assert_eq!(
            keymap.mappings.get("ReplyMessage"),
            Some(&crate::config::KeymapBinding::one("<leader>m r"))
        );
        assert_eq!(
            keymap.groups.get("<C-w>").map(String::as_str),
            Some("Window")
        );
        assert!(keymap.guild_actions.is_empty());
        assert!(keymap.channel_actions.is_empty());
        assert!(keymap.message_actions.is_empty());
        assert!(keymap.member_actions.is_empty());
        assert!(keymap.composer.is_empty());
    }

    #[test]
    fn keymap_options_parse_structured_bindings() {
        let keymap = parse_keymap_options(
            "[keymap]\nChannelSwitcher = { keys = [\"<C-w>f\", \"<leader><C-w>\"], description = \"find channel\" }\nOpenPaneFilter = { keys = \"<C-f>\" }\n",
        );

        assert_eq!(
            keymap.mappings.get("ChannelSwitcher"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["<C-w>f".to_owned(), "<leader><C-w>".to_owned()],
                description: Some("find channel".to_owned()),
            })
        );
        assert_eq!(
            keymap.mappings.get("OpenPaneFilter"),
            Some(&crate::config::KeymapBinding::one("<C-f>"))
        );
    }

    #[test]
    fn keymap_options_parse_documented_start_composer_binding() {
        let keymap = parse_keymap_options("[keymap]\nStartComposer = { keys = [\"c\"] }\n");

        assert_eq!(
            keymap.mappings.get("StartComposer"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["c".to_owned()],
                description: None,
            })
        );
    }

    #[test]
    fn keymap_options_parse_action_table_bindings() {
        let keymap = parse_keymap_options(
            "[keymap.VoiceDeafen]\nkeys = [\"dd\"]\ndescription = \"deafen voice\"\n",
        );

        assert_eq!(
            keymap.mappings.get("VoiceDeafen"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["dd".to_owned()],
                description: Some("deafen voice".to_owned()),
            })
        );
    }

    #[test]
    fn keymap_options_parse_scoped_action_bindings() {
        let keymap = parse_keymap_options(
            "[keymap.guild_actions]\nMuteServer = { keys = [\"m\"], description = \"mute server\" }\n\n[keymap.channel_actions]\nMuteChannel = \"x\"\n\n[keymap.message_actions]\nGoToReferencedMessage = { keys = [\"g\"], description = \"go to referenced message\" }\n\n[keymap.member_actions]\nShowProfile = \"p\"\n\n[keymap.composer]\nOpenEditor = \"<C-o>\"\nDeletePreviousWord = { keys = [\"<A-backspace>\"], description = \"delete word\" }\n",
        );

        assert_eq!(
            keymap.guild_actions.get("MuteServer"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["m".to_owned()],
                description: Some("mute server".to_owned()),
            })
        );
        assert_eq!(
            keymap.channel_actions.get("MuteChannel"),
            Some(&crate::config::KeymapBinding::one("x"))
        );
        assert_eq!(
            keymap.member_actions.get("ShowProfile"),
            Some(&crate::config::KeymapBinding::one("p"))
        );
        assert_eq!(
            keymap.message_actions.get("GoToReferencedMessage"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["g".to_owned()],
                description: Some("go to referenced message".to_owned()),
            })
        );
        assert_eq!(
            keymap.composer.get("OpenEditor"),
            Some(&crate::config::KeymapBinding::one("<C-o>"))
        );
        assert_eq!(
            keymap.composer.get("DeletePreviousWord"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["<A-backspace>".to_owned()],
                description: Some("delete word".to_owned()),
            })
        );
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
                circular_avatars: true,
                server_width: 12,
                channel_list_width: 30,
                member_list_width: 18,
            },
            composer: ComposerOptions {
                emojis_as_links: true,
            },
            notifications: NotificationOptions {
                desktop_notifications: false,
                notification_icon: Some("/tmp/icon.svg".to_string()),
                voice_join_sound: Some(std::path::PathBuf::from("/tmp/join.wav")),
                voice_leave_sound: Some(std::path::PathBuf::from("/tmp/leave.wav")),
            },
            voice: VoiceOptions {
                self_mute: true,
                self_deaf: true,
                allow_microphone_transmit: true,
                microphone_sensitivity: MicrophoneSensitivityDb::new(-50),
                microphone_volume: VoiceVolumePercent::new(80),
                voice_output_volume: VoiceVolumePercent::new(60),
            },
            ui_state: Default::default(),
        };

        save_options_to_path(&path, &options).expect("config should save");
        let saved = fs::read_to_string(&path).expect("config should be readable");
        assert!(!saved.contains("[keymap"));
        let loaded = load_options_from_path(&path).expect("config should load");

        assert_eq!(loaded, options);
        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn keymap_options_load_from_path_defaults_when_missing() {
        let path = test_keymap_path();

        let loaded = load_keymap_options_from_path(&path).expect("missing keymap should load");

        assert_eq!(loaded, KeymapOptions::default());
    }

    #[test]
    fn keymap_options_load_from_path_reads_keymap_file() {
        let path = test_keymap_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test keymap parent should be created");
        }
        fs::write(&path, "[keymap]\nStartComposer = { keys = [\"c\"] }\n")
            .expect("test keymap should be written");

        let loaded = load_keymap_options_from_path(&path).expect("keymap should load");

        assert_eq!(
            loaded.mappings.get("StartComposer"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["c".to_owned()],
                description: None,
            })
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

    fn test_keymap_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("concord-keymap-test-{unique}"))
            .join("keymap.toml")
    }

    fn parse_keymap_options(toml: &str) -> KeymapOptions {
        toml::from_str::<KeymapFileOptions>(toml)
            .expect("keymap config should parse")
            .keymap
    }
}
