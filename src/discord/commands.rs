use std::{
    io,
    path::{Path, PathBuf},
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, UserMarker},
};

use super::application_commands::ApplicationCommandInvocation;
use super::message::MessageInfo;
use super::{ActivityInfo, PresenceStatus};

pub const MAX_UPLOAD_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_UPLOAD_TOTAL_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_UPLOAD_ATTACHMENT_COUNT: usize = 10;
pub const MAX_PROFILE_AVATAR_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AttachmentDownloadId(u64);

impl AttachmentDownloadId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAttachmentUpload {
    source: UploadSource,
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GlobalUserProfileUpdate {
    pub display_name: Option<String>,
    pub pronouns: Option<String>,
    pub avatar: Option<ProfileAvatarUpload>,
}

impl GlobalUserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none() && self.pronouns.is_none() && self.avatar.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileAvatarUpload {
    source: UploadSource,
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum UploadSource {
    File(PathBuf),
    Bytes(Vec<u8>),
}

impl UploadSource {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::File(path) => Some(path),
            Self::Bytes(_) => None,
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::File(_) => None,
            Self::Bytes(bytes) => Some(bytes),
        }
    }
}

impl ProfileAvatarUpload {
    pub fn from_path(path: PathBuf) -> Self {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("avatar")
            .to_owned();
        Self {
            source: UploadSource::File(path),
            filename,
            size_bytes: 0,
        }
    }

    pub fn from_bytes(filename: String, bytes: Vec<u8>) -> Self {
        Self {
            size_bytes: bytes.len() as u64,
            source: UploadSource::Bytes(bytes),
            filename,
        }
    }

