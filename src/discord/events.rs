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
    ActivityInfo, AttachmentUpdate, ChannelInfo, CustomEmojiInfo, EmbedInfo, GuildFolder,
    GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo, PollInfo, PresenceStatus,
    ReactionUsersInfo, ReadStateInfo, RelationshipInfo, RoleInfo, SnapshotAreas, UserProfileInfo,
    VoiceConnectionStatus, VoiceServerInfo, VoiceSoundKind, VoiceStateInfo, is_thread_kind,
};

#[cfg(test)]
use super::PollAnswerInfo;

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

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
        user_id: Option<Id<UserMarker>>,
    },
    CurrentUserCapabilities {
        has_nitro: bool,
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
    ThreadMembersUpdate {
        channel_id: Id<ChannelMarker>,
        added_user_ids: Vec<Id<UserMarker>>,
        removed_user_ids: Vec<Id<UserMarker>>,
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
    MessageHistoryLoadFailed {
        channel_id: Id<ChannelMarker>,
        target: MessageHistoryLoadTarget,
        message: String,
    },
    MessageUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        fields: MessageUpdateEventFields,
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
        guild_id: Option<Id<GuildMarker>>,
        presence: PresenceEventFields,
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
    VoiceSound {
        kind: VoiceSoundKind,
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
    Ready: AppEvent::Ready { .. },
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
    ThreadMembersUpdate: AppEvent::ThreadMembersUpdate { .. },
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
    MessageHistoryLoadFailed: AppEvent::MessageHistoryLoadFailed { .. },
    MessageUpdate: AppEvent::MessageUpdate { .. },
    MessageDelete: AppEvent::MessageDelete { .. },
    MessageDeleteBulk: AppEvent::MessageDeleteBulk { .. },
    GuildMemberListCounts: AppEvent::GuildMemberListCounts { .. },
    GuildMemberUpsert: AppEvent::GuildMemberUpsert { .. },
    GuildMemberAdd: AppEvent::GuildMemberAdd { .. },
    GuildMemberRemove: AppEvent::GuildMemberRemove { .. },
    PresenceUpdate: AppEvent::PresenceUpdate { .. },
    VoiceStateUpdate: AppEvent::VoiceStateUpdate { .. },
    VoiceSpeakingUpdate: AppEvent::VoiceSpeakingUpdate { .. },
    VoiceServerUpdate: AppEvent::VoiceServerUpdate { .. },
    VoiceConnectionStatusChanged: AppEvent::VoiceConnectionStatusChanged { .. },
    VoiceSound: AppEvent::VoiceSound { .. },
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
    GuildFoldersUpdate: AppEvent::GuildFoldersUpdate { .. },
    UserGuildNotificationSettingsInit: AppEvent::UserGuildNotificationSettingsInit { .. },
    UserGuildNotificationSettingsUpdate: AppEvent::UserGuildNotificationSettingsUpdate { .. },
    GatewayError: AppEvent::GatewayError { .. },
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
            | AppEventKind::ThreadMembersUpdate
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

            AppEventKind::MessageUpdate
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
            | AppEventKind::GuildMemberListCounts
            | AppEventKind::GuildMemberRemove
            | AppEventKind::PresenceUpdate
            | AppEventKind::VoiceStateUpdate
            | AppEventKind::VoiceSpeakingUpdate
            | AppEventKind::TypingStart
            | AppEventKind::GuildFoldersUpdate
            | AppEventKind::UserNoteLoaded
            | AppEventKind::UserGuildNotificationSettingsInit
            | AppEventKind::UserGuildNotificationSettingsUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation())
            }

            AppEventKind::ReadStateInit | AppEventKind::MessageAck => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_detail())
            }

            AppEventKind::GatewayError
            | AppEventKind::MediaPlaybackWindowReady
            | AppEventKind::CurrentUserCapabilities
            | AppEventKind::ApplicationCommandsLoaded
            | AppEventKind::AttachmentDownloadStarted
            | AppEventKind::AttachmentDownloadProgress
            | AppEventKind::AttachmentDownloadCompleted
            | AppEventKind::AttachmentDownloadFailed
            | AppEventKind::UpdateAvailable
            | AppEventKind::ReactionUsersLoaded
            | AppEventKind::AttachmentPreviewLoaded
            | AppEventKind::AttachmentPreviewLoadFailed
            | AppEventKind::ThreadPreviewLoadFailed
            | AppEventKind::ForumPostsLoadFailed
            | AppEventKind::MessageSearchLoadFailed
            | AppEventKind::MessageHistoryLoadFailed
            | AppEventKind::PinnedMessagesLoadFailed
            | AppEventKind::UserProfileLoadFailed
            | AppEventKind::UserProfileUpdateFailed
            | AppEventKind::VoiceConnectionStatusChanged
            | AppEventKind::VoiceSound
            | AppEventKind::ActivateChannel
            | AppEventKind::GatewayResumed
            | AppEventKind::GatewayReidentified
            | AppEventKind::GatewayClosed => AppEventMetadata::effect_only(),

            AppEventKind::VoiceServerUpdate => AppEventMetadata::inert(),
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

pub(crate) fn default_avatar_url(user_id: Id<UserMarker>, discriminator: u16) -> String {
    let index = if discriminator == 0 {
        (user_id.get() >> 22) % 6
    } else {
        u64::from(discriminator % 5)
    };

    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

pub(crate) fn avatar_hash_extension(hash: &str) -> &'static str {
    if hash.starts_with("a_") { "gif" } else { "png" }
}

#[cfg(test)]
mod tests {
    use crate::discord::AttachmentInfo;

    use super::*;

    #[test]
    fn attachment_media_classification_controls_inline_preview() {
        let video = attachment_info("clip.mp4", Some("video/mp4"));
        assert!(!video.is_image());
        assert!(video.is_video());
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
        let event = AppEvent::CurrentUserCapabilities { has_nitro: true };

        assert!(!event.mutates_discord_state());
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
