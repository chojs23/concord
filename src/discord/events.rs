use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::commands::{
    AttachmentDownloadId, DownloadAttachmentSource, MediaPlaybackRequestId,
    MessageHistoryAfterMode, MessageSearchPage, MessageSearchQuery, ReactionEmoji,
    StreamCaptureTargetsRequestId,
};
use super::{
    ActivityInfo, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, EmbedInfo,
    FriendStatus, GuildBoostTier, GuildNotificationSettingsInfo, GuildOnboardingInfo,
    GuildVerificationLevel, MemberInfo, MentionInfo, MessageComponentInfo, MessageInfo, PollInfo,
    PremiumTier, PresenceStatus, ReactionUserInfo, ReadStateInfo, RelationshipInfo,
    RelationshipUpdateInfo, RoleInfo, SnapshotAreas, StickerInfo, StreamCaptureTarget,
    StreamCreateInfo, StreamDeleteInfo, StreamServerInfo, StreamUpdateInfo, ThreadGatewayInfo,
    ThreadListSyncInfo, ThreadMemberInfo, ThreadMemberListUpdateInfo, ThreadMembersUpdateInfo,
    UserProfileInfo, UserSettingsInfo, VoiceConnectionStatus, VoiceScope, VoiceServerInfo,
    VoiceSoundKind, VoiceStateInfo, is_thread_kind,
};
use super::{ApplicationCommandChoiceInfo, ApplicationCommandInfo};
use super::{ArchivedThreadsPage, ForumPostDataInfo};

#[cfg(test)]
use super::PollAnswerInfo;

