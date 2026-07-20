use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize, de};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::discord::{MicrophoneSensitivityDb, VoiceVolumePercent};

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
    pub notification_inbox_actions: BTreeMap<String, KeymapBinding>,
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

/// Validated Highlight Group and UI definitions from `theme.toml`.
///
/// The storage is private so only registered names cross the configuration
/// boundary into runtime theme resolution.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThemeOptions {
    highlights: BTreeMap<HighlightGroup, HighlightDefinitionOptions>,
    border_shapes: BorderShapeOptions,
}

impl ThemeOptions {
    pub(crate) fn highlights(&self) -> &BTreeMap<HighlightGroup, HighlightDefinitionOptions> {
        &self.highlights
    }

    pub(crate) const fn border_shapes(&self) -> &BorderShapeOptions {
        &self.border_shapes
    }

    pub(crate) const fn border_shapes_mut(&mut self) -> &mut BorderShapeOptions {
        &mut self.border_shapes
    }

    pub(crate) fn highlight_mut(
        &mut self,
        group: HighlightGroup,
    ) -> &mut HighlightDefinitionOptions {
        self.highlights.entry(group).or_default()
    }
}

macro_rules! define_border_surfaces {
    ($($variant:ident => $name:literal, rounded_by_default = $rounded:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(usize)]
        pub(crate) enum BorderSurface {
            $($variant),+
        }

        impl BorderSurface {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub(crate) const COUNT: usize = Self::ALL.len();

            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub(crate) const fn rounded_by_default(self) -> bool {
                match self {
                    $(Self::$variant => $rounded),+
                }
            }
        }
    };
}