    pub fn from_message_attachment(upload: MessageAttachmentUpload) -> Self {
        Self {
            source: upload.source,
            filename: upload.filename,
            size_bytes: upload.size_bytes,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.source.path()
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.source.bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildUserProfileUpdate {
    pub guild_id: Id<GuildMarker>,
    pub nickname: Option<String>,
    pub pronouns: Option<String>,
}

impl GuildUserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.nickname.is_none() && self.pronouns.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfileUpdate {
    pub user_id: Id<UserMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub global: GlobalUserProfileUpdate,
    pub guild: Option<GuildUserProfileUpdate>,
}

impl UserProfileUpdate {
    pub fn is_empty(&self) -> bool {
        self.global.is_empty()
            && self
                .guild
                .as_ref()
                .is_none_or(GuildUserProfileUpdate::is_empty)
    }
}

impl MessageAttachmentUpload {
    pub fn from_path(path: PathBuf, filename: String, size_bytes: u64) -> Self {
        Self {
            source: UploadSource::File(path),
            filename,
            size_bytes,
        }
    }

    pub fn from_existing_path(path: PathBuf) -> io::Result<Self> {
        let metadata = path.metadata()?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_owned();
        Ok(Self::from_path(path, filename, metadata.len()))
    }

    pub fn from_bytes(filename: String, bytes: Vec<u8>) -> Self {
        Self {
            size_bytes: bytes.len() as u64,
            source: UploadSource::Bytes(bytes),
            filename,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.source.path()
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.source.bytes()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReactionEmoji {
    Unicode(String),
    Custom {
        id: Id<EmojiMarker>,
        name: Option<String>,
        animated: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ForumPostArchiveState {
    #[default]
    Active,
    Archived,
}

impl ForumPostArchiveState {
    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Active => "false",
            Self::Archived => "true",
        }
    }

    pub fn as_log_label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MuteDuration {
    Minutes(u64),
    Permanent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSearchHas {
    Link,
    Embed,
    File,
    Video,
    Image,
    Sound,
    Sticker,
}

impl MessageSearchHas {
    pub fn from_input(value: &str) -> Option<Self> {
        match normalized_search_token(value).as_str() {
            "link" | "links" => Some(Self::Link),
            "embed" | "embeds" => Some(Self::Embed),
            "file" | "files" | "attachment" | "attachments" => Some(Self::File),
            "video" | "videos" => Some(Self::Video),
            "image" | "images" | "img" => Some(Self::Image),
            "sound" | "sounds" | "audio" => Some(Self::Sound),
            "sticker" | "stickers" => Some(Self::Sticker),
            _ => None,
        }
    }

    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Embed => "embed",
            Self::File => "file",
            Self::Video => "video",
            Self::Image => "image",
            Self::Sound => "sound",
            Self::Sticker => "sticker",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSearchAuthorType {
    User,
    Bot,
    Webhook,
}

impl MessageSearchAuthorType {
    pub fn from_input(value: &str) -> Option<Self> {
        match normalized_search_token(value).as_str() {
            "user" | "person" | "people" => Some(Self::User),
            "bot" | "bots" => Some(Self::Bot),
            "webhook" | "webhooks" => Some(Self::Webhook),
            _ => None,
        }
    }

    pub fn as_query_value(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Bot => "bot",
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageSearchQuery {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub author_id: Option<Id<UserMarker>>,
    pub mentions_user_id: Option<Id<UserMarker>>,
    pub content: Option<String>,
    pub has: Vec<MessageSearchHas>,
    pub date: Option<String>,
    pub author_type: Vec<MessageSearchAuthorType>,
    pub pinned: Option<bool>,
    pub offset: usize,
}

impl MessageSearchQuery {
    pub fn is_empty(&self) -> bool {
        self.channel_id.is_none()
            && self.author_id.is_none()
            && self.mentions_user_id.is_none()
            && self.content.as_deref().is_none_or(str::is_empty)
            && self.has.is_empty()
            && self.date.as_deref().is_none_or(str::is_empty)
            && self.author_type.is_empty()
            && self.pinned.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSearchPage {
    pub query: MessageSearchQuery,
    pub messages: Vec<MessageInfo>,
    pub total_results: Option<usize>,
    pub has_more: bool,
}

impl MuteDuration {
    pub fn minutes(self) -> Option<u64> {
        match self {
            Self::Minutes(minutes) => Some(minutes),
            Self::Permanent => None,
        }
    }

    pub fn selected_time_window_seconds(self) -> i64 {
        match self {
            Self::Minutes(minutes) => i64::try_from(minutes.saturating_mul(60)).unwrap_or(i64::MAX),
            Self::Permanent => -1,
        }
    }
}

impl ReactionEmoji {
    pub fn status_label(&self) -> String {
        match self {
            Self::Unicode(emoji) => emoji.clone(),
            Self::Custom { name, .. } => name
                .as_deref()
                .map(|name| format!(":{name}:"))
                .unwrap_or_else(|| ":custom:".to_owned()),
        }
    }

    pub fn custom_image_url(&self) -> Option<String> {
        let Self::Custom { id, animated, .. } = self else {
            return None;
        };
        let extension = if *animated { "gif" } else { "png" };
        Some(format!(
            "https://cdn.discordapp.com/emojis/{}.{}",
            id.get(),
            extension
        ))
    }

    pub(crate) fn route_component(&self) -> String {
        match self {
            Self::Unicode(name) => percent_encode_path_segment(name),
            Self::Custom { id, name, .. } => percent_encode_path_segment(&format!(
                "{}:{id}",
                name.as_deref().unwrap_or_default()
            )),
        }
    }
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHistoryAfterMode {
    GapFill,
    CatchUp,
}

impl MessageHistoryAfterMode {
    pub(crate) fn exhausts_on_empty(self) -> bool {
        matches!(self, Self::GapFill)
    }

    pub(crate) fn is_catch_up(self) -> bool {
        matches!(self, Self::CatchUp)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
    },
    RefreshMessageHistory {
        channel_id: Id<ChannelMarker>,
    },
    LoadMessageHistoryAfter {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        mode: MessageHistoryAfterMode,
    },
    LoadMessageHistoryAround {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    LoadThreadPreview {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    LoadForumPosts {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    },
    SearchMessages {
        query: MessageSearchQuery,
    },
    LoadGuildMembers {
        guild_id: Id<GuildMarker>,
    },
    LoadGuildMembersByIds {
        guild_id: Id<GuildMarker>,
        user_ids: Vec<Id<UserMarker>>,
    },
    SearchGuildMembers {
        guild_id: Id<GuildMarker>,
        query: String,
    },
    SetSelectedGuild {
        guild_id: Option<Id<GuildMarker>>,
    },
    LeaveGuild {
        guild_id: Id<GuildMarker>,
        label: String,
    },
    SetSelectedMessageChannel {
        channel_id: Option<Id<ChannelMarker>>,
    },
    SubscribeDirectMessage {
        channel_id: Id<ChannelMarker>,
    },
    SubscribeGuildChannel {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    },
    /// Resubscribe an active op-37 channel subscription with a wider set of
    /// member-list ranges as the user scrolls through the member sidebar.
    UpdateMemberListSubscription {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    },
    JoinVoiceChannel {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        self_mute: bool,
        self_deaf: bool,
        allow_microphone_transmit: bool,
        microphone_sensitivity: crate::config::MicrophoneSensitivityDb,
        microphone_volume: crate::config::VoiceVolumePercent,
        voice_output_volume: crate::config::VoiceVolumePercent,
    },
    UpdateVoiceState {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        self_mute: bool,
        self_deaf: bool,
    },
    UpdateVoiceCapturePermission {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        allow_microphone_transmit: bool,
        microphone_sensitivity: crate::config::MicrophoneSensitivityDb,
        microphone_volume: crate::config::VoiceVolumePercent,
        voice_output_volume: crate::config::VoiceVolumePercent,
    },
    LeaveVoiceChannel {
        guild_id: Id<GuildMarker>,
        self_mute: bool,
        self_deaf: bool,
    },
    LoadAttachmentPreview {
        url: String,
    },
    LoadProfileAvatarPreview {
        key: String,
        upload: ProfileAvatarUpload,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
        reply_to: Option<Id<MessageMarker>>,
        attachments: Vec<MessageAttachmentUpload>,
    },
    SendTtsMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
    },
    LoadApplicationCommands {
        guild_id: Option<Id<GuildMarker>>,
    },
    RunApplicationCommand {
        invocation: ApplicationCommandInvocation,
    },
    EditMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: String,
    },
    DeleteMessage {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    OpenUrl {
        url: String,
    },
    PlayMedia {
        target: MediaPlaybackTarget,
        request_id: Option<MediaPlaybackRequestId>,
    },
    DownloadAttachment {
        id: AttachmentDownloadId,
        url: String,
        filename: String,
        source: DownloadAttachmentSource,
    },
    AddReaction {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    RemoveReaction {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    LoadReactionUsers {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<ReactionEmoji>,
    },
    LoadPinnedMessages {
        channel_id: Id<ChannelMarker>,
    },
    SetMessagePinned {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    },
    VotePoll {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: Vec<u8>,
    },
    LoadUserProfile {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    },
    LoadUserNote {
        user_id: Id<UserMarker>,
    },
    UpdateUserProfile {
        update: UserProfileUpdate,
    },
    UpdateCurrentUserStatus {
        status: PresenceStatus,
    },
    UpdateCurrentUserActivity {
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    AckChannel {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    ScheduleAckChannel {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    SetGuildMuted {
        guild_id: Id<GuildMarker>,
        muted: bool,
        duration: Option<MuteDuration>,
        label: String,
    },
    SetChannelMuted {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        duration: Option<MuteDuration>,
        label: String,
    },
    AckChannels {
        targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    },
}

fn normalized_search_token(value: &str) -> String {
    value.trim().trim_start_matches(':').to_ascii_lowercase()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadAttachmentSource {
    AttachmentViewer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaPlaybackSource {
    Message,
    AttachmentViewer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaPlaybackRequestId(u64);

impl MediaPlaybackRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaPlaybackTarget {
    pub url: String,
    pub label: String,
    pub source: MediaPlaybackSource,
}