#[derive(Clone, Debug, PartialEq)]
pub struct GatewayDispatchInfo {
    pub event_type: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelUnreadInfo {
    pub channel_id: Id<ChannelMarker>,
    pub last_message_id: Option<Option<Id<MessageMarker>>>,
    pub last_pin_timestamp: Option<Option<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUpdateEventFields {
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
    pub stickers: Option<Vec<StickerInfo>>,
    pub mentions: Option<Vec<MentionInfo>>,
    pub mention_everyone: Option<bool>,
    pub mention_roles: Option<Vec<Id<RoleMarker>>>,
    pub flags: Option<u64>,
    pub pinned: Option<bool>,
    pub attachments: AttachmentUpdate,
    pub embeds: Option<Vec<EmbedInfo>>,
    pub components: Option<Vec<MessageComponentInfo>>,
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
            stickers: None,
            mentions: None,
            mention_everyone: None,
            mention_roles: None,
            flags: None,
            pinned: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            components: None,
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
pub enum GuildMemberListItem {
    Member {
        member: MemberInfo,
        presence: Option<PresenceEventFields>,
    },
    Group {
        id: String,
        count: u64,
    },
    Unknown {
        raw: Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildMemberListOperation {
    Sync {
        range: (u32, u32),
        items: Vec<GuildMemberListItem>,
    },
    Insert {
        index: u32,
        item: GuildMemberListItem,
    },
    Update {
        index: u32,
        item: GuildMemberListItem,
    },
    Delete {
        index: u32,
    },
    Invalidate {
        range: (u32, u32),
    },
    /// An operation Concord does not understand cannot be treated as a no-op.
    /// Keeping the raw value lets state invalidate the list conservatively and
    /// preserves enough data to add support once Discord introduces it.
    Unknown {
        name: Option<String>,
        raw: Value,
    },
}

impl GuildMemberListOperation {
    pub fn items(&self) -> &[GuildMemberListItem] {
        match self {
            Self::Sync { items, .. } => items,
            Self::Insert { item, .. } | Self::Update { item, .. } => std::slice::from_ref(item),
            Self::Delete { .. } | Self::Invalidate { .. } | Self::Unknown { .. } => &[],
        }
    }
}

impl GuildMemberListItem {
    pub fn member(&self) -> Option<(&MemberInfo, Option<&PresenceEventFields>)> {
        match self {
            Self::Member { member, presence } => Some((member, presence.as_ref())),
            Self::Group { .. } | Self::Unknown { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GuildMemberListUpdateInfo {
    pub guild_id: Id<GuildMarker>,
    pub list_id: Option<String>,
    pub member_count: Option<u64>,
    pub online_count: Option<u32>,
    pub groups: Vec<Value>,
    pub ops: Vec<GuildMemberListOperation>,
    pub extra_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadySnapshotInfo {
    /// `None` means the source payload omitted the field, so existing state
    /// must not be reconciled from an incomplete test or future payload.
    pub guild_ids: Option<Vec<Id<GuildMarker>>>,
    /// Guild channel collections are authoritative when present in READY.
    pub guild_channel_ids: BTreeMap<Id<GuildMarker>, Vec<Id<ChannelMarker>>>,
    /// READY and READY_SUPPLEMENTAL together form the private-channel snapshot.
    pub private_channel_ids: Option<Vec<Id<ChannelMarker>>>,
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
    ReadyUserDirectory {
        users: Vec<ChannelRecipientInfo>,
    },
    /// Marks the end of READY parsing. State uses the complete ID sets to
    /// remove guilds and guild channels that belonged only to an older
    /// Gateway session. Private channels wait for READY_SUPPLEMENTAL.
    ReadySnapshotComplete {
        snapshot: ReadySnapshotInfo,
    },
    /// Marks the end of READY_SUPPLEMENTAL private-channel parsing so the
    /// READY and supplemental ID sets can be reconciled as one snapshot.
    ReadySupplementalComplete {
        private_channel_ids: Vec<Id<ChannelMarker>>,
    },
    SignedOut,
    CurrentUserCapabilities {
        premium_tier: PremiumTier,
    },
    CurrentUserVerification {
        email_verified: Option<bool>,
        phone_verified: Option<bool>,
        mfa_enabled: Option<bool>,
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
    ApplicationCommandIndexUpdated {
        guild_id: Id<GuildMarker>,
    },
    InteractionSucceeded {
        interaction_id: u64,
        nonce: Option<String>,
        correlated: bool,
    },
    InteractionFailed {
        interaction_id: u64,
        nonce: Option<String>,
        reason_code: u64,
        correlated: bool,
    },
    ApplicationCommandAutocompleteResponse {
        nonce: Option<String>,
        choices: Vec<ApplicationCommandChoiceInfo>,
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
        verification_level: Option<GuildVerificationLevel>,
        mfa_level: Option<u64>,
        features: Option<Vec<String>>,
        onboarding: Option<GuildOnboardingInfo>,
        channels: Vec<ChannelInfo>,
        /// Whether the Gateway payload contained the guild's `threads` array.
        /// An empty array clears the snapshot, while an omitted field in
        /// `CLIENT_STATE_V2` partial mode must preserve cached thread state.
        thread_snapshot_complete: bool,
        current_user_thread_members: Vec<ThreadMemberInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<PresenceEventFields>,
        roles: Option<Vec<RoleInfo>>,
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
        verification_level: Option<GuildVerificationLevel>,
        mfa_level: Option<u64>,
        features: Option<Vec<String>>,
        onboarding: Option<GuildOnboardingInfo>,
        roles: Option<Vec<RoleInfo>>,
        emojis: Option<Vec<CustomEmojiInfo>>,
    },
    GuildOnboardingUpdate {
        guild_id: Id<GuildMarker>,
        onboarding: GuildOnboardingInfo,
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
    GuildUnavailable {
        guild_id: Id<GuildMarker>,
    },
    SelectedGuildChanged {
        guild_id: Option<Id<GuildMarker>>,
    },
    SelectedMessageChannelChanged {
        channel_id: Option<Id<ChannelMarker>>,
    },
    ChannelUpsert(ChannelInfo),
    LazyPrivateChannelUpsert {
        channel: ChannelInfo,
        recipient_ids: Vec<Id<UserMarker>>,
    },
    ChannelRecipientAdd {
        channel_id: Id<ChannelMarker>,
        recipient: ChannelRecipientInfo,
    },
    ChannelRecipientRemove {
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    },
    ChannelDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
    },
    ThreadUpsert {
        thread: ThreadGatewayInfo,
        created: bool,
    },
    ThreadListSync {
        sync: ThreadListSyncInfo,
    },
    ThreadMembersUpdateDispatch {
        update: ThreadMembersUpdateInfo,
    },
    ThreadMemberListUpdate {
        update: ThreadMemberListUpdateInfo,
    },
    ThreadMemberUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        member: ThreadMemberInfo,
    },
    MessageCreate {
        message: MessageInfo,
    },
    MessageSendFailed {
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
    },
    MessageSendRateLimited {
        channel_id: Id<ChannelMarker>,
        retry_after_millis: u64,
    },
    MessageSendCooldownStarted {
        channel_id: Id<ChannelMarker>,
        duration_millis: u64,
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
    ForumPostDataLoaded {
        channel_id: Id<ChannelMarker>,
        requested_thread_ids: Vec<Id<ChannelMarker>>,
        posts: Vec<ForumPostDataInfo>,
    },
    ForumPostDataLoadFailed {
        channel_id: Id<ChannelMarker>,
        thread_ids: Vec<Id<ChannelMarker>>,
        message: String,
    },
    ArchivedThreadsLoaded {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        before: Option<String>,
        page: ArchivedThreadsPage,
    },
    ArchivedThreadsLoadFailed {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        before: Option<String>,
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
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
        has_more: bool,
    },
    InboxMentionsLoadFailed {
        request_id: u64,
        before: Option<Id<MessageMarker>>,
    },
    InboxRecentMentionDeleted {
        message_id: Id<MessageMarker>,
    },
    InboxRecentMentionDeleteFailed {
        message_id: Id<MessageMarker>,
        message: String,
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
    StreamCreate {
        stream: StreamCreateInfo,
    },
    StreamUpdate {
        stream: StreamUpdateInfo,
    },
    StreamServerUpdate {
        server: StreamServerInfo,
    },
    StreamDelete {
        stream: StreamDeleteInfo,
    },
    VoiceConnectionStatusChanged {
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        status: VoiceConnectionStatus,
        message: Option<String>,
    },
    VoiceAudioSourcesLoaded {
        request_id: u64,
        inputs: Vec<(String, String)>,
        outputs: Vec<(String, String)>,
        error: Option<String>,
    },
    VoiceAudioSourcesApplyFailed {
        requested_input_source: Option<String>,
        requested_output_source: Option<String>,
        active_input_source: Option<String>,
        active_output_source: Option<String>,
        message: String,
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
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        member: Option<MemberInfo>,
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
    UserNotificationSettingsUpdate {
        flags: u64,
    },
    UserGuildSettingsInit {
        settings: Vec<UserGuildSettingsInfo>,
    },
    UserGuildSettingsSync {
        settings: Vec<UserGuildSettingsInfo>,
        partial: bool,
        version: Option<i64>,
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
    StreamPlaybackWindowReady {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    },
    StreamPlaybackEnded {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        reconnecting: bool,
    },
    StreamCaptureTargetsLoaded {
        request_id: StreamCaptureTargetsRequestId,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        targets: Vec<StreamCaptureTarget>,
        error: Option<String>,
    },
    StreamBroadcastStarted {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    StreamBroadcastAudioUnavailable {
        message: String,
    },
    StreamBroadcastStartFailed {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    },
    StreamBroadcastEnded {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
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
    RelationshipUpdate {
        update: RelationshipUpdateInfo,
    },
    RelationshipRemove {
        user_id: Id<UserMarker>,
        status: Option<FriendStatus>,
    },
    /// Full read-state replacement used by internal and test data sources.
    ReadStateInit {
        entries: Vec<ReadStateInfo>,
    },
    /// READY read states with their versioned-array replacement semantics.
    ReadStateSync {
        entries: Vec<ReadStateInfo>,
        partial: bool,
        version: Option<i64>,
    },
    /// Gateway `MESSAGE_ACK` or a locally synthesized ack on activation.
    MessageAck {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        mention_count: Option<u32>,
        flags: Option<u64>,
        last_viewed: Option<u64>,
        /// Gateway acknowledgements carry the aggregate read-state version.
        /// Locally synthesized optimistic acknowledgements leave it unknown.
        version: Option<i64>,
    },
    FeatureReadStateAck {
        read_state_type: u8,
        resource_id: u64,
        entity_id: u64,
        version: i64,
    },
    ChannelPinsAck {
        channel_id: Id<ChannelMarker>,
        timestamp: String,
        version: i64,
    },
    ChannelUnreadUpdate {
        guild_id: Id<GuildMarker>,
        channels: Vec<ChannelUnreadInfo>,
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
    /// Optimistic update for the current user's thread member mute settings.
    ThreadMuteUpdate {
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<String>,
        selected_time_window: Option<i64>,
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
    ReadyUserDirectory: AppEvent::ReadyUserDirectory { .. },
    ReadySnapshotComplete: AppEvent::ReadySnapshotComplete { .. },
    ReadySupplementalComplete: AppEvent::ReadySupplementalComplete { .. },
    SignedOut: AppEvent::SignedOut,
    CurrentUserCapabilities: AppEvent::CurrentUserCapabilities { .. },
    CurrentUserVerification: AppEvent::CurrentUserVerification { .. },
    UserIdentityUpdate: AppEvent::UserIdentityUpdate { .. },
    ApplicationCommandsLoaded: AppEvent::ApplicationCommandsLoaded { .. },
    ApplicationCommandIndexUpdated: AppEvent::ApplicationCommandIndexUpdated { .. },
    InteractionSucceeded: AppEvent::InteractionSucceeded { .. },
    InteractionFailed: AppEvent::InteractionFailed { .. },
    ApplicationCommandAutocompleteResponse: AppEvent::ApplicationCommandAutocompleteResponse { .. },
    GuildCreate: AppEvent::GuildCreate { .. },
    GuildUpdate: AppEvent::GuildUpdate { .. },
    GuildOnboardingUpdate: AppEvent::GuildOnboardingUpdate { .. },
    GuildRolesUpdate: AppEvent::GuildRolesUpdate { .. },
    GuildRoleUpsert: AppEvent::GuildRoleUpsert { .. },
    GuildRoleDelete: AppEvent::GuildRoleDelete { .. },
    GuildEmojisUpdate: AppEvent::GuildEmojisUpdate { .. },
    GuildDelete: AppEvent::GuildDelete { .. },
    GuildUnavailable: AppEvent::GuildUnavailable { .. },
    SelectedGuildChanged: AppEvent::SelectedGuildChanged { .. },
    SelectedMessageChannelChanged: AppEvent::SelectedMessageChannelChanged { .. },
    ChannelUpsert: AppEvent::ChannelUpsert(_),
    LazyPrivateChannelUpsert: AppEvent::LazyPrivateChannelUpsert { .. },
    ChannelRecipientAdd: AppEvent::ChannelRecipientAdd { .. },
    ChannelRecipientRemove: AppEvent::ChannelRecipientRemove { .. },
    ChannelDelete: AppEvent::ChannelDelete { .. },
    ThreadUpsert: AppEvent::ThreadUpsert { .. },
    ThreadListSync: AppEvent::ThreadListSync { .. },
    ThreadMembersUpdateDispatch: AppEvent::ThreadMembersUpdateDispatch { .. },
    ThreadMemberListUpdate: AppEvent::ThreadMemberListUpdate { .. },
    ThreadMemberUpdate: AppEvent::ThreadMemberUpdate { .. },
    MessageCreate: AppEvent::MessageCreate { .. },
    MessageSendFailed: AppEvent::MessageSendFailed { .. },
    MessageSendRateLimited: AppEvent::MessageSendRateLimited { .. },
    MessageSendCooldownStarted: AppEvent::MessageSendCooldownStarted { .. },
    MessageHistoryLoaded: AppEvent::MessageHistoryLoaded { .. },
    MessageHistoryRefreshed: AppEvent::MessageHistoryRefreshed { .. },
    MessageHistoryAfterLoaded: AppEvent::MessageHistoryAfterLoaded { .. },
    MessageHistoryAroundLoaded: AppEvent::MessageHistoryAroundLoaded { .. },
    ThreadPreviewLoaded: AppEvent::ThreadPreviewLoaded { .. },
    ThreadPreviewLoadFailed: AppEvent::ThreadPreviewLoadFailed { .. },
    ForumPostDataLoaded: AppEvent::ForumPostDataLoaded { .. },
    ForumPostDataLoadFailed: AppEvent::ForumPostDataLoadFailed { .. },
    ArchivedThreadsLoaded: AppEvent::ArchivedThreadsLoaded { .. },
    ArchivedThreadsLoadFailed: AppEvent::ArchivedThreadsLoadFailed { .. },
    MessageSearchLoaded: AppEvent::MessageSearchLoaded { .. },
    MessageSearchLoadFailed: AppEvent::MessageSearchLoadFailed { .. },
    InboxMentionsLoaded: AppEvent::InboxMentionsLoaded { .. },
    InboxMentionsLoadFailed: AppEvent::InboxMentionsLoadFailed { .. },
    InboxRecentMentionDeleted: AppEvent::InboxRecentMentionDeleted { .. },
    InboxRecentMentionDeleteFailed: AppEvent::InboxRecentMentionDeleteFailed { .. },
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
    StreamCreate: AppEvent::StreamCreate { .. },
    StreamUpdate: AppEvent::StreamUpdate { .. },
    StreamServerUpdate: AppEvent::StreamServerUpdate { .. },
    StreamDelete: AppEvent::StreamDelete { .. },
    VoiceConnectionStatusChanged: AppEvent::VoiceConnectionStatusChanged { .. },
    VoiceAudioSourcesLoaded: AppEvent::VoiceAudioSourcesLoaded { .. },
    VoiceAudioSourcesApplyFailed: AppEvent::VoiceAudioSourcesApplyFailed { .. },
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
    UserNotificationSettingsUpdate: AppEvent::UserNotificationSettingsUpdate { .. },
    UserGuildSettingsInit: AppEvent::UserGuildSettingsInit { .. },
    UserGuildSettingsSync: AppEvent::UserGuildSettingsSync { .. },
    UserGuildSettingsUpdate: AppEvent::UserGuildSettingsUpdate { .. },
    GatewayError: AppEvent::GatewayError { .. },
    CaptchaRequired: AppEvent::CaptchaRequired { .. },
    ThreadNotificationLevelUpdate: AppEvent::ThreadNotificationLevelUpdate { .. },
    ThreadMuteUpdate: AppEvent::ThreadMuteUpdate { .. },
    MediaPlaybackWindowReady: AppEvent::MediaPlaybackWindowReady { .. },
    StreamPlaybackWindowReady: AppEvent::StreamPlaybackWindowReady { .. },
    StreamPlaybackEnded: AppEvent::StreamPlaybackEnded { .. },
    StreamCaptureTargetsLoaded: AppEvent::StreamCaptureTargetsLoaded { .. },
    StreamBroadcastStarted: AppEvent::StreamBroadcastStarted { .. },
    StreamBroadcastAudioUnavailable: AppEvent::StreamBroadcastAudioUnavailable { .. },
    StreamBroadcastStartFailed: AppEvent::StreamBroadcastStartFailed { .. },
    StreamBroadcastEnded: AppEvent::StreamBroadcastEnded { .. },
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
    RelationshipUpdate: AppEvent::RelationshipUpdate { .. },
    RelationshipRemove: AppEvent::RelationshipRemove { .. },
    ReadStateInit: AppEvent::ReadStateInit { .. },
    ReadStateSync: AppEvent::ReadStateSync { .. },
    MessageAck: AppEvent::MessageAck { .. },
    FeatureReadStateAck: AppEvent::FeatureReadStateAck { .. },
    ChannelPinsAck: AppEvent::ChannelPinsAck { .. },
    ChannelUnreadUpdate: AppEvent::ChannelUnreadUpdate { .. },
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
    }

    pub(crate) fn guild_message_create_fixture() -> MessageCreateFixture {
        MessageCreateFixture::guild_message(Id::new(1), Id::new(2), Id::new(1))
    }

    pub(crate) fn message_create_event(event: MessageCreateFixture) -> AppEvent {
        AppEvent::MessageCreate { message: event }
    }

    use crate::discord::{
        ChannelInfo, CustomEmojiInfo, GuildBoostTier, GuildOnboardingInfo, MemberInfo, RoleInfo,
        ThreadMemberInfo,
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
        pub(crate) verification_level: GuildVerificationLevel,
        pub(crate) mfa_level: u64,
        pub(crate) features: Vec<String>,
        pub(crate) onboarding: Option<GuildOnboardingInfo>,
        pub(crate) channels: Vec<ChannelInfo>,
        pub(crate) thread_snapshot_complete: bool,
        pub(crate) current_user_thread_members: Vec<ThreadMemberInfo>,
        pub(crate) members: Vec<MemberInfo>,
        pub(crate) presences: Vec<PresenceEventFields>,
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
                verification_level: GuildVerificationLevel::None,
                mfa_level: 0,
                features: Vec::new(),
                onboarding: None,
                channels: Vec::new(),
                thread_snapshot_complete: true,
                current_user_thread_members: Vec::new(),
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
            verification_level: Some(event.verification_level),
            mfa_level: Some(event.mfa_level),
            features: Some(event.features),
            onboarding: event.onboarding,
            channels: event.channels,
            thread_snapshot_complete: event.thread_snapshot_complete,
            current_user_thread_members: event.current_user_thread_members,
            members: event.members,
            presences: event.presences,
            roles: Some(event.roles),
            emojis: event.emojis,
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

    pub(crate) fn empty_latest_message_history_loaded_event(
        channel_id: Id<ChannelMarker>,
    ) -> AppEvent {
        message_history_loaded_event(MessageHistoryLoadedFixture {
            channel_id,
            ..MessageHistoryLoadedFixture::new()
        })
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
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) user_id: Id<UserMarker>,
        pub(crate) member: Option<MemberInfo>,
    }

    impl TypingStartFixture {
        pub(crate) fn new() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(1),
                user_id: Id::new(1),
                member: None,
            }
        }
    }

    pub(crate) fn typing_start_event(f: TypingStartFixture) -> AppEvent {
        AppEvent::TypingStart {
            guild_id: f.guild_id,
            channel_id: f.channel_id,
            user_id: f.user_id,
            member: f.member,
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
            mention_count: Some(f.mention_count),
            flags: None,
            last_viewed: None,
            version: None,
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
        pub(crate) verification_level: Option<GuildVerificationLevel>,
        pub(crate) mfa_level: Option<u64>,
        pub(crate) features: Option<Vec<String>>,
        pub(crate) onboarding: Option<GuildOnboardingInfo>,
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
                verification_level: None,
                mfa_level: None,
                features: None,
                onboarding: None,
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
            verification_level: f.verification_level,
            mfa_level: f.mfa_level,
            features: f.features,
            onboarding: f.onboarding,
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
    /// `Some` means the event mutates `DiscordState` and names the areas whose
    /// revision must advance. Applying without advancing a revision would leave
    /// the TUI permanently unaware of the write, so the two facts are one field
    /// rather than a bool that can drift out of step with the areas.
    pub(crate) snapshot_areas: Option<SnapshotAreas>,
    pub(crate) needs_effect_delivery: bool,
}

impl AppEventMetadata {
    const fn mutating(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            snapshot_areas: Some(snapshot_areas),
            needs_effect_delivery: false,
        }
    }

    const fn mutating_effect(snapshot_areas: SnapshotAreas) -> Self {
        Self {
            snapshot_areas: Some(snapshot_areas),
            needs_effect_delivery: true,
        }
    }

    const fn effect_only() -> Self {
        Self {
            snapshot_areas: None,
            needs_effect_delivery: true,
        }
    }

    const fn inert() -> Self {
        Self {
            snapshot_areas: None,
            needs_effect_delivery: false,
        }
    }
}

impl AppEventKind {
    const fn metadata(self) -> AppEventMetadata {
        match self {
            AppEventKind::GuildCreate
            | AppEventKind::GuildUpdate
            | AppEventKind::GuildOnboardingUpdate
            | AppEventKind::ChannelUpsert
            | AppEventKind::LazyPrivateChannelUpsert
            | AppEventKind::ChannelRecipientAdd
            | AppEventKind::ChannelRecipientRemove
            | AppEventKind::ArchivedThreadsLoaded
            | AppEventKind::Ready => AppEventMetadata::mutating(SnapshotAreas::navigation()),

            AppEventKind::ForumPostDataLoaded => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::MessageCreate => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::ThreadUpsert => {
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
            | AppEventKind::CurrentUserPollVoteUpdate
            | AppEventKind::MessageDelete
            | AppEventKind::MessageDeleteBulk => {
                AppEventMetadata::mutating(SnapshotAreas::message())
            }

            AppEventKind::ChannelPinsUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::message_and_detail())
            }

            AppEventKind::SelectedMessageChannelChanged => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::UserProfileLoaded => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::GuildDelete
            | AppEventKind::ChannelDelete
            | AppEventKind::ReadySnapshotComplete
            | AppEventKind::ReadySupplementalComplete
            | AppEventKind::GuildMemberListUpdate
            | AppEventKind::GuildMembersChunk
            | AppEventKind::GuildMemberAdd
            | AppEventKind::GuildMemberUpsert
            | AppEventKind::ThreadListSync
            | AppEventKind::ThreadMembersUpdateDispatch
            | AppEventKind::ThreadMemberListUpdate
            | AppEventKind::ThreadMemberUpdate
            | AppEventKind::RelationshipsLoaded
            | AppEventKind::RelationshipUpdate
            | AppEventKind::UserIdentityUpdate
            | AppEventKind::VoiceStateUpdate
            | AppEventKind::TypingStart
            | AppEventKind::ReadyUserDirectory => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_message())
            }

            AppEventKind::RelationshipUpsert | AppEventKind::RelationshipRemove => {
                AppEventMetadata::mutating(SnapshotAreas::all())
            }

            AppEventKind::GuildUnavailable => AppEventMetadata::inert(),

            AppEventKind::GatewayReidentified => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }

            AppEventKind::ArchivedThreadsLoadFailed => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }

            AppEventKind::SelectedGuildChanged
            | AppEventKind::GuildRolesUpdate
            | AppEventKind::GuildRoleUpsert
            | AppEventKind::GuildRoleDelete
            | AppEventKind::GuildEmojisUpdate
            | AppEventKind::GuildMemberRemove
            | AppEventKind::PresenceUpdate
            | AppEventKind::VoiceSpeakingUpdate
            | AppEventKind::CallDelete
            | AppEventKind::UserSettingsUpdate
            | AppEventKind::UserNotificationSettingsUpdate
            | AppEventKind::UserNoteLoaded
            | AppEventKind::CurrentUserVerification
            | AppEventKind::UserGuildSettingsInit
            | AppEventKind::UserGuildSettingsSync
            | AppEventKind::UserGuildSettingsUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation())
            }

            AppEventKind::ReadStateInit
            | AppEventKind::ReadStateSync
            | AppEventKind::MessageAck
            | AppEventKind::FeatureReadStateAck
            | AppEventKind::ChannelPinsAck
            | AppEventKind::ChannelUnreadUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation_and_detail())
            }

            AppEventKind::GatewayError
            | AppEventKind::CaptchaRequired
            | AppEventKind::MessageSendFailed
            | AppEventKind::MessageSendRateLimited
            | AppEventKind::MessageSendCooldownStarted
            | AppEventKind::GatewayDispatchReceived
            | AppEventKind::SignedOut
            | AppEventKind::MediaPlaybackWindowReady
            | AppEventKind::StreamPlaybackWindowReady
            | AppEventKind::StreamPlaybackEnded
            | AppEventKind::StreamCaptureTargetsLoaded
            | AppEventKind::VoiceAudioSourcesLoaded
            | AppEventKind::VoiceAudioSourcesApplyFailed
            | AppEventKind::StreamBroadcastStarted
            | AppEventKind::StreamBroadcastAudioUnavailable
            | AppEventKind::StreamBroadcastStartFailed
            | AppEventKind::StreamBroadcastEnded
            | AppEventKind::ApplicationCommandsLoaded
            | AppEventKind::ApplicationCommandIndexUpdated
            | AppEventKind::InteractionSucceeded
            | AppEventKind::InteractionFailed
            | AppEventKind::ApplicationCommandAutocompleteResponse
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
            | AppEventKind::ForumPostDataLoadFailed
            | AppEventKind::MessageSearchLoadFailed
            | AppEventKind::MessageHistoryLoadFailed
            | AppEventKind::InboxMentionsLoaded
            | AppEventKind::InboxMentionsLoadFailed
            | AppEventKind::InboxRecentMentionDeleted
            | AppEventKind::InboxRecentMentionDeleteFailed
            | AppEventKind::InboxChannelMessagesLoaded
            | AppEventKind::InboxChannelMessagesLoadFailed
            | AppEventKind::PinnedMessagesLoadFailed
            | AppEventKind::UserProfileLoadFailed
            | AppEventKind::UserProfileUpdateFailed
            | AppEventKind::VoiceConnectionStatusChanged
            | AppEventKind::VoiceSound
            | AppEventKind::RichPresenceDetected
            | AppEventKind::GatewayResumed
            | AppEventKind::GatewayClosed => AppEventMetadata::effect_only(),

            AppEventKind::StreamCreate
            | AppEventKind::StreamUpdate
            | AppEventKind::StreamDelete => AppEventMetadata::mutating(SnapshotAreas::navigation()),

            AppEventKind::VoiceServerUpdate | AppEventKind::StreamServerUpdate => {
                AppEventMetadata::inert()
            }

            // The current user's Nitro tier is stored in the session (part of
            // the navigation snapshot area) so the upload-limit check can read
            // it, and it still needs effect delivery so the TUI can update
            // Nitro-gated UI such as the emoji picker.
            AppEventKind::CurrentUserCapabilities => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }

            AppEventKind::ThreadNotificationLevelUpdate | AppEventKind::ThreadMuteUpdate => {
                AppEventMetadata::mutating(SnapshotAreas::navigation())
            }
        }
    }
}

impl AppEvent {
    pub(crate) fn metadata(&self) -> AppEventMetadata {
        match self {
            AppEvent::ChannelUpsert(channel) if channel_upsert_needs_effect_delivery(channel) => {
                AppEventMetadata::mutating_effect(SnapshotAreas::navigation())
            }
            AppEvent::ThreadUpsert { created: true, .. } => {
                AppEventMetadata::mutating_effect(SnapshotAreas::all())
            }
            _ => self.kind().metadata(),
        }
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
    fn event_metadata_routes_each_delivery_category() {
        let cases = [
            (
                "mutating, snapshot only",
                AppEvent::MessageDeleteBulk {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    message_ids: vec![Id::new(20), Id::new(30)],
                },
                Some(SnapshotAreas::message()),
                false,
            ),
            (
                "mutating, also delivered as an effect",
                AppEvent::CurrentUserCapabilities {
                    premium_tier: PremiumTier::Nitro,
                },
                Some(SnapshotAreas::navigation()),
                true,
            ),
            ("effect only", AppEvent::GatewayClosed, None, true),
            (
                "inert",
                AppEvent::GuildUnavailable {
                    guild_id: Id::new(1),
                },
                None,
                false,
            ),
            (
                "typing updates shared member and message identity",
                AppEvent::TypingStart {
                    guild_id: Some(Id::new(1)),
                    channel_id: Id::new(10),
                    user_id: Id::new(20),
                    member: None,
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
            (
                "ready user directory joins guild and message identity",
                AppEvent::ReadyUserDirectory {
                    users: vec![ChannelRecipientInfo::test(Id::new(20), "Ready User")],
                },
                Some(SnapshotAreas::navigation_and_message()),
                false,
            ),
            (
                "archived pages update navigation state",
                AppEvent::ArchivedThreadsLoaded {
                    guild_id: Id::new(1),
                    channel_id: Id::new(10),
                    before: None,
                    page: ArchivedThreadsPage {
                        threads: Vec::new(),
                        members: Vec::new(),
                        has_more: false,
                        next_before: None,
                        extra_fields: BTreeMap::new(),
                    },
                },
                Some(SnapshotAreas::navigation()),
                false,
            ),
            (
                "archived page failures update state and notify the UI",
                AppEvent::ArchivedThreadsLoadFailed {
                    guild_id: Id::new(1),
                    channel_id: Id::new(10),
                    before: None,
                    message: "temporary failure".to_owned(),
                },
                Some(SnapshotAreas::navigation()),
                true,
            ),
            (
                "current-user thread creation can acknowledge its parent",
                AppEvent::ThreadUpsert {
                    thread: ThreadGatewayInfo {
                        channel: ChannelInfo::test(Id::new(10), "GuildPublicThread"),
                        current_user_member: None,
                    },
                    created: true,
                },
                Some(SnapshotAreas::all()),
                true,
            ),
        ];

        for (label, event, expected_areas, expected_effect) in cases {
            assert_eq!(event.snapshot_areas(), expected_areas, "{label}");
            assert_eq!(event.needs_effect_delivery(), expected_effect, "{label}");
        }
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
