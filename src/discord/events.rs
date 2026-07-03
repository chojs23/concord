use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::ApplicationCommandInfo;
use super::commands::{
    AttachmentDownloadId, DownloadAttachmentSource, ForumPostArchiveState, MediaPlaybackRequestId,
    MessageHistoryAfterMode, MessageSearchPage, MessageSearchQuery, ReactionEmoji,
};
use super::{
    ActivityInfo, AttachmentUpdate, ChannelInfo, CustomEmojiInfo, EmbedInfo, GuildBoostTier,
    GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo, PollInfo, PremiumTier,
    PresenceStatus, ReactionUserInfo, ReadStateInfo, RelationshipInfo, RoleInfo, SnapshotAreas,
    UserProfileInfo, UserSettingsInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo, is_thread_kind,
};

#[cfg(test)]
use super::PollAnswerInfo;

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayDispatchInfo {
    pub event_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUpdateEventFields {
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
    pub sticker_names: Option<Vec<String>>,
    pub mentions: Option<Vec<MentionInfo>>,
    pub mention_everyone: Option<bool>,
    pub mention_roles: Option<Vec<Id<RoleMarker>>>,
    pub flags: Option<u64>,
    pub attachments: AttachmentUpdate,
    pub embeds: Option<Vec<EmbedInfo>>,
    pub edited_timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MessageUpdateDispatchInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub fields: MessageUpdateEventFields,
    pub extra_fields: BTreeMap<String, Value>,
}

impl Default for MessageUpdateEventFields {
    fn default() -> Self {
        Self {
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceEventFields {
    pub user_id: Id<UserMarker>,
    pub status: PresenceStatus,
    pub activities: Vec<ActivityInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserGuildSettingsInfo {
    pub notification_settings: GuildNotificationSettingsInfo,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadListSyncInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_ids: Vec<Id<ChannelMarker>>,
    pub threads: Vec<ChannelInfo>,
    pub thread_members: Vec<Value>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMemberUpdateInfo {
    pub user_id: Id<UserMarker>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadMembersUpdateInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub member_count: Option<u64>,
    pub added_members: Vec<ThreadMemberUpdateInfo>,
    pub added_user_ids: Vec<Id<UserMarker>>,
    pub removed_user_ids: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMemberListUpdateInfo {
    pub guild_id: Id<GuildMarker>,
    pub list_id: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u32>,
    pub members: Vec<MemberInfo>,
    pub presences: Vec<PresenceEventFields>,
    pub groups: Vec<Value>,
    pub ops: Vec<Value>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMembersChunkInfo {
    pub guild_id: Id<GuildMarker>,
    pub members: Vec<MemberInfo>,
    pub presences: Vec<PresenceEventFields>,
    pub chunk_index: Option<u64>,
    pub chunk_count: Option<u64>,
    pub nonce: Option<String>,
    pub not_found: Vec<Id<UserMarker>>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    GatewayDispatchReceived {
        dispatch: GatewayDispatchInfo,
    },
    Ready {
        user: String,
        user_id: Option<Id<UserMarker>>,
    },
    SignedOut,
    CurrentUserCapabilities {
        premium_tier: PremiumTier,
    },
    UserIdentityUpdate {
        user_id: Id<UserMarker>,
        username: String,
        global_name: Option<String>,
        avatar_url: Option<String>,
        is_bot: bool,
    },
    ApplicationCommandsLoaded {
        guild_id: Option<Id<GuildMarker>>,
        commands: Vec<ApplicationCommandInfo>,
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        member_count: Option<u64>,
        /// Snowflake of the guild owner. The owner short-circuits permission
        /// checks (sees every channel regardless of overwrites).
        owner_id: Option<Id<UserMarker>>,
        boost_tier: GuildBoostTier,
        boost_count: u32,
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
        // `Some` only when this GUILD_UPDATE payload actually carried the field,
        // so a rename does not reset a guild's boost state to unboosted.
        boost_tier: Option<GuildBoostTier>,
        boost_count: Option<u32>,
        roles: Option<Vec<RoleInfo>>,
        emojis: Option<Vec<CustomEmojiInfo>>,
    },
    GuildRolesUpdate {
        guild_id: Id<GuildMarker>,
        roles: Vec<RoleInfo>,
    },
    GuildRoleUpsert {
        guild_id: Id<GuildMarker>,
        role: RoleInfo,
    },
    GuildRoleDelete {
        guild_id: Id<GuildMarker>,
        role_id: Id<RoleMarker>,
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
    ThreadListSync {
        sync: ThreadListSyncInfo,
    },
    ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo,
    },
    MessageCreate {
        message: MessageInfo,
    },
    MessageHistoryLoaded {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryRefreshed {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryAfterLoaded {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        messages: Vec<MessageInfo>,
        has_more: bool,
        mode: MessageHistoryAfterMode,
    },
    MessageHistoryAroundLoaded {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
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
        threads: Vec<ChannelInfo>,
        first_messages: Vec<MessageInfo>,
        has_more: bool,
    },
    ForumPostsLoadFailed {
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        message: String,
    },
    MessageSearchLoaded {
        page: MessageSearchPage,
    },
    MessageSearchLoadFailed {
        query: MessageSearchQuery,
        message: String,
    },
    InboxMentionsLoaded {
        request_id: u64,
        messages: Vec<MessageInfo>,
    },
    InboxMentionsLoadFailed {
        request_id: u64,
    },
    InboxChannelMessagesLoaded {
        request_id: u64,
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    InboxChannelMessagesLoadFailed {
        request_id: u64,
        channel_id: Id<ChannelMarker>,
    },
    MessageHistoryLoadFailed {
        channel_id: Id<ChannelMarker>,
        target: MessageHistoryLoadTarget,
        message: String,
    },
    MessageUpdateDispatch {
        update: MessageUpdateDispatchInfo,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    MessageDeleteBulk {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_ids: Vec<Id<MessageMarker>>,
    },
    GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo,
    },
    GuildMembersChunk {
        chunk: GuildMembersChunkInfo,
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
        guild_id: Option<Id<GuildMarker>>,
        presence: PresenceEventFields,
    },
    /// Rich Presence activities published by local apps over the RPC socket. Not a
    /// gateway dispatch: emitted so the profile popup can list detectable apps. It
    /// does not change presence on its own.
    RichPresenceDetected {
        activities: Vec<ActivityInfo>,
    },
    VoiceStateUpdate {
        state: VoiceStateInfo,
    },
    VoiceSpeakingUpdate {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    },
    VoiceServerUpdate {
        server: VoiceServerInfo,
    },
    VoiceConnectionStatusChanged {
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        status: VoiceConnectionStatus,
        message: Option<String>,
    },
    VoiceSound {
        kind: VoiceSoundKind,
    },
    /// A DM or group-DM call ended; every voice state in that channel is dropped.
    CallDelete {
        channel_id: Id<ChannelMarker>,
    },
    /// Discord's TYPING_START dispatch: emitted ~10s before the typing
    /// indicator should expire. The dashboard tracks the latest timestamp
    /// per (channel, user) and shows "X is typing…" while it's fresh.
    TypingStart {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        display_name: Option<String>,
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
    ChannelPinsUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        last_pin_timestamp: Option<String>,
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
        emoji: ReactionEmoji,
        users: Vec<ReactionUserInfo>,
        next_after: Option<Id<UserMarker>>,
        /// The cursor this page was requested with: `None` replaces the emoji's
        /// users (first page), `Some` appends (next page).
        after: Option<Id<UserMarker>>,
    },
    ReactionUsersLoadFailed {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    },
    UserSettingsUpdate {
        settings: UserSettingsInfo,
    },
    UserGuildSettingsInit {
        settings: Vec<UserGuildSettingsInfo>,
    },
    UserGuildSettingsUpdate {
        settings: UserGuildSettingsInfo,
    },
    GatewayError {
        message: String,
    },
    /// A REST action was refused until Discord's CAPTCHA is solved. `action`
    /// labels what was attempted (e.g. "send message"). Shown as a transient
    /// toast, never the gateway-error banner, since the connection is fine.
    CaptchaRequired {
        action: String,
    },
    MediaPlaybackWindowReady {
        request_id: MediaPlaybackRequestId,
        url: String,
    },
    AttachmentDownloadStarted {
        id: AttachmentDownloadId,
        filename: String,
        total_bytes: Option<u64>,
        source: DownloadAttachmentSource,
    },
    AttachmentDownloadProgress {
        id: AttachmentDownloadId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    AttachmentDownloadCompleted {
        id: AttachmentDownloadId,
        path: String,
        source: DownloadAttachmentSource,
    },
    AttachmentDownloadFailed {
        id: AttachmentDownloadId,
        filename: String,
        message: String,
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
    UserProfileUpdateFailed {
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
    GatewayResumed,
    GatewayReidentified,
    GatewayClosed,
    /// Optimistic update for the current user's notification level on a thread,
    /// published by the `SetThreadNotificationLevel` command handler on success.
    ThreadNotificationLevelUpdate {
        channel_id: Id<ChannelMarker>,
        flags: u64,
    },
}

macro_rules! define_app_event_kinds {
    ($($kind:ident: $pattern:pat,)*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum AppEventKind {
            $($kind,)*
        }

        impl AppEvent {
            pub(crate) fn kind(&self) -> AppEventKind {
                match self {
                    $($pattern => AppEventKind::$kind,)*
                }
            }
        }
    };
}

define_app_event_kinds! {
    GatewayDispatchReceived: AppEvent::GatewayDispatchReceived { .. },
    Ready: AppEvent::Ready { .. },
    SignedOut: AppEvent::SignedOut,
    CurrentUserCapabilities: AppEvent::CurrentUserCapabilities { .. },
    UserIdentityUpdate: AppEvent::UserIdentityUpdate { .. },
    ApplicationCommandsLoaded: AppEvent::ApplicationCommandsLoaded { .. },
    GuildCreate: AppEvent::GuildCreate { .. },
    GuildUpdate: AppEvent::GuildUpdate { .. },
    GuildRolesUpdate: AppEvent::GuildRolesUpdate { .. },
    GuildRoleUpsert: AppEvent::GuildRoleUpsert { .. },
    GuildRoleDelete: AppEvent::GuildRoleDelete { .. },
    GuildEmojisUpdate: AppEvent::GuildEmojisUpdate { .. },
    GuildDelete: AppEvent::GuildDelete { .. },
    SelectedGuildChanged: AppEvent::SelectedGuildChanged { .. },
    SelectedMessageChannelChanged: AppEvent::SelectedMessageChannelChanged { .. },
    ChannelUpsert: AppEvent::ChannelUpsert(_),
    ChannelDelete: AppEvent::ChannelDelete { .. },
    ThreadListSync: AppEvent::ThreadListSync { .. },
    ThreadMembersUpdateDispatch: AppEvent::ThreadMembersUpdateDispatch { .. },
    MessageCreate: AppEvent::MessageCreate { .. },
    MessageHistoryLoaded: AppEvent::MessageHistoryLoaded { .. },
    MessageHistoryRefreshed: AppEvent::MessageHistoryRefreshed { .. },
    MessageHistoryAfterLoaded: AppEvent::MessageHistoryAfterLoaded { .. },
    MessageHistoryAroundLoaded: AppEvent::MessageHistoryAroundLoaded { .. },
    ThreadPreviewLoaded: AppEvent::ThreadPreviewLoaded { .. },
    ThreadPreviewLoadFailed: AppEvent::ThreadPreviewLoadFailed { .. },
    ForumPostsLoaded: AppEvent::ForumPostsLoaded { .. },
    ForumPostsLoadFailed: AppEvent::ForumPostsLoadFailed { .. },
    MessageSearchLoaded: AppEvent::MessageSearchLoaded { .. },
    MessageSearchLoadFailed: AppEvent::MessageSearchLoadFailed { .. },
    InboxMentionsLoaded: AppEvent::InboxMentionsLoaded { .. },
    InboxMentionsLoadFailed: AppEvent::InboxMentionsLoadFailed { .. },
    InboxChannelMessagesLoaded: AppEvent::InboxChannelMessagesLoaded { .. },
    InboxChannelMessagesLoadFailed: AppEvent::InboxChannelMessagesLoadFailed { .. },
    MessageHistoryLoadFailed: AppEvent::MessageHistoryLoadFailed { .. },
    MessageUpdateDispatch: AppEvent::MessageUpdateDispatch { .. },
    MessageDelete: AppEvent::MessageDelete { .. },
    MessageDeleteBulk: AppEvent::MessageDeleteBulk { .. },
    GuildMemberListUpdate: AppEvent::GuildMemberListUpdate { .. },
    GuildMembersChunk: AppEvent::GuildMembersChunk { .. },
    GuildMemberUpsert: AppEvent::GuildMemberUpsert { .. },
    GuildMemberAdd: AppEvent::GuildMemberAdd { .. },
    GuildMemberRemove: AppEvent::GuildMemberRemove { .. },
    PresenceUpdate: AppEvent::PresenceUpdate { .. },
    RichPresenceDetected: AppEvent::RichPresenceDetected { .. },
    VoiceStateUpdate: AppEvent::VoiceStateUpdate { .. },
    VoiceSpeakingUpdate: AppEvent::VoiceSpeakingUpdate { .. },
    VoiceServerUpdate: AppEvent::VoiceServerUpdate { .. },
    VoiceConnectionStatusChanged: AppEvent::VoiceConnectionStatusChanged { .. },
    VoiceSound: AppEvent::VoiceSound { .. },
    CallDelete: AppEvent::CallDelete { .. },
    TypingStart: AppEvent::TypingStart { .. },
    CurrentUserReactionAdd: AppEvent::CurrentUserReactionAdd { .. },
    CurrentUserReactionRemove: AppEvent::CurrentUserReactionRemove { .. },
    MessageReactionAdd: AppEvent::MessageReactionAdd { .. },
    MessageReactionRemove: AppEvent::MessageReactionRemove { .. },
    MessageReactionRemoveAll: AppEvent::MessageReactionRemoveAll { .. },
    MessageReactionRemoveEmoji: AppEvent::MessageReactionRemoveEmoji { .. },
    MessagePinnedUpdate: AppEvent::MessagePinnedUpdate { .. },
    ChannelPinsUpdate: AppEvent::ChannelPinsUpdate { .. },
    PinnedMessagesLoaded: AppEvent::PinnedMessagesLoaded { .. },
    PinnedMessagesLoadFailed: AppEvent::PinnedMessagesLoadFailed { .. },
    CurrentUserPollVoteUpdate: AppEvent::CurrentUserPollVoteUpdate { .. },
    ReactionUsersLoaded: AppEvent::ReactionUsersLoaded { .. },
    ReactionUsersLoadFailed: AppEvent::ReactionUsersLoadFailed { .. },
    UserSettingsUpdate: AppEvent::UserSettingsUpdate { .. },
    UserGuildSettingsInit: AppEvent::UserGuildSettingsInit { .. },
    UserGuildSettingsUpdate: AppEvent::UserGuildSettingsUpdate { .. },
    GatewayError: AppEvent::GatewayError { .. },
    CaptchaRequired: AppEvent::CaptchaRequired { .. },
    ThreadNotificationLevelUpdate: AppEvent::ThreadNotificationLevelUpdate { .. },
    MediaPlaybackWindowReady: AppEvent::MediaPlaybackWindowReady { .. },
    AttachmentDownloadStarted: AppEvent::AttachmentDownloadStarted { .. },
    AttachmentDownloadProgress: AppEvent::AttachmentDownloadProgress { .. },
    AttachmentDownloadCompleted: AppEvent::AttachmentDownloadCompleted { .. },
    AttachmentDownloadFailed: AppEvent::AttachmentDownloadFailed { .. },
    UpdateAvailable: AppEvent::UpdateAvailable { .. },
    AttachmentPreviewLoaded: AppEvent::AttachmentPreviewLoaded { .. },
    AttachmentPreviewLoadFailed: AppEvent::AttachmentPreviewLoadFailed { .. },
    UserProfileLoaded: AppEvent::UserProfileLoaded { .. },
    UserProfileLoadFailed: AppEvent::UserProfileLoadFailed { .. },
    UserProfileUpdateFailed: AppEvent::UserProfileUpdateFailed { .. },
    UserNoteLoaded: AppEvent::UserNoteLoaded { .. },
    RelationshipsLoaded: AppEvent::RelationshipsLoaded { .. },
    RelationshipUpsert: AppEvent::RelationshipUpsert { .. },
    RelationshipRemove: AppEvent::RelationshipRemove { .. },
    ActivateChannel: AppEvent::ActivateChannel { .. },
    ReadStateInit: AppEvent::ReadStateInit { .. },
    MessageAck: AppEvent::MessageAck { .. },
    GatewayResumed: AppEvent::GatewayResumed,
    GatewayReidentified: AppEvent::GatewayReidentified,
    GatewayClosed: AppEvent::GatewayClosed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHistoryLoadTarget {
    Latest,
    Older { before: Id<MessageMarker> },
    Newer { after: Id<MessageMarker> },
    Around { message_id: Id<MessageMarker> },
}

#[cfg(test)]
pub(crate) mod test_builders {
    use crate::discord::{AttachmentInfo, MessageKind, MessageReferenceInfo};

    use super::*;

    pub(crate) type MessageCreateFixture = MessageInfo;

    impl MessageCreateFixture {
        pub(crate) fn test_fixture_default() -> Self {
            Self {
                channel_id: Id::new(2),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                message_kind: MessageKind::regular(),
                content: Some("hello".to_owned()),
                ..Self::default()
            }
        }

        pub(crate) fn direct_message(
            channel_id: Id<ChannelMarker>,
            message_id: Id<MessageMarker>,
        ) -> Self {
            Self {
                channel_id,
                message_id,
                ..Self::test_fixture_default()
            }
        }

        pub(crate) fn guild_message(
            guild_id: Id<GuildMarker>,
            channel_id: Id<ChannelMarker>,
            message_id: Id<MessageMarker>,
        ) -> Self {
            Self {
                guild_id: Some(guild_id),
                channel_id,
                message_id,
                ..Self::test_fixture_default()
            }
        }

        pub(crate) fn with_author_id(mut self, author_id: Id<UserMarker>) -> Self {
            self.author_id = author_id;
            self
        }

        pub(crate) fn with_author(
            mut self,
            author_id: Id<UserMarker>,
            author: impl Into<String>,
        ) -> Self {
            self.author_id = author_id;
            self.author = author.into();
            self
        }

        pub(crate) fn with_message_kind(mut self, message_kind: MessageKind) -> Self {
            self.message_kind = message_kind;
            self
        }

        pub(crate) fn with_reference(mut self, reference: MessageReferenceInfo) -> Self {
            self.reference = Some(reference);
            self
        }

        pub(crate) fn with_attachments(mut self, attachments: Vec<AttachmentInfo>) -> Self {
            self.attachments = attachments;
            self
        }

        pub(crate) fn with_content(mut self, content: impl Into<String>) -> Self {
            self.content = Some(content.into());
            self
        }

        pub(crate) fn without_content(mut self) -> Self {
            self.content = None;
            self
        }
    }

    pub(crate) fn guild_message_create_fixture() -> MessageCreateFixture {
        MessageCreateFixture::guild_message(Id::new(1), Id::new(2), Id::new(1))
    }

    pub(crate) fn message_create_event(event: MessageCreateFixture) -> AppEvent {
        AppEvent::MessageCreate { message: event }
    }

    use crate::discord::{
        ChannelInfo, CustomEmojiInfo, GuildBoostTier, MemberInfo, PresenceStatus, RoleInfo,
    };

    // Single construction seam for `AppEvent::GuildCreate` so a new field on the
    // variant only touches this fixture, not the ~20 test files that build the event.
    pub(crate) struct GuildCreateFixture {
        pub(crate) guild_id: Id<GuildMarker>,
        pub(crate) name: String,
        pub(crate) member_count: Option<u64>,
        pub(crate) owner_id: Option<Id<UserMarker>>,
        pub(crate) boost_tier: GuildBoostTier,
        pub(crate) boost_count: u32,
        pub(crate) channels: Vec<ChannelInfo>,
        pub(crate) members: Vec<MemberInfo>,
        pub(crate) presences: Vec<(Id<UserMarker>, PresenceStatus)>,
        pub(crate) roles: Vec<RoleInfo>,
        pub(crate) emojis: Vec<CustomEmojiInfo>,
    }

    impl GuildCreateFixture {
        pub(crate) fn new(guild_id: Id<GuildMarker>) -> Self {
            Self {
                guild_id,
                name: "guild".to_owned(),
                member_count: None,
                owner_id: None,
                boost_tier: GuildBoostTier::None,
                boost_count: 0,
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
            }
        }
    }

    pub(crate) fn guild_create_event(event: GuildCreateFixture) -> AppEvent {
        AppEvent::GuildCreate {
            guild_id: event.guild_id,
            name: event.name,
            member_count: event.member_count,
            owner_id: event.owner_id,
            boost_tier: event.boost_tier,
            boost_count: event.boost_count,
            channels: event.channels,
            members: event.members,
            presences: event.presences,
            roles: event.roles,
            emojis: event.emojis,
        }
    }

    pub(crate) struct ForumPostsLoadedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) archive_state: ForumPostArchiveState,
        pub(crate) offset: usize,
        pub(crate) next_offset: usize,
        pub(crate) threads: Vec<ChannelInfo>,
        pub(crate) first_messages: Vec<MessageInfo>,
        pub(crate) has_more: bool,
    }

    impl ForumPostsLoadedFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                archive_state: ForumPostArchiveState::default(),
                offset: 0,
                next_offset: 0,
                threads: Vec::new(),
                first_messages: Vec::new(),
                has_more: false,
            }
        }
    }

    pub(crate) fn forum_posts_loaded_event(f: ForumPostsLoadedFixture) -> AppEvent {
        AppEvent::ForumPostsLoaded {
            channel_id: f.channel_id,
            archive_state: f.archive_state,
            offset: f.offset,
            next_offset: f.next_offset,
            threads: f.threads,
            first_messages: f.first_messages,
            has_more: f.has_more,
        }
    }

    pub(crate) struct MessageHistoryLoadedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) before: Option<Id<MessageMarker>>,
        pub(crate) messages: Vec<MessageInfo>,
    }

    impl MessageHistoryLoadedFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                before: None,
                messages: Vec::new(),
            }
        }
    }

    pub(crate) fn message_history_loaded_event(f: MessageHistoryLoadedFixture) -> AppEvent {
        AppEvent::MessageHistoryLoaded {
            channel_id: f.channel_id,
            before: f.before,
            messages: f.messages,
        }
    }

    pub(crate) struct MessageHistoryLoadFailedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) target: MessageHistoryLoadTarget,
        pub(crate) message: String,
    }
    pub(crate) fn message_history_load_failed_event(
        f: MessageHistoryLoadFailedFixture,
    ) -> AppEvent {
        AppEvent::MessageHistoryLoadFailed {
            channel_id: f.channel_id,
            target: f.target,
            message: f.message,
        }
    }

    pub(crate) struct TypingStartFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) display_name: Option<String>,
    }

    impl TypingStartFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                user_id: Id::new(1),
                display_name: None,
            }
        }
    }

    pub(crate) fn typing_start_event(f: TypingStartFixture) -> AppEvent {
        AppEvent::TypingStart {
            channel_id: f.channel_id,
            user_id: f.user_id,
            display_name: f.display_name,
        }
    }

    pub(crate) struct VoiceSpeakingUpdateFixture {
        pub(crate) scope: VoiceScope,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) speaking: bool,
    }
    pub(crate) fn voice_speaking_update_event(f: VoiceSpeakingUpdateFixture) -> AppEvent {
        AppEvent::VoiceSpeakingUpdate {
            scope: f.scope,
            channel_id: f.channel_id,
            user_id: f.user_id,
            speaking: f.speaking,
        }
    }

    pub(crate) struct MessageHistoryAfterLoadedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) after: Id<MessageMarker>,
        pub(crate) messages: Vec<MessageInfo>,
        pub(crate) has_more: bool,
        pub(crate) mode: MessageHistoryAfterMode,
    }

    impl MessageHistoryAfterLoadedFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                after: Id::new(1),
                messages: Vec::new(),
                has_more: false,
                mode: MessageHistoryAfterMode::GapFill,
            }
        }
    }

    pub(crate) fn message_history_after_loaded_event(
        f: MessageHistoryAfterLoadedFixture,
    ) -> AppEvent {
        AppEvent::MessageHistoryAfterLoaded {
            channel_id: f.channel_id,
            after: f.after,
            messages: f.messages,
            has_more: f.has_more,
            mode: f.mode,
        }
    }

    pub(crate) struct VoiceConnectionStatusChangedFixture {
        pub(crate) scope: VoiceScope,
        pub(crate) channel_id: Option<Id<ChannelMarker>>,
        pub(crate) status: VoiceConnectionStatus,
        pub(crate) message: Option<String>,
    }

    impl VoiceConnectionStatusChangedFixture {
        pub(crate) fn new() -> Self {
            Self {
                scope: VoiceScope::Guild(Id::new(1)),
                channel_id: None,
                status: VoiceConnectionStatus::Connecting,
                message: None,
            }
        }
    }

    pub(crate) fn voice_connection_status_changed_event(
        f: VoiceConnectionStatusChangedFixture,
    ) -> AppEvent {
        AppEvent::VoiceConnectionStatusChanged {
            scope: f.scope,
            channel_id: f.channel_id,
            status: f.status,
            message: f.message,
        }
    }

    pub(crate) struct MessageReactionAddFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) emoji: ReactionEmoji,
    }

    impl MessageReactionAddFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                user_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub(crate) fn message_reaction_add_event(f: MessageReactionAddFixture) -> AppEvent {
        AppEvent::MessageReactionAdd {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            user_id: f.user_id,
            emoji: f.emoji,
        }
    }

    pub(crate) struct ChannelPinsUpdateFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) last_pin_timestamp: Option<String>,
    }

    impl ChannelPinsUpdateFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                last_pin_timestamp: None,
            }
        }
    }

    pub(crate) fn channel_pins_update_event(f: ChannelPinsUpdateFixture) -> AppEvent {
        AppEvent::ChannelPinsUpdate {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            last_pin_timestamp: f.last_pin_timestamp,
        }
    }

    pub(crate) struct UserProfileLoadFailedFixture {
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) message: String,
    }

    impl UserProfileLoadFailedFixture {
        pub(crate) fn new() -> Self {
            Self {
                user_id: Id::new(1),
                guild_id: None,
                message: String::new(),
            }
        }
    }

    pub(crate) fn user_profile_load_failed_event(f: UserProfileLoadFailedFixture) -> AppEvent {
        AppEvent::UserProfileLoadFailed {
            user_id: f.user_id,
            guild_id: f.guild_id,
            message: f.message,
        }
    }

    pub(crate) struct MessageAckFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) mention_count: u32,
    }

    impl MessageAckFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                mention_count: 0,
            }
        }
    }

    pub(crate) fn message_ack_event(f: MessageAckFixture) -> AppEvent {
        AppEvent::MessageAck {
            channel_id: f.channel_id,
            message_id: f.message_id,
            mention_count: f.mention_count,
        }
    }

    pub(crate) struct ReactionUsersLoadedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) emoji: ReactionEmoji,
        pub(crate) users: Vec<ReactionUserInfo>,
        pub(crate) next_after: Option<Id<UserMarker>>,
        pub(crate) after: Option<Id<UserMarker>>,
    }
    pub(crate) fn reaction_users_loaded_event(f: ReactionUsersLoadedFixture) -> AppEvent {
        AppEvent::ReactionUsersLoaded {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
            users: f.users,
            next_after: f.next_after,
            after: f.after,
        }
    }

    pub(crate) struct CurrentUserPollVoteUpdateFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) answer_ids: Vec<u8>,
    }

    impl CurrentUserPollVoteUpdateFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                answer_ids: Vec::new(),
            }
        }
    }

    pub(crate) fn current_user_poll_vote_update_event(
        f: CurrentUserPollVoteUpdateFixture,
    ) -> AppEvent {
        AppEvent::CurrentUserPollVoteUpdate {
            channel_id: f.channel_id,
            message_id: f.message_id,
            answer_ids: f.answer_ids,
        }
    }

    pub(crate) struct UserIdentityUpdateFixture {
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) username: String,
        pub(crate) global_name: Option<String>,
        pub(crate) avatar_url: Option<String>,
        pub(crate) is_bot: bool,
    }

    impl UserIdentityUpdateFixture {
        pub(crate) fn new() -> Self {
            Self {
                user_id: Id::new(1),
                username: String::new(),
                global_name: None,
                avatar_url: None,
                is_bot: false,
            }
        }
    }

    pub(crate) fn user_identity_update_event(f: UserIdentityUpdateFixture) -> AppEvent {
        AppEvent::UserIdentityUpdate {
            user_id: f.user_id,
            username: f.username,
            global_name: f.global_name,
            avatar_url: f.avatar_url,
            is_bot: f.is_bot,
        }
    }

    pub(crate) struct MessagePinnedUpdateFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) pinned: bool,
    }

    impl MessagePinnedUpdateFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                message_id: Id::new(1),
                pinned: false,
            }
        }
    }

    pub(crate) fn message_pinned_update_event(f: MessagePinnedUpdateFixture) -> AppEvent {
        AppEvent::MessagePinnedUpdate {
            channel_id: f.channel_id,
            message_id: f.message_id,
            pinned: f.pinned,
        }
    }

    pub(crate) struct MessageHistoryAroundLoadedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) messages: Vec<MessageInfo>,
    }
    pub(crate) fn message_history_around_loaded_event(
        f: MessageHistoryAroundLoadedFixture,
    ) -> AppEvent {
        AppEvent::MessageHistoryAroundLoaded {
            channel_id: f.channel_id,
            message_id: f.message_id,
            messages: f.messages,
        }
    }

    pub(crate) struct CurrentUserReactionAddFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) emoji: ReactionEmoji,
    }
    pub(crate) fn current_user_reaction_add_event(f: CurrentUserReactionAddFixture) -> AppEvent {
        AppEvent::CurrentUserReactionAdd {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub(crate) struct GuildUpdateFixture {
        pub(crate) guild_id: Id<GuildMarker>,
        pub(crate) name: String,
        pub(crate) owner_id: Option<Id<UserMarker>>,
        pub(crate) boost_tier: Option<GuildBoostTier>,
        pub(crate) boost_count: Option<u32>,
        pub(crate) roles: Option<Vec<RoleInfo>>,
        pub(crate) emojis: Option<Vec<CustomEmojiInfo>>,
    }

    impl GuildUpdateFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: Id::new(1),
                name: String::new(),
                owner_id: None,
                boost_tier: None,
                boost_count: None,
                roles: None,
                emojis: None,
            }
        }
    }

    pub(crate) fn guild_update_event(f: GuildUpdateFixture) -> AppEvent {
        AppEvent::GuildUpdate {
            guild_id: f.guild_id,
            name: f.name,
            owner_id: f.owner_id,
            boost_tier: f.boost_tier,
            boost_count: f.boost_count,
            roles: f.roles,
            emojis: f.emojis,
        }
    }

    pub(crate) struct MessageReactionRemoveFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) emoji: ReactionEmoji,
    }

    impl MessageReactionRemoveFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                user_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub(crate) fn message_reaction_remove_event(f: MessageReactionRemoveFixture) -> AppEvent {
        AppEvent::MessageReactionRemove {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            user_id: f.user_id,
            emoji: f.emoji,
        }
    }

    pub(crate) struct AttachmentDownloadStartedFixture {
        pub(crate) id: AttachmentDownloadId,
        pub(crate) filename: String,
        pub(crate) total_bytes: Option<u64>,
        pub(crate) source: DownloadAttachmentSource,
    }

    impl AttachmentDownloadStartedFixture {
        pub(crate) fn new() -> Self {
            Self {
                id: AttachmentDownloadId::new(0),
                filename: String::new(),
                total_bytes: None,
                source: DownloadAttachmentSource::AttachmentViewer,
            }
        }
    }

    pub(crate) fn attachment_download_started_event(
        f: AttachmentDownloadStartedFixture,
    ) -> AppEvent {
        AppEvent::AttachmentDownloadStarted {
            id: f.id,
            filename: f.filename,
            total_bytes: f.total_bytes,
            source: f.source,
        }
    }

    pub(crate) struct MessageReactionRemoveAllFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
    }

    impl MessageReactionRemoveAllFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
            }
        }
    }

    pub(crate) fn message_reaction_remove_all_event(
        f: MessageReactionRemoveAllFixture,
    ) -> AppEvent {
        AppEvent::MessageReactionRemoveAll {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
        }
    }

    pub(crate) struct MessageDeleteBulkFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_ids: Vec<Id<MessageMarker>>,
    }
    pub(crate) fn message_delete_bulk_event(f: MessageDeleteBulkFixture) -> AppEvent {
        AppEvent::MessageDeleteBulk {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_ids: f.message_ids,
        }
    }

    pub(crate) struct CurrentUserReactionRemoveFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) emoji: ReactionEmoji,
    }
    pub(crate) fn current_user_reaction_remove_event(
        f: CurrentUserReactionRemoveFixture,
    ) -> AppEvent {
        AppEvent::CurrentUserReactionRemove {
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub(crate) struct AttachmentDownloadProgressFixture {
        pub(crate) id: AttachmentDownloadId,
        pub(crate) downloaded_bytes: u64,
        pub(crate) total_bytes: Option<u64>,
    }
    pub(crate) fn attachment_download_progress_event(
        f: AttachmentDownloadProgressFixture,
    ) -> AppEvent {
        AppEvent::AttachmentDownloadProgress {
            id: f.id,
            downloaded_bytes: f.downloaded_bytes,
            total_bytes: f.total_bytes,
        }
    }

    pub(crate) struct MessageReactionRemoveEmojiFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) emoji: ReactionEmoji,
    }

    impl MessageReactionRemoveEmojiFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                message_id: Id::new(1),
                emoji: ReactionEmoji::Unicode(String::new()),
            }
        }
    }

    pub(crate) fn message_reaction_remove_emoji_event(
        f: MessageReactionRemoveEmojiFixture,
    ) -> AppEvent {
        AppEvent::MessageReactionRemoveEmoji {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            message_id: f.message_id,
            emoji: f.emoji,
        }
    }

    pub(crate) struct ForumPostsLoadFailedFixture {
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) archive_state: ForumPostArchiveState,
        pub(crate) offset: usize,
        pub(crate) message: String,
    }

    impl ForumPostsLoadFailedFixture {
        pub(crate) fn new() -> Self {
            Self {
                channel_id: Id::new(1),
                archive_state: ForumPostArchiveState::default(),
                offset: 0,
                message: String::new(),
            }
        }
    }

    pub(crate) fn forum_posts_load_failed_event(f: ForumPostsLoadFailedFixture) -> AppEvent {
        AppEvent::ForumPostsLoadFailed {
            channel_id: f.channel_id,
            archive_state: f.archive_state,
            offset: f.offset,
            message: f.message,
        }
    }

    pub(crate) struct AttachmentDownloadFailedFixture {
        pub(crate) id: AttachmentDownloadId,
        pub(crate) filename: String,
        pub(crate) message: String,
        pub(crate) source: DownloadAttachmentSource,
    }
    pub(crate) fn attachment_download_failed_event(f: AttachmentDownloadFailedFixture) -> AppEvent {
        AppEvent::AttachmentDownloadFailed {
            id: f.id,
            filename: f.filename,
            message: f.message,
            source: f.source,
        }
    }
    pub(crate) struct AttachmentDownloadCompletedFixture {
        pub(crate) id: AttachmentDownloadId,
        pub(crate) path: String,
        pub(crate) source: DownloadAttachmentSource,
    }
    pub(crate) fn attachment_download_completed_event(
        f: AttachmentDownloadCompletedFixture,
    ) -> AppEvent {
        AppEvent::AttachmentDownloadCompleted {
            id: f.id,
            path: f.path,
            source: f.source,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SequencedAppEvent {
    pub revision: u64,
    pub event: AppEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppEventMetadata {
    pub(crate) mutates_discord_state: bool,
    pub(crate) needs_effect_delivery: bool,
    pub(crate) snapshot_areas: Option<SnapshotAreas>,
}

impl AppEventMetadata {
    const fn mutating(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            mutates_discord_state: true,
            needs_effect_delivery: false,
            snapshot_areas: Some(snapshot_areas),
        }
    }

    const fn mutating_effect(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            mutates_discord_state: true,
            needs_effect_delivery: true,
            snapshot_areas: Some(snapshot_areas),
        }
    }

    const fn effect_only() -> Self {
        Self {
            mutates_discord_state: false,
            needs_effect_delivery: true,
            snapshot_areas: None,
        }
    }

    const fn inert() -> Self {
        Self {
            mutates_discord_state: false,
            needs_effect_delivery: false,
            snapshot_areas: None,
        }
    }
}

impl AppEventKind {
    const fn metadata(self) -> AppEventMetadata {
        match self {
            AppEventKind::GuildCreate
            | AppEventKind::GuildUpdate
            | AppEventKind::GuildDelete
            | AppEventKind::ThreadListSync
            | AppEventKind::ThreadMembersUpdateDispatch
            | AppEventKind::ChannelUpsert
            | AppEventKind::ChannelDelete
            | AppEventKind::Ready => AppEventMetadata::mutating(SnapshotAreas::all()),

            AppEventKind::ForumPostsLoaded => {
                AppEventMetadata::mutating_effect(SnapshotAreas::all())
            }

            AppEventKind::MessageCreate => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::MessageHistoryLoaded
            | AppEventKind::MessageHistoryRefreshed
            | AppEventKind::MessageHistoryAfterLoaded
            | AppEventKind::MessageHistoryAroundLoaded
            | AppEventKind::MessageSearchLoaded
            | AppEventKind::ThreadPreviewLoaded
            | AppEventKind::PinnedMessagesLoaded => {
                AppEventMetadata::mutating_effect(SnapshotAreas::message())
            }

            AppEventKind::MessageUpdateDispatch
            | AppEventKind::CurrentUserReactionAdd
            | AppEventKind::CurrentUserReactionRemove
            | AppEventKind::MessageReactionAdd
            | AppEventKind::MessageReactionRemove
            | AppEventKind::MessageReactionRemoveAll
            | AppEventKind::MessageReactionRemoveEmoji
            | AppEventKind::MessagePinnedUpdate
            | AppEventKind::ChannelPinsUpdate
            | AppEventKind::CurrentUserPollVoteUpdate
            | AppEventKind::MessageDelete
            | AppEventKind::MessageDeleteBulk => {
                AppEventMetadata::mutating(SnapshotAreas::message())
            }

            AppEventKind::SelectedMessageChannelChanged => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::UserProfileLoaded => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::GuildMemberAdd
            | AppEventKind::GuildMemberUpsert
            | AppEventKind::RelationshipsLoaded
            | AppEventKind::RelationshipUpsert
            | AppEventKind::UserIdentityUpdate
            | AppEventKind::RelationshipRemove => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::SelectedGuildChanged
            | AppEventKind::GuildRolesUpdate
            | AppEventKind::GuildRoleUpsert
            | AppEventKind::GuildRoleDelete
            | AppEventKind::GuildEmojisUpdate
            | AppEventKind::GuildMemberListUpdate
            | AppEventKind::GuildMembersChunk
            | AppEventKind::GuildMemberRemove
            | AppEventKind::PresenceUpdate
            | AppEventKind::VoiceStateUpdate
            | AppEventKind::VoiceSpeakingUpdate
            | AppEventKind::CallDelete
            | AppEventKind::TypingStart
            | AppEventKind::UserSettingsUpdate
            | AppEventKind::UserNoteLoaded
            | AppEventKind::UserGuildSettingsInit
            | AppEventKind::UserGuildSettingsUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation())
            }

            AppEventKind::ReadStateInit | AppEventKind::MessageAck => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_detail())
            }

            AppEventKind::GatewayError
            | AppEventKind::CaptchaRequired
            | AppEventKind::GatewayDispatchReceived
            | AppEventKind::SignedOut
            | AppEventKind::MediaPlaybackWindowReady
            | AppEventKind::ApplicationCommandsLoaded
            | AppEventKind::AttachmentDownloadStarted
            | AppEventKind::AttachmentDownloadProgress
            | AppEventKind::AttachmentDownloadCompleted
            | AppEventKind::AttachmentDownloadFailed
            | AppEventKind::UpdateAvailable
            | AppEventKind::ReactionUsersLoaded
            | AppEventKind::ReactionUsersLoadFailed
            | AppEventKind::AttachmentPreviewLoaded
            | AppEventKind::AttachmentPreviewLoadFailed
            | AppEventKind::ThreadPreviewLoadFailed
            | AppEventKind::ForumPostsLoadFailed
            | AppEventKind::MessageSearchLoadFailed
            | AppEventKind::MessageHistoryLoadFailed
            | AppEventKind::InboxMentionsLoaded
            | AppEventKind::InboxMentionsLoadFailed
            | AppEventKind::InboxChannelMessagesLoaded
            | AppEventKind::InboxChannelMessagesLoadFailed
            | AppEventKind::PinnedMessagesLoadFailed
            | AppEventKind::UserProfileLoadFailed
            | AppEventKind::UserProfileUpdateFailed
            | AppEventKind::VoiceConnectionStatusChanged
            | AppEventKind::VoiceSound
            | AppEventKind::ActivateChannel
            | AppEventKind::RichPresenceDetected
            | AppEventKind::GatewayResumed
            | AppEventKind::GatewayReidentified
            | AppEventKind::GatewayClosed => AppEventMetadata::effect_only(),

            AppEventKind::VoiceServerUpdate => AppEventMetadata::inert(),

            // The current user's Nitro tier is stored in the session (part of
            // the navigation snapshot area) so the upload-limit check can read
            // it, and it still needs effect delivery so the TUI can update
            // Nitro-gated UI such as the emoji picker.
            AppEventKind::CurrentUserCapabilities => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }

            AppEventKind::ThreadNotificationLevelUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation())
            }
        }
    }
}

