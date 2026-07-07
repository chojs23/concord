use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{Result, paths, support::private_file};

pub const DEFAULT_SERVER_WIDTH: u16 = 20;
pub const DEFAULT_CHANNEL_LIST_WIDTH: u16 = 24;
pub const DEFAULT_MEMBER_LIST_WIDTH: u16 = 26;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayOptions {
    pub disable_image_preview: bool,
    pub show_avatars: bool,
    pub show_images: bool,
    pub media_playback: bool,
    pub image_preview_quality: ImagePreviewQualityPreset,
    pub attachment_viewer_quality: ImagePreviewQualityPreset,
    pub image_protocol: ImageProtocolPreference,
    pub show_custom_emoji: bool,
    pub circular_avatars: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ComposerOptions {
    pub emojis_as_links: bool,
    pub ping_on_reply: bool,
}

impl Default for ComposerOptions {
    fn default() -> Self {
        Self {
            emojis_as_links: false,
            ping_on_reply: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct CredentialOptions {
    pub store: CredentialStoreMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialStoreMode {
    #[default]
    Auto,
    Keychain,
    Plain,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationOptions {
    pub desktop_notifications: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_sound: Option<PathBuf>,
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
    pub thread_actions: BTreeMap<String, KeymapBinding>,
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
    // Variant order matters: `untagged` tries top to bottom, so `Disabled`
    // catches `false` before `Keys` reads it, and `Keys` catches an array
    // before `Structured`.
    Disabled(bool),
    Keys(KeymapKeysInput),
    Structured {
        keys: KeymapKeysInput,
        description: Option<String>,
    },
}

/// One key (`"x"`) or several (`["x", "y"]`), shared by the bare and table forms.
#[derive(Deserialize)]
#[serde(untagged)]
enum KeymapKeysInput {
    One(String),
    Many(Vec<String>),
}

impl KeymapKeysInput {
    fn into_keys(self) -> Vec<String> {
        match self {
            Self::One(key) if key.trim().is_empty() => Vec::new(),
            Self::One(key) => vec![key],
            Self::Many(keys) if keys.iter().all(|key| key.trim().is_empty()) => Vec::new(),
            Self::Many(keys) => keys,
        }
    }
}

impl KeymapBinding {
    pub fn one(key: impl Into<String>) -> Self {
        Self {
            keys: vec![key.into()],
            description: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            keys: Vec::new(),
            description: None,
        }
    }

    pub fn is_disabled(&self) -> bool {
        self.keys.is_empty()
    }
}

impl<'de> Deserialize<'de> for KeymapBinding {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (keys, description) = match KeymapBindingInput::deserialize(deserializer)? {
            KeymapBindingInput::Disabled(false) => return Ok(Self::disabled()),
            KeymapBindingInput::Disabled(true) => {
                return Err(de::Error::custom(
                    "keymap binding boolean must be false to disable the shortcut",
                ));
            }
            KeymapBindingInput::Keys(keys) => (keys.into_keys(), None),
            KeymapBindingInput::Structured { keys, description } => (keys.into_keys(), description),
        };
        if keys.is_empty() {
            Ok(Self::disabled())
        } else {
            Ok(Self { keys, description })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PresenceOptions {
    pub share_rich_presence: bool,
}

impl Default for PresenceOptions {
    fn default() -> Self {
        Self {
            share_rich_presence: true,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct AppOptions {
    pub display: DisplayOptions,
    pub composer: ComposerOptions,
    pub credentials: CredentialOptions,
    pub notifications: NotificationOptions,
    pub voice: VoiceOptions,
    pub presence: PresenceOptions,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(default)]
struct KeymapFileOptions {
    keymap: KeymapOptions,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
struct UiStateFileOptions {
    ui_state: UiStateOptions,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct UiStateOptions {
    #[serde(default = "default_pane_visible")]
    pub guild_pane_visible: bool,
    #[serde(default = "default_pane_visible")]
    pub channel_pane_visible: bool,
    #[serde(default = "default_pane_visible")]
    pub member_pane_visible: bool,
    #[serde(default = "default_server_width")]
    pub server_width: u16,
    #[serde(default = "default_channel_list_width")]
    pub channel_list_width: u16,
    #[serde(default = "default_member_list_width")]
    pub member_list_width: u16,
    pub collapsed_channel_categories: Vec<Id<ChannelMarker>>,
    pub collapsed_server_folder_ids: Vec<u64>,
    pub collapsed_server_folder_guilds: Vec<Vec<Id<GuildMarker>>>,
    /// DMs confirmed to hold enough of our own messages, persisted so the
    /// composer skips re-scanning history on every open.
    pub established_dms: Vec<Id<ChannelMarker>>,
}

impl Default for UiStateOptions {
    fn default() -> Self {
        Self {
            guild_pane_visible: true,
            channel_pane_visible: true,
            member_pane_visible: true,
            server_width: default_server_width(),
            channel_list_width: default_channel_list_width(),
            member_list_width: default_member_list_width(),
            collapsed_channel_categories: Vec::new(),
            collapsed_server_folder_ids: Vec::new(),
            collapsed_server_folder_guilds: Vec::new(),
            established_dms: Vec::new(),
        }
    }
}

fn default_pane_visible() -> bool {
    true
}

fn default_server_width() -> u16 {
    DEFAULT_SERVER_WIDTH
}

fn default_channel_list_width() -> u16 {
    DEFAULT_CHANNEL_LIST_WIDTH
}

fn default_member_list_width() -> u16 {
    DEFAULT_MEMBER_LIST_WIDTH
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
pub enum ImageProtocolPreference {
    #[default]
    Auto,
    Iterm2,
    Kitty,
    Sixel,
    Halfblocks,
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
            notification_sound: None,
            voice_join_sound: None,
            voice_leave_sound: None,
        }
    }
}

impl Default for CredentialOptions {
    fn default() -> Self {
        Self {
            store: CredentialStoreMode::Auto,
        }
    }
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            disable_image_preview: false,
            show_avatars: true,
            show_images: true,
            media_playback: false,
            image_preview_quality: ImagePreviewQualityPreset::default(),
            attachment_viewer_quality: ImagePreviewQualityPreset::Original,
            image_protocol: ImageProtocolPreference::default(),
            show_custom_emoji: true,
            circular_avatars: false,
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

    pub fn media_playback_enabled(self) -> bool {
        self.media_playback
    }

    pub fn custom_emoji_visible(self) -> bool {
        !self.disable_image_preview && self.show_custom_emoji
    }
}

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

/// Parse `config.toml` tolerantly: a value with a wrong type or unknown variant
/// is skipped (its field falls back to default) instead of discarding the whole
/// file. Only real syntax errors fail.
fn parse_app_options(content: &str) -> Result<(AppOptions, Vec<String>)> {
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

/// A TOML table holding a single `key = value`, used to probe one field at a
/// time.
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
        // A one-key probe deserializes exactly when that value is valid, because
        // `T` derives `#[serde(default)]` and fills in the omitted fields.
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

fn load_keymap_options_from_path(path: &Path) -> Result<(KeymapOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_keymap_options)
}

fn load_ui_state_options_from_path(path: &Path) -> Result<(UiStateOptions, Vec<String>)> {
    load_tolerant_options_from_path(path, parse_ui_state_options)
}

fn parse_ui_state_options(content: &str) -> Result<(UiStateOptions, Vec<String>)> {
    let root: toml::Table = toml::from_str(content)?;
    let mut warnings = Vec::new();
    let ui_state = section(&root, "ui_state", &mut warnings);
    Ok((ui_state, warnings))
}

/// Named map fields of `KeymapOptions`. Any other `[keymap]` key flattens into
/// `mappings` and is validated as a top-level binding instead. A test keeps this
/// in sync with the struct's named `BTreeMap` fields.
const KEYMAP_ACTION_MAPS: [&str; 7] = [
    "groups",
    "guild_actions",
    "channel_actions",
    "message_actions",
    "member_actions",
    "thread_actions",
    "composer",
];

/// Whether a `[keymap]` probe table deserializes, i.e. the binding it holds is
/// valid.
fn keymap_accepts(keymap: toml::Table) -> bool {
    toml::Value::Table(keymap)
        .try_into::<KeymapOptions>()
        .is_ok()
}

/// Parse `keymap.toml` tolerantly, one keybinding at a time: a bad binding (at
/// the top level or inside an action map like `[keymap.guild_actions]`) is
/// dropped on its own. Only real syntax errors fail.
fn parse_keymap_options(content: &str) -> Result<(KeymapOptions, Vec<String>)> {
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

fn state_path() -> Result<PathBuf> {
    resolved_path(
        paths::state_file(),
        "could not resolve user state directory",
    )
}

fn resolved_path(path: Option<PathBuf>, missing_message: &'static str) -> Result<PathBuf> {
    path.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, missing_message).into())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        AppOptions, ComposerOptions, CredentialOptions, CredentialStoreMode, DisplayOptions,
        ImagePreviewQualityPreset, ImageProtocolPreference, KeymapBinding, KeymapFileOptions,
        KeymapOptions, MicrophoneSensitivityDb, NotificationOptions, PresenceOptions, VoiceOptions,
        VoiceVolumePercent, load_keymap_options_from_path, load_options_from_path,
        parse_app_options, save_options_to_path,
    };

    #[test]
    fn display_options_default_to_all_media_enabled() {
        let options = DisplayOptions::default();

        assert!(options.avatars_visible());
        assert!(options.images_visible());
        assert!(!options.media_playback_enabled());
        assert!(options.custom_emoji_visible());
        assert_eq!(
            options.image_preview_quality,
            ImagePreviewQualityPreset::Balanced
        );
        assert_eq!(options.image_protocol, ImageProtocolPreference::Auto);
    }

    #[test]
    fn global_disable_overrides_individual_toggles() {
        let options = DisplayOptions {
            disable_image_preview: true,
            show_avatars: true,
            show_images: true,
            media_playback: false,
            image_preview_quality: ImagePreviewQualityPreset::Balanced,
            attachment_viewer_quality: ImagePreviewQualityPreset::Original,
            image_protocol: ImageProtocolPreference::Auto,
            show_custom_emoji: true,
            circular_avatars: false,
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
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[display]\nmedia_playback = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
                false,
                true,
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
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[voice]\nallow_microphone_transmit = true\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
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
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[notifications]\nnotification_sound = \"/tmp/message.wav\"\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
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
                false,
                MicrophoneSensitivityDb::default(),
            ),
            (
                "[credentials]\nstore = \"plain\"\n",
                false,
                ImagePreviewQualityPreset::Balanced,
                false,
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
            media_playback,
            allow_microphone_transmit,
            microphone_sensitivity,
        ) in cases
        {
            let config: AppOptions = toml::from_str(toml).expect("partial config should parse");
            assert_eq!(config.display.disable_image_preview, disable_image_preview);
            assert!(config.display.show_avatars);
            assert!(config.display.show_images);
            assert_eq!(config.display.media_playback, media_playback);
            assert_eq!(config.display.image_preview_quality, image_preview_quality);
            assert_eq!(config.display.image_protocol, ImageProtocolPreference::Auto);
            assert!(config.display.show_custom_emoji);
            assert!(!config.display.circular_avatars);
            let expected_desktop_notifications =
                !toml.contains("[notifications]\ndesktop_notifications = false");
            assert_eq!(
                config.notifications.desktop_notifications,
                expected_desktop_notifications
            );
            if toml.contains("notification_sound") {
                assert_eq!(
                    config.notifications.notification_sound.as_deref(),
                    Some(std::path::Path::new("/tmp/message.wav"))
                );
            } else {
                assert!(config.notifications.notification_sound.is_none());
            }
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
            assert_eq!(
                config.composer.emojis_as_links,
                toml.contains("emojis_as_links")
            );
            let expected_credential_store = if toml.contains("store = \"plain\"") {
                CredentialStoreMode::Plain
            } else {
                CredentialStoreMode::Auto
            };
            assert_eq!(config.credentials.store, expected_credential_store);
        }
    }

    #[test]
    fn display_image_protocol_parses_supported_values() {
        let cases = [
            ("auto", ImageProtocolPreference::Auto),
            ("iterm2", ImageProtocolPreference::Iterm2),
            ("kitty", ImageProtocolPreference::Kitty),
            ("sixel", ImageProtocolPreference::Sixel),
            ("halfblocks", ImageProtocolPreference::Halfblocks),
        ];

        for (value, expected) in cases {
            let config: AppOptions =
                toml::from_str(&format!("[display]\nimage_protocol = \"{value}\"\n"))
                    .expect("image protocol config should parse");

            assert_eq!(config.display.image_protocol, expected);
        }
    }

    #[test]
    fn invalid_value_is_skipped_without_discarding_the_rest() {
        let (options, warnings) = parse_app_options(
            "[display]\nshow_avatars = false\nimage_protocol = \"bogus\"\nshow_images = \"yes\"\n\n[voice]\nself_mute = true\n",
        )
        .expect("syntactically valid config should parse");

        assert!(!options.display.show_avatars, "valid sibling value applies");
        assert!(
            options.voice.self_mute,
            "valid value in other section applies"
        );
        assert_eq!(
            options.display.image_protocol,
            ImageProtocolPreference::default(),
            "invalid value falls back to default"
        );
        assert!(
            options.display.show_images,
            "wrong-typed value falls back to default"
        );
        assert_eq!(warnings.len(), 2, "one warning per skipped value");
        assert!(warnings.iter().any(|w| w.contains("image_protocol")));
        assert!(warnings.iter().any(|w| w.contains("show_images")));
    }

    #[test]
    fn valid_config_reports_no_warnings() {
        let (_, warnings) = parse_app_options("[display]\nshow_avatars = false\n")
            .expect("valid config should parse");

        assert!(warnings.is_empty());
    }

    #[test]
    fn syntax_error_still_fails() {
        assert!(parse_app_options("[display]\nshow_avatars = ").is_err());
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
    fn keymap_options_parse_multi_key_bindings() {
        let keymap = parse_keymap_options(
            "[keymap]\nChannelSwitcher = { keys = [\"<C-w>f\", \"<leader><C-w>\"], description = \"find channel\" }\nOpenPaneFilter = { keys = \"<C-f>\" }\n\n[keymap.composer]\nSubmit = [\"<A-enter>\", \"<C-enter>\", \"<S-enter>\"]\n",
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

        assert_eq!(
            keymap.composer.get("Submit"),
            Some(&crate::config::KeymapBinding {
                keys: vec![
                    "<A-enter>".to_owned(),
                    "<C-enter>".to_owned(),
                    "<S-enter>".to_owned(),
                ],
                description: None,
            })
        );
    }

    #[test]
    fn keymap_options_parse_disabled_bindings() {
        let keymap = parse_keymap_options(
            "[keymap]\nPlayMedia = \"\"\nOpenPaneFilter = false\n\n[keymap.message_actions]\nPlayMedia = false\nOpenUrl = \"\"\n",
        );

        assert_eq!(
            keymap.mappings.get("PlayMedia"),
            Some(&crate::config::KeymapBinding::disabled())
        );
        assert_eq!(
            keymap.mappings.get("OpenPaneFilter"),
            Some(&crate::config::KeymapBinding::disabled())
        );
        assert_eq!(
            keymap.message_actions.get("PlayMedia"),
            Some(&crate::config::KeymapBinding::disabled())
        );
        assert_eq!(
            keymap.message_actions.get("OpenUrl"),
            Some(&crate::config::KeymapBinding::disabled())
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
            "[keymap.guild_actions]\nToggleMute = { keys = [\"m\"], description = \"mute server\" }\n\n[keymap.channel_actions]\nToggleMute = \"x\"\n\n[keymap.message_actions]\nGoToReferencedMessage = { keys = [\"g\"], description = \"go to referenced message\" }\n\n[keymap.member_actions]\nShowProfile = \"p\"\n\n[keymap.composer]\nOpenEditor = \"<C-o>\"\nDeletePreviousWord = { keys = [\"<A-backspace>\"], description = \"delete word\" }\n",
        );

        assert_eq!(
            keymap.guild_actions.get("ToggleMute"),
            Some(&crate::config::KeymapBinding {
                keys: vec!["m".to_owned()],
                description: Some("mute server".to_owned()),
            })
        );
        assert_eq!(
            keymap.channel_actions.get("ToggleMute"),
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
    fn keymap_invalid_binding_is_skipped_without_discarding_the_rest() {
        let (keymap, warnings) = super::parse_keymap_options(
            "[keymap]\nleader = \"space\"\nStartComposer = \"<leader>e\"\nReplyMessage = 123\n\n[keymap.guild_actions]\nToggleMute = \"m\"\nBadAction = 7\n",
        )
        .expect("syntactically valid keymap should parse");

        assert_eq!(keymap.leader.as_deref(), Some("space"));
        assert_eq!(
            keymap.mappings.get("StartComposer"),
            Some(&crate::config::KeymapBinding::one("<leader>e")),
            "valid top-level binding applies"
        );
        assert!(
            !keymap.mappings.contains_key("ReplyMessage"),
            "invalid top-level binding is skipped"
        );
        assert_eq!(
            keymap.guild_actions.get("ToggleMute"),
            Some(&crate::config::KeymapBinding::one("m")),
            "valid action binding survives a bad sibling"
        );
        assert!(
            !keymap.guild_actions.contains_key("BadAction"),
            "invalid action binding is skipped"
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.contains("ReplyMessage")));
        assert!(warnings.iter().any(|w| w.contains("BadAction")));
    }

    #[test]
    fn keymap_action_maps_list_stays_in_sync_with_struct() {
        use std::collections::{BTreeMap, BTreeSet};

        // Built field by field on purpose: adding a named map to KeymapOptions
        // fails to compile here, a reminder to also list it in
        // KEYMAP_ACTION_MAPS so its bindings keep field-level tolerance.
        let action = || BTreeMap::from([("Probe".to_owned(), KeymapBinding::one("a"))]);
        let keymap = KeymapOptions {
            leader: Some("space".to_owned()),
            groups: BTreeMap::from([("Probe".to_owned(), "value".to_owned())]),
            guild_actions: action(),
            channel_actions: action(),
            message_actions: action(),
            member_actions: action(),
            thread_actions: action(),
            composer: action(),
            mappings: BTreeMap::new(),
        };

        // Named maps serialize as TOML tables; leader is a scalar. The set of
        // table-valued keys must equal the hand-maintained list.
        let serialized = toml::Value::try_from(&keymap).expect("keymap serializes");
        let named_maps: BTreeSet<&str> = serialized
            .as_table()
            .expect("keymap is a table")
            .iter()
            .filter(|(_, value)| value.is_table())
            .map(|(key, _)| key.as_str())
            .collect();

        let listed: BTreeSet<&str> = super::KEYMAP_ACTION_MAPS.iter().copied().collect();
        assert_eq!(named_maps, listed);
    }

    #[test]
    fn ui_state_invalid_value_is_skipped_without_discarding_the_rest() {
        let (ui_state, warnings) = super::parse_ui_state_options(
            "[ui_state]\nguild_pane_visible = false\nserver_width = \"wide\"\n",
        )
        .expect("syntactically valid ui_state should parse");

        assert!(!ui_state.guild_pane_visible, "valid value applies");
        assert_eq!(
            ui_state.server_width,
            super::DEFAULT_SERVER_WIDTH,
            "invalid value falls back to default"
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("server_width"));
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
                media_playback: false,
                image_preview_quality: ImagePreviewQualityPreset::Original,
                attachment_viewer_quality: ImagePreviewQualityPreset::Original,
                image_protocol: ImageProtocolPreference::Kitty,
                show_custom_emoji: false,
                circular_avatars: true,
            },
            composer: ComposerOptions {
                emojis_as_links: true,
                ping_on_reply: false,
            },
            credentials: CredentialOptions {
                store: CredentialStoreMode::Plain,
            },
            notifications: NotificationOptions {
                desktop_notifications: false,
                notification_icon: Some("/tmp/icon.svg".to_string()),
                notification_sound: Some(std::path::PathBuf::from("/tmp/message.wav")),
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
            presence: PresenceOptions {
                share_rich_presence: false,
            },
        };

        save_options_to_path(&path, &options).expect("config should save");
        let saved = fs::read_to_string(&path).expect("config should be readable");
        assert!(!saved.contains("[keymap"));
        assert!(!saved.contains("[ui_state"));
        let (loaded, warnings) = load_options_from_path(&path).expect("config should load");
        assert!(warnings.is_empty());

        assert_eq!(loaded.display, options.display);
        assert_eq!(loaded.composer, options.composer);
        assert_eq!(loaded.notifications, options.notifications);
        assert_eq!(loaded.voice, options.voice);
        assert_eq!(loaded.presence, options.presence);
        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn keymap_options_load_from_path_defaults_when_missing() {
        let path = test_keymap_path();

        let (loaded, warnings) =
            load_keymap_options_from_path(&path).expect("missing keymap should load");

        assert_eq!(loaded, KeymapOptions::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn keymap_options_load_from_path_reads_keymap_file() {
        let path = test_keymap_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test keymap parent should be created");
        }
        fs::write(&path, "[keymap]\nStartComposer = { keys = [\"c\"] }\n")
            .expect("test keymap should be written");

        let (loaded, _) = load_keymap_options_from_path(&path).expect("keymap should load");

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