define_border_surfaces! {
    Pane => "pane", rounded_by_default = false,
    Composer => "composer", rounded_by_default = true,
    Modal => "modal", rounded_by_default = false,
    Picker => "picker", rounded_by_default = false,
    Login => "login", rounded_by_default = false,
    Message => "message", rounded_by_default = true,
    Forum => "forum", rounded_by_default = true,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BorderShapeOptions {
    pub(crate) default: Option<BorderShape>,
    surfaces: [Option<BorderShape>; BorderSurface::COUNT],
}

impl BorderShapeOptions {
    pub(crate) const fn get(&self, surface: BorderSurface) -> Option<BorderShape> {
        self.surfaces[surface as usize]
    }

    pub(crate) fn set(&mut self, surface: BorderSurface, shape: BorderShape) {
        self.surfaces[surface as usize] = Some(shape);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BorderShape {
    Plain,
    Rounded,
    Double,
    Thick,
    LightDoubleDashed,
    HeavyDoubleDashed,
    LightTripleDashed,
    HeavyTripleDashed,
    LightQuadrupleDashed,
    HeavyQuadrupleDashed,
    QuadrantInside,
    QuadrantOutside,
}

impl BorderShape {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "plain" => Some(Self::Plain),
            "rounded" => Some(Self::Rounded),
            "double" => Some(Self::Double),
            "thick" => Some(Self::Thick),
            "light_double_dashed" => Some(Self::LightDoubleDashed),
            "heavy_double_dashed" => Some(Self::HeavyDoubleDashed),
            "light_triple_dashed" => Some(Self::LightTripleDashed),
            "heavy_triple_dashed" => Some(Self::HeavyTripleDashed),
            "light_quadruple_dashed" => Some(Self::LightQuadrupleDashed),
            "heavy_quadruple_dashed" => Some(Self::HeavyQuadrupleDashed),
            "quadrant_inside" => Some(Self::QuadrantInside),
            "quadrant_outside" => Some(Self::QuadrantOutside),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct HighlightDefinitionOptions {
    pub(crate) link: Option<HighlightLinkOptions>,
    pub(crate) foreground: Option<String>,
    pub(crate) background: Option<String>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) dim: Option<bool>,
    pub(crate) underline: Option<bool>,
    pub(crate) strikethrough: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HighlightLinkOptions {
    Inherit(HighlightGroup),
    Detached,
}

macro_rules! define_highlight_groups {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        #[repr(usize)]
        pub(crate) enum HighlightGroup {
            $($variant),+
        }

        impl HighlightGroup {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub(crate) const COUNT: usize = Self::ALL.len();

            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub(super) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

define_highlight_groups! {
    Normal => "Normal",
    Strong => "Strong",
    Emphasis => "Emphasis",
    Muted => "Muted",
    Title => "Title",
    Heading => "Heading",
    Decoration => "Decoration",
    Hint => "Hint",
    Description => "Description",
    Shortcut => "Shortcut",
    Activity => "Activity",
    ChannelTypeMarker => "ChannelTypeMarker",
    FieldLabel => "FieldLabel",
    SearchContext => "SearchContext",
    Timestamp => "Timestamp",
    Placeholder => "Placeholder",
    Disabled => "Disabled",
    Loading => "Loading",
    Edited => "Edited",
    Unavailable => "Unavailable",
    LoginTitle => "LoginTitle",
    LoginHint => "LoginHint",
    PaneTitle => "PaneTitle",
    ModalTitle => "ModalTitle",
    ComposerTitle => "ComposerTitle",
    HeaderTitle => "HeaderTitle",
    HeaderLabel => "HeaderLabel",
    MessageAuthor => "MessageAuthor",
    MessageTimestamp => "MessageTimestamp",
    CategoryHeading => "CategoryHeading",
    MemberGroupHeading => "MemberGroupHeading",
    MessageSecondary => "MessageSecondary",
    ForumSecondary => "ForumSecondary",
    EmbedAuthor => "EmbedAuthor",
    EmbedTitle => "EmbedTitle",
    EmbedFieldName => "EmbedFieldName",
    EmbedFooter => "EmbedFooter",
    CodeBlockBorder => "CodeBlockBorder",
    ScrollbarTrack => "ScrollbarTrack",
    UnavailableEmoji => "UnavailableEmoji",
    HeaderError => "HeaderError",
    HeaderWarning => "HeaderWarning",
    Border => "Border",
    FocusBorder => "FocusBorder",
    Selection => "Selection",
    SelectionBorder => "SelectionBorder",
    PaneBorder => "PaneBorder",
    FocusedPaneBorder => "FocusedPaneBorder",
    LoginBorder => "LoginBorder",
    ComposerBorder => "ComposerBorder",
    ActiveComposerBorder => "ActiveComposerBorder",
    ModalBorder => "ModalBorder",
    ComposerPickerBorder => "ComposerPickerBorder",
    SelectedRow => "SelectedRow",
    SelectionMarker => "SelectionMarker",
    ActiveField => "ActiveField",
    ActiveTab => "ActiveTab",
    MessageSelectedBorder => "MessageSelectedBorder",
    ForumBorder => "ForumBorder",
    ForumSelectedBorder => "ForumSelectedBorder",
    ScrollbarThumb => "ScrollbarThumb",
    UnreadNotice => "UnreadNotice",
    Editing => "Editing",
    Reaction => "Reaction",
    SelfReaction => "SelfReaction",
    PresenceOnline => "PresenceOnline",
    PresenceIdle => "PresenceIdle",
    PresenceDnd => "PresenceDnd",
    PresenceOffline => "PresenceOffline",
    VoiceDisabled => "VoiceDisabled",
    VoiceConnection => "VoiceConnection",
    FolderFallback => "FolderFallback",
    NavigationActive => "NavigationActive",
    NavigationMentioned => "NavigationMentioned",
    NavigationNotified => "NavigationNotified",
    NavigationUnread => "NavigationUnread",
    MentionBadge => "MentionBadge",
    NotificationBadge => "NotificationBadge",
    JoinedVoiceChannel => "JoinedVoiceChannel",
    VoiceSpeaking => "VoiceSpeaking",
    ReplyPingEnabled => "ReplyPingEnabled",
    Tag => "Tag",
    RelationshipFriend => "RelationshipFriend",
    RelationshipIncoming => "RelationshipIncoming",
    RelationshipOutgoing => "RelationshipOutgoing",
    RelationshipBlocked => "RelationshipBlocked",
    RelationshipNone => "RelationshipNone",
    GaugeFill => "GaugeFill",
    MessageBody => "MessageBody",
    MarkdownHeading1 => "MarkdownHeading1",
    MarkdownHeading2 => "MarkdownHeading2",
    MarkdownHeading3 => "MarkdownHeading3",
    MarkdownQuote => "MarkdownQuote",
    MarkdownMarker => "MarkdownMarker",
    MessageAttachment => "MessageAttachment",
    ImageOverflow => "ImageOverflow",
    InlineCode => "InlineCode",
    MessageLink => "MessageLink",
    MentionSelf => "MentionSelf",
    MentionOther => "MentionOther",
    MentionRole => "MentionRole",
    MentionPickerRole => "MentionPickerRole",
    EmbedGutter => "EmbedGutter",
    EmbedLink => "EmbedLink",
    CommandName => "CommandName",
    SystemThreadName => "SystemThreadName",
    PollAnswerSelected => "PollAnswerSelected",
    PollWinner => "PollWinner",
    UnreadBanner => "UnreadBanner",
    UnreadDivider => "UnreadDivider",
    ForumPinnedBadge => "ForumPinnedBadge",
    BotBadge => "BotBadge",
    Error => "Error",
    Warning => "Warning",
    Success => "Success",
    Info => "Info",
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(default)]
pub(super) struct KeymapFileOptions {
    pub(super) keymap: KeymapOptions,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct UiStateFileOptions {
    pub(super) ui_state: UiStateOptions,
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
    /// One-to-one DMs confirmed to contain enough messages from the current
    /// user. This avoids reclassifying an established conversation when those
    /// messages later fall outside the latest 50-message window.
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
