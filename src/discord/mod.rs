mod auth_http;
mod client;
mod commands;
mod display_name;
mod events;
mod fingerprint;
mod gateway;
pub mod ids;
pub mod password_auth;
pub mod qr_auth;
mod rest;
mod state;
mod voice;

pub use client::DiscordClient;
pub(crate) use client::validate_token_header;
pub use commands::{AppCommand, DownloadAttachmentSource, ForumPostArchiveState, MuteDuration};
pub use commands::{
    MAX_UPLOAD_ATTACHMENT_COUNT, MAX_UPLOAD_FILE_BYTES, MAX_UPLOAD_TOTAL_BYTES,
    MessageAttachmentUpload, ReactionEmoji,
};
pub use events::{
    ActivityEmoji, ActivityInfo, ActivityKind, AppEvent, AttachmentInfo, AttachmentUpdate,
    ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo, CustomEmojiInfo,
    EmbedFieldInfo, EmbedInfo, FriendStatus, GuildFolder, GuildNotificationSettingsInfo,
    InlinePreviewInfo, MemberInfo, MentionInfo, MessageInfo, MessageKind, MessageReferenceInfo,
    MessageSnapshotInfo, MutualGuildInfo, NotificationLevel, PermissionOverwriteInfo,
    PermissionOverwriteKind, PollAnswerInfo, PollInfo, PresenceStatus, ReactionInfo,
    ReactionUserInfo, ReactionUsersInfo, ReadStateInfo, RelationshipInfo, ReplyInfo, RoleInfo,
    SequencedAppEvent, UserProfileInfo, VoiceConnectionStatus, VoiceServerInfo, VoiceSoundKind,
    VoiceStateInfo,
};
pub use ids::{Id, marker};
pub use rest::ForumPostPage;
pub use state::{
    ChannelRecipientState, ChannelState, ChannelUnreadState, ChannelVisibilityStats,
    CurrentVoiceConnectionState, DiscordSnapshot, DiscordState, GuildMemberState, GuildState,
    MessageCapabilities, MessageState, RoleState, SnapshotAreas, SnapshotRevision,
    VoiceParticipantState,
};