impl AppEvent {
    pub(crate) fn metadata(&self) -> AppEventMetadata {
        match self {
            AppEvent::ChannelUpsert(channel) if channel_upsert_needs_effect_delivery(channel) => {
                AppEventMetadata::mutating_effect(SnapshotAreas::all())
            }
            _ => self.kind().metadata(),
        }
    }

    pub fn mutates_discord_state(&self) -> bool {
        self.metadata().mutates_discord_state
    }

    pub fn needs_effect_delivery(&self) -> bool {
        self.metadata().needs_effect_delivery
    }

    pub(crate) fn snapshot_areas(&self) -> Option<SnapshotAreas> {
        self.metadata().snapshot_areas
    }
}

fn channel_upsert_needs_effect_delivery(channel: &ChannelInfo) -> bool {
    channel.parent_id.is_some() && is_thread_kind(&channel.kind)
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
        answers,
        results_finalized: Some(true),
        total_votes,
        ..PollInfo::test(question)
    })
}

#[cfg(test)]
mod tests {
    use crate::discord::{AttachmentInfo, AttachmentMediaType};

    use super::*;

    #[test]
    fn attachment_media_classification_controls_inline_preview() {
        let video = attachment_info("clip.mp4", Some("video/mp4"));
        assert!(video.media_type() == Some(AttachmentMediaType::Video));
        assert_eq!(video.inline_preview_url(), None);
        assert_eq!(
            video.inline_preview_info().map(|info| (
                info.url,
                info.proxy_url,
                info.proxy_preview_only,
            )),
            Some((
                "https://media.discordapp.net/clip.mp4",
                Some("https://media.discordapp.net/clip.mp4"),
                true,
            ))
        );

        let image = attachment_info("cat.png", Some("image/png"));
        assert!(image.media_type() == Some(AttachmentMediaType::Image));
        assert_eq!(
            image.inline_preview_url(),
            Some("https://cdn.discordapp.com/cat.png")
        );
        assert_eq!(
            image.inline_preview_info().and_then(|info| info.proxy_url),
            Some("https://media.discordapp.net/cat.png")
        );

        assert!(attachment_info("CAT.PNG", None).media_type() == Some(AttachmentMediaType::Image));
        assert!(attachment_info("CLIP.MP4", None).media_type() == Some(AttachmentMediaType::Video));
        assert!(
            attachment_info("MUSIC.MP3", None).media_type() == Some(AttachmentMediaType::Audio)
        );
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
    fn current_user_capabilities_mutate_state_and_deliver_ui_effect() {
        let event = AppEvent::CurrentUserCapabilities {
            premium_tier: PremiumTier::Nitro,
        };

        assert!(event.mutates_discord_state());
        assert!(event.needs_effect_delivery());
    }

    #[test]
    fn message_delete_bulk_is_snapshot_driven_state_mutation() {
        let event = AppEvent::MessageDeleteBulk {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            message_ids: vec![Id::new(20), Id::new(30)],
        };

        assert!(event.mutates_discord_state());
        assert!(!event.needs_effect_delivery());
    }

    fn attachment_info(filename: &str, content_type: Option<&str>) -> AttachmentInfo {
        AttachmentInfo {
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: content_type.map(str::to_owned),
            size: 1024,
            width: Some(640),
            height: Some(480),
            ..AttachmentInfo::test(Id::new(1), filename)
        }
    }
}
