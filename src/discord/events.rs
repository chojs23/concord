use std::fmt;

use crate::discord::ids::{
    Id,
    marker::{
        AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker,
        UserMarker,
    },
};

use super::commands::{DownloadAttachmentSource, ForumPostArchiveState, ReactionEmoji};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
    Unknown,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::DoNotDisturb => "Do Not Disturb",
            Self::Offline => "Offline",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Custom,
    Competing,
    Unknown,
}

impl ActivityKind {
    pub fn from_code(code: u64) -> Self {
        match code {
            0 => Self::Playing,
            1 => Self::Streaming,
            2 => Self::Listening,
            3 => Self::Watching,
            4 => Self::Custom,
            5 => Self::Competing,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEmoji {
    pub name: String,
    pub id: Option<Id<EmojiMarker>>,
    pub animated: bool,
}

impl ActivityEmoji {
    /// CDN URL for the emoji image, when this is a custom emoji (i.e. carries
    /// an `id`). Returns `None` for unicode-only emojis, which render as text
    /// and don't need a network fetch.
    pub fn image_url(&self) -> Option<String> {
        let id = self.id?;
        let ext = if self.animated { "gif" } else { "png" };
        Some(format!(
            "https://cdn.discordapp.com/emojis/{}.{}",
            id.get(),
            ext
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityInfo {
    pub kind: ActivityKind,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub application_id: Option<String>,
    pub emoji: Option<ActivityEmoji>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
    pub message_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub thread_archived: Option<bool>,
    pub thread_locked: Option<bool>,
    /// Whether this thread is pinned in its parent forum/media channel.
    /// Discord encodes the bit in `flags` (`PINNED = 1 << 1`). Only set on
    /// threads inside forum-style parents.
    pub thread_pinned: Option<bool>,
    pub recipients: Option<Vec<ChannelRecipientInfo>>,
    /// Channel-level permission overrides. The empty default means a
    /// gateway/REST payload that omitted the field is treated as "no
    /// channel-specific overrides", which matches Discord's behavior of
    /// inheriting from the guild base permissions.
    pub permission_overwrites: Vec<PermissionOverwriteInfo>,
}

/// Whether a `PermissionOverwriteInfo` targets a role or an individual member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionOverwriteKind {
    Role,
    Member,
}

/// A single channel-level allow/deny pair against either a role or a member.
/// IDs are stored raw because the same field can refer to a role id, a member
/// id, or the guild id (the `@everyone` role is keyed by the guild snowflake).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PermissionOverwriteInfo {
    pub id: u64,
    pub kind: PermissionOverwriteKind,
    pub allow: u64,
    pub deny: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelRecipientInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle (`User.name`). Kept alongside `display_name` so
    /// the @-mention picker can fuzzy-match on both the alias and the raw
    /// username. `None` when the source payload didn't carry a username.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub status: Option<PresenceStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle (`User.name`). Same role as in
    /// [`ChannelRecipientInfo::username`].
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoiceStateInfo {
    pub guild_id: Id<GuildMarker>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub user_id: Id<UserMarker>,
    pub session_id: Option<String>,
    pub member: Option<MemberInfo>,
    pub deaf: bool,
    pub mute: bool,
    pub self_deaf: bool,
    pub self_mute: bool,
    pub self_stream: bool,
}

impl fmt::Debug for VoiceStateInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceStateInfo")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field(
                "session_id",
                &self.session_id.as_ref().map(|_| "<redacted>"),
            )
            .field("member", &self.member)
            .field("deaf", &self.deaf)
            .field("mute", &self.mute)
            .field("self_deaf", &self.self_deaf)
            .field("self_mute", &self.self_mute)
            .field("self_stream", &self.self_stream)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct VoiceServerInfo {
    pub guild_id: Id<GuildMarker>,
    pub endpoint: Option<String>,
    pub token: String,
}

impl fmt::Debug for VoiceServerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceServerInfo")
            .field("guild_id", &self.guild_id)
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleInfo {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
    /// Discord permission bitfield carried by this role. Used by
    /// `DiscordState::can_view_channel` to compute base permissions and
    /// detect ADMINISTRATOR.
    pub permissions: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentionInfo {
    pub user_id: Id<UserMarker>,
    /// Per-server nickname carried by this message's mention payload. Kept
    /// separate from `display_name` so rendering can prefer a proven guild
    /// alias while still using cached member names when the payload only has a
    /// global display name or username.
    pub guild_nick: Option<String>,
    pub display_name: String,
}

/// One entry from the user's `guild_folders` setting. A folder with `id ==
/// None` and a single member is an ungrouped guild. Discord stores those as
/// "folders" too just for ordering. Real folders carry an integer id, an
/// optional name, and an optional RGB color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildFolder {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<Id<GuildMarker>>,
}

/// One entry from `READY.read_state.entries[]`. The Discord wire field
/// `last_message_id` is renamed here because it actually carries the
/// last *ACKED* id, not the newest message in the channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadStateInfo {
    pub channel_id: Id<ChannelMarker>,
    pub last_acked_message_id: Option<Id<MessageMarker>>,
    pub mention_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationLevel {
    AllMessages,
    OnlyMentions,
    NoMessages,
    ParentDefault,
}

impl NotificationLevel {
    pub const fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::AllMessages),
            1 => Some(Self::OnlyMentions),
            2 => Some(Self::NoMessages),
            3 => Some(Self::ParentDefault),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelNotificationOverrideInfo {
    pub channel_id: Id<ChannelMarker>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildNotificationSettingsInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub message_notifications: Option<NotificationLevel>,
    pub muted: bool,
    pub mute_end_time: Option<String>,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub channel_overrides: Vec<ChannelNotificationOverrideInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustomEmojiInfo {
    pub id: Id<EmojiMarker>,
    pub name: String,
    pub animated: bool,
    pub available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentInfo {
    pub id: Id<AttachmentMarker>,
    pub filename: String,
    pub url: String,
    pub proxy_url: String,
    pub content_type: Option<String>,
    pub size: u64,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbedFieldInfo {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbedInfo {
    pub color: Option<u32>,
    pub provider_name: Option<String>,
    pub author_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub timestamp: Option<String>,
    pub fields: Vec<EmbedFieldInfo>,
    pub footer_text: Option<String>,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_proxy_url: Option<String>,
    pub thumbnail_width: Option<u64>,
    pub thumbnail_height: Option<u64>,
    pub image_url: Option<String>,
    pub image_proxy_url: Option<String>,
    pub image_width: Option<u64>,
    pub image_height: Option<u64>,
    pub video_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlinePreviewInfo<'a> {
    pub url: &'a str,
    pub proxy_url: Option<&'a str>,
    pub filename: &'a str,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub accent_color: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MessageKind {
    code: u8,
}

impl MessageKind {
    pub const fn new(code: u8) -> Self {
        Self { code }
    }

    pub const fn regular() -> Self {
        Self::new(0)
    }

    pub const fn code(self) -> u8 {
        self.code
    }

    pub const fn is_regular(self) -> bool {
        self.code == 0
    }

    pub const fn is_regular_or_reply(self) -> bool {
        // if it's a message or a reply to one
        self.code == 0 || self.code == 19
    }

    pub const fn known_label(self) -> Option<&'static str> {
        match self.code {
            0 => Some("Default"),
            1 => Some("Recipient add"),
            2 => Some("Recipient remove"),
            3 => Some("Call"),
            4 => Some("Channel name change"),
            5 => Some("Channel icon change"),
            6 => Some("Pinned message"),
            7 => Some("User join"),
            8 => Some("Guild boost"),
            9 => Some("Guild boost tier 1"),
            10 => Some("Guild boost tier 2"),
            11 => Some("Guild boost tier 3"),
            12 => Some("Channel follow add"),
            14 => Some("Guild discovery disqualified"),
            15 => Some("Guild discovery requalified"),
            16 => Some("Guild discovery initial warning"),
            17 => Some("Guild discovery final warning"),
            18 => Some("Thread created"),
            19 => Some("Reply"),
            20 => Some("Chat input command"),
            21 => Some("Thread starter message"),
            22 => Some("Guild invite reminder"),
            23 => Some("Context menu command"),
            24 => Some("Auto moderation action"),
            25 => Some("Role subscription purchase"),
            26 => Some("Premium upsell"),
            27 => Some("Stage start"),
            28 => Some("Stage end"),
            29 => Some("Stage speaker"),
            31 => Some("Stage topic"),
            32 => Some("Application premium subscription"),
            36 => Some("Incident alert mode enabled"),
            37 => Some("Incident alert mode disabled"),
            38 => Some("Incident raid report"),
            39 => Some("Incident false alarm report"),
            44 => Some("Purchase notification"),
            46 => Some("Poll result"),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self.known_label() {
            Some(label) => label,
            None => "Unknown message type",
        }
    }
}

impl Default for MessageKind {
    fn default() -> Self {
        Self::regular()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSnapshotInfo {
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub source_channel_id: Option<Id<ChannelMarker>>,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyInfo {
    pub author_id: Option<Id<UserMarker>>,
    pub author: String,
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    pub mentions: Vec<MentionInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageReferenceInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Option<Id<ChannelMarker>>,
    pub message_id: Option<Id<MessageMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollInfo {
    pub question: String,
    pub answers: Vec<PollAnswerInfo>,
    pub allow_multiselect: bool,
    pub results_finalized: Option<bool>,
    pub total_votes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollAnswerInfo {
    pub answer_id: u8,
    pub text: String,
    pub vote_count: Option<u64>,
    pub me_voted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionInfo {
    pub emoji: ReactionEmoji,
    pub count: u64,
    pub me: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUserInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersInfo {
    pub emoji: ReactionEmoji,
    pub users: Vec<ReactionUserInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub author_role_ids: Vec<Id<RoleMarker>>,
    pub message_kind: MessageKind,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub pinned: bool,
    pub reactions: Vec<ReactionInfo>,
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
    pub edited_timestamp: Option<String>,
}

impl Default for MessageInfo {
    fn default() -> Self {
        Self {
            guild_id: None,
            channel_id: Id::new(1),
            message_id: Id::new(1),
            author_id: Id::new(1),
            author: String::new(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::default(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachmentUpdate {
    Unchanged,
    Replace(Vec<AttachmentInfo>),
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
        user_id: Option<Id<UserMarker>>,
    },
    CurrentUserCapabilities {
        can_use_animated_custom_emojis: bool,
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        member_count: Option<u64>,
        /// Snowflake of the guild owner. The owner short-circuits permission
        /// checks (sees every channel regardless of overwrites).
        owner_id: Option<Id<UserMarker>>,
        channels: Vec<ChannelInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<(Id<UserMarker>, PresenceStatus)>,
        roles: Vec<RoleInfo>,
        emojis: Vec<CustomEmojiInfo>,
    },
    GuildUpdate {
        guild_id: Id<GuildMarker>,
        name: String,
        owner_id: Option<Id<UserMarker>>,
        roles: Option<Vec<RoleInfo>>,
        emojis: Option<Vec<CustomEmojiInfo>>,
    },
    GuildRolesUpdate {
        guild_id: Id<GuildMarker>,
        roles: Vec<RoleInfo>,
    },
    GuildEmojisUpdate {
        guild_id: Id<GuildMarker>,
        emojis: Vec<CustomEmojiInfo>,
    },
    GuildDelete {
        guild_id: Id<GuildMarker>,
    },
    SelectedGuildChanged {
        guild_id: Option<Id<GuildMarker>>,
    },
    SelectedMessageChannelChanged {
        channel_id: Option<Id<ChannelMarker>>,
    },
    ChannelUpsert(ChannelInfo),
    ChannelDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
    },
    MessageCreate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_id: Id<UserMarker>,
        author: String,
        author_avatar_url: Option<String>,
        author_role_ids: Vec<Id<RoleMarker>>,
        message_kind: MessageKind,
        reference: Option<MessageReferenceInfo>,
        reply: Option<ReplyInfo>,
        poll: Option<PollInfo>,
        content: Option<String>,
        sticker_names: Vec<String>,
        mentions: Vec<MentionInfo>,
        attachments: Vec<AttachmentInfo>,
        embeds: Vec<EmbedInfo>,
        forwarded_snapshots: Vec<MessageSnapshotInfo>,
    },
    MessageHistoryLoaded {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
    },
    ThreadPreviewLoaded {
        channel_id: Id<ChannelMarker>,
        message: MessageInfo,
    },
    ThreadPreviewLoadFailed {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    ForumPostsLoaded {
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        next_offset: usize,
        posts: Vec<ChannelInfo>,
        preview_messages: Vec<MessageInfo>,
        has_more: bool,
    },
    ForumPostsLoadFailed {
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        message: String,
    },
    MessageHistoryLoadFailed {
        channel_id: Id<ChannelMarker>,
        message: String,
    },
    MessageUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        poll: Option<PollInfo>,
        content: Option<String>,
        sticker_names: Option<Vec<String>>,
        mentions: Option<Vec<MentionInfo>>,
        attachments: AttachmentUpdate,
        embeds: Option<Vec<EmbedInfo>>,
        edited_timestamp: Option<String>,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    GuildMemberListCounts {
        guild_id: Id<GuildMarker>,
        online: u32,
    },
    GuildMemberUpsert {
        guild_id: Id<GuildMarker>,
        member: MemberInfo,
    },
    GuildMemberAdd {
        guild_id: Id<GuildMarker>,
        member: MemberInfo,
    },
    GuildMemberRemove {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    },
    PresenceUpdate {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    UserPresenceUpdate {
        user_id: Id<UserMarker>,
        status: PresenceStatus,
        activities: Vec<ActivityInfo>,
    },
    VoiceStateUpdate {
        state: VoiceStateInfo,
    },
    VoiceSpeakingUpdate {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    },
    VoiceServerUpdate {
        server: VoiceServerInfo,
    },
    VoiceConnectionStatusChanged {
        guild_id: Id<GuildMarker>,
        channel_id: Option<Id<ChannelMarker>>,
        status: VoiceConnectionStatus,
        message: Option<String>,
    },
    /// Discord's TYPING_START dispatch: emitted ~10s before the typing
    /// indicator should expire. The dashboard tracks the latest timestamp
    /// per (channel, user) and shows "X is typing…" while it's fresh.
    TypingStart {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    },
    CurrentUserReactionAdd {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    CurrentUserReactionRemove {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionAdd {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionRemove {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    },
    MessageReactionRemoveAll {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    MessageReactionRemoveEmoji {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    MessagePinnedUpdate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    },
    PinnedMessagesLoaded {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    PinnedMessagesLoadFailed {
        channel_id: Id<ChannelMarker>,
        message: String,
    },
    CurrentUserPollVoteUpdate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: Vec<u8>,
    },
    ReactionUsersLoaded {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<ReactionUsersInfo>,
    },
    GuildFoldersUpdate {
        folders: Vec<GuildFolder>,
    },
    UserGuildNotificationSettingsInit {
        settings: Vec<GuildNotificationSettingsInfo>,
    },
    UserGuildNotificationSettingsUpdate {
        settings: GuildNotificationSettingsInfo,
    },
    GatewayError {
        message: String,
    },
    AttachmentDownloadCompleted {
        path: String,
        source: DownloadAttachmentSource,
    },
    UpdateAvailable {
        latest_version: String,
    },
    AttachmentPreviewLoaded {
        url: String,
        bytes: Vec<u8>,
    },
    AttachmentPreviewLoadFailed {
        url: String,
        message: String,
    },
    UserProfileLoaded {
        guild_id: Option<Id<GuildMarker>>,
        profile: UserProfileInfo,
    },
    UserProfileLoadFailed {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        message: String,
    },
    UserNoteLoaded {
        user_id: Id<UserMarker>,
        note: Option<String>,
    },
    RelationshipsLoaded {
        relationships: Vec<RelationshipInfo>,
    },
    RelationshipUpsert {
        relationship: RelationshipInfo,
    },
    RelationshipRemove {
        user_id: Id<UserMarker>,
    },
    /// Tells the TUI to switch to a specific channel after a
    /// REST-side action (e.g. opening a DM) creates or resolves a channel
    /// outside the gateway flow. The channel itself must already be in
    /// state (typically because a prior `ChannelUpsert` for the same id
    /// arrived first).
    ActivateChannel {
        channel_id: Id<ChannelMarker>,
    },
    ReadStateInit {
        entries: Vec<ReadStateInfo>,
    },
    /// Gateway `MESSAGE_ACK` or a locally synthesized ack on activation.
    MessageAck {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        mention_count: u32,
    },
    GatewayClosed,
}

#[derive(Clone, Debug)]
pub struct SequencedAppEvent {
    pub revision: u64,
    pub event: AppEvent,
}

impl AppEvent {
    pub fn mutates_discord_state(&self) -> bool {
        !matches!(
            self,
            AppEvent::GatewayError { .. }
                | AppEvent::CurrentUserCapabilities { .. }
                | AppEvent::AttachmentDownloadCompleted { .. }
                | AppEvent::UpdateAvailable { .. }
                | AppEvent::ReactionUsersLoaded { .. }
                | AppEvent::AttachmentPreviewLoaded { .. }
                | AppEvent::AttachmentPreviewLoadFailed { .. }
                | AppEvent::ThreadPreviewLoadFailed { .. }
                | AppEvent::ForumPostsLoadFailed { .. }
                | AppEvent::MessageHistoryLoadFailed { .. }
                | AppEvent::PinnedMessagesLoadFailed { .. }
                | AppEvent::UserProfileLoadFailed { .. }
                | AppEvent::VoiceServerUpdate { .. }
                | AppEvent::VoiceConnectionStatusChanged { .. }
                | AppEvent::ActivateChannel { .. }
                | AppEvent::GatewayClosed
        )
    }

    pub fn needs_effect_delivery(&self) -> bool {
        match self {
            AppEvent::ChannelUpsert(channel) => channel_upsert_needs_effect_delivery(channel),
            AppEvent::MessageCreate { .. }
            | AppEvent::MessageHistoryLoaded { .. }
            | AppEvent::MessageHistoryLoadFailed { .. }
            | AppEvent::ThreadPreviewLoaded { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostsLoaded { .. }
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::PinnedMessagesLoaded { .. }
            | AppEvent::PinnedMessagesLoadFailed { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::CurrentUserCapabilities { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::ActivateChannel { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::VoiceConnectionStatusChanged { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::GatewayClosed => true,
            _ => false,
        }
    }
}

fn channel_upsert_needs_effect_delivery(channel: &ChannelInfo) -> bool {
    channel.parent_id.is_some()
        && matches!(
            channel.kind.as_str(),
            "thread" | "GuildPublicThread" | "GuildPrivateThread" | "GuildNewsThread"
        )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FriendStatus {
    None,
    Friend,
    Blocked,
    IncomingRequest,
    OutgoingRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipInfo {
    pub user_id: Id<UserMarker>,
    pub status: FriendStatus,
    /// Friend nickname set by the current user. This is distinct from guild
    /// nicknames and only applies to 1:1 friendships / DMs.
    pub nickname: Option<String>,
    /// Best available non-nickname label from the relationship payload,
    /// usually `global_name` and otherwise the username.
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutualGuildInfo {
    pub guild_id: Id<GuildMarker>,
    pub nick: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserProfileInfo {
    pub user_id: Id<UserMarker>,
    pub username: String,
    pub global_name: Option<String>,
    pub guild_nick: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub pronouns: Option<String>,
    pub mutual_guilds: Vec<MutualGuildInfo>,
    pub mutual_friends_count: u32,
    pub friend_status: FriendStatus,
    pub note: Option<String>,
}

impl UserProfileInfo {
    pub fn display_name(&self) -> &str {
        self.guild_nick
            .as_deref()
            .or(self.global_name.as_deref())
            .unwrap_or(&self.username)
    }
}

impl AttachmentInfo {
    pub fn preferred_url(&self) -> Option<&str> {
        if self.url.is_empty() {
            (!self.proxy_url.is_empty()).then_some(self.proxy_url.as_str())
        } else {
            Some(self.url.as_str())
        }
    }

    pub fn is_image(&self) -> bool {
        if let Some(content_type) = self.content_type.as_deref() {
            return content_type.starts_with("image/");
        }

        filename_has_extension(
            &self.filename,
            &["avif", "gif", "jpeg", "jpg", "png", "webp"],
        )
    }

    pub fn is_video(&self) -> bool {
        if let Some(content_type) = self.content_type.as_deref() {
            return content_type.starts_with("video/");
        }

        filename_has_extension(&self.filename, &["m4v", "mov", "mp4", "webm"])
    }

    pub fn inline_preview_url(&self) -> Option<&str> {
        self.is_image().then(|| self.preferred_url()).flatten()
    }

    pub fn inline_preview_info(&self) -> Option<InlinePreviewInfo<'_>> {
        Some(InlinePreviewInfo {
            url: self.inline_preview_url()?,
            proxy_url: (!self.proxy_url.is_empty()).then_some(self.proxy_url.as_str()),
            filename: self.filename.as_str(),
            width: self.width,
            height: self.height,
            accent_color: None,
        })
    }
}

impl EmbedInfo {
    pub fn inline_preview_info(&self) -> Option<InlinePreviewInfo<'_>> {
        if let Some(url) = self.thumbnail_url.as_deref() {
            return Some(InlinePreviewInfo {
                url,
                proxy_url: self.thumbnail_proxy_url.as_deref(),
                filename: "embed-thumbnail",
                width: self.thumbnail_width,
                height: self.thumbnail_height,
                accent_color: Some(self.color.unwrap_or(0xff0000)),
            });
        }

        self.image_url.as_deref().map(|url| InlinePreviewInfo {
            url,
            proxy_url: self.image_proxy_url.as_deref(),
            filename: "embed-image",
            width: self.image_width,
            height: self.image_height,
            accent_color: Some(self.color.unwrap_or(0xff0000)),
        })
    }
}

fn filename_has_extension(filename: &str, extensions: &[&str]) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| {
        extensions
            .iter()
            .any(|value| extension.eq_ignore_ascii_case(value))
    })
}

#[cfg(test)]
fn poll_result_info_from_fields<'a>(
    fields: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<PollInfo> {
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;
    for (name, value) in fields {
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let question = question.unwrap_or_else(|| "Poll results".to_owned());
    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();

    Some(PollInfo {
        question,
        answers,
        allow_multiselect: false,
        results_finalized: Some(true),
        total_votes,
    })
}

pub(crate) fn default_avatar_url(user_id: Id<UserMarker>, discriminator: u16) -> String {
    let index = if discriminator == 0 {
        (user_id.get() >> 22) % 6
    } else {
        u64::from(discriminator % 5)
    };

    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_media_classification_controls_inline_preview() {
        let video = attachment_info("clip.mp4", Some("video/mp4"));
        assert!(!video.is_image());
        assert!(video.is_video());
        assert_eq!(video.inline_preview_url(), None);

        let image = attachment_info("cat.png", Some("image/png"));
        assert!(image.is_image());
        assert!(!image.is_video());
        assert_eq!(
            image.inline_preview_url(),
            Some("https://cdn.discordapp.com/cat.png")
        );
        assert_eq!(
            image.inline_preview_info().and_then(|info| info.proxy_url),
            Some("https://media.discordapp.net/cat.png")
        );

        assert!(attachment_info("CAT.PNG", None).is_image());
        assert!(attachment_info("CLIP.MP4", None).is_video());
    }

    #[test]
    fn poll_result_embed_fields_map_to_poll_summary() {
        let poll = poll_result_info_from_fields([
            ("poll_question_text", "오늘 뭐 먹지?"),
            ("victor_answer_id", "1"),
            ("victor_answer_text", "김치찌개"),
            ("victor_answer_votes", "5"),
            ("total_votes", "7"),
        ])
        .expect("poll result fields should map");

        assert_eq!(poll.question, "오늘 뭐 먹지?");
        assert_eq!(poll.total_votes, Some(7));
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].text, "김치찌개");
        assert_eq!(poll.answers[0].vote_count, Some(5));
    }

    #[test]
    fn current_user_capabilities_are_delivered_as_ui_effect_only() {
        let event = AppEvent::CurrentUserCapabilities {
            can_use_animated_custom_emojis: true,
        };

        assert!(!event.mutates_discord_state());
        assert!(event.needs_effect_delivery());
    }

    fn attachment_info(filename: &str, content_type: Option<&str>) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(1),
            filename: filename.to_owned(),
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: content_type.map(str::to_owned),
            size: 1024,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }
}
