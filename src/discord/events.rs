use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::ApplicationCommandInfo;
use super::commands::{
    DownloadAttachmentSource, ForumPostArchiveState, MessageSearchPage, MessageSearchQuery,
    ReactionEmoji,
};
use super::{
    ActivityInfo, AttachmentInfo, AttachmentUpdate, ChannelInfo, CustomEmojiInfo, EmbedInfo,
    GuildFolder, GuildNotificationSettingsInfo, MemberInfo, MentionInfo, MessageInfo,
    MessageInteractionInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PollInfo,
    PresenceStatus, ReactionUsersInfo, ReadStateInfo, RelationshipInfo, ReplyInfo, RoleInfo,
    UserProfileInfo, VoiceConnectionStatus, VoiceServerInfo, VoiceSoundKind, VoiceStateInfo,
};

#[cfg(test)]
use super::PollAnswerInfo;

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
        user_id: Option<Id<UserMarker>>,
    },
    CurrentUserCapabilities {
        has_nitro: bool,
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
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_id: Id<UserMarker>,
        author: String,
        author_avatar_url: Option<String>,
        author_is_bot: bool,
        author_role_ids: Vec<Id<RoleMarker>>,
        message_kind: MessageKind,
        interaction: Option<MessageInteractionInfo>,
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
    MessageHistoryRefreshed {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryAfterLoaded {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        messages: Vec<MessageInfo>,
        has_more: bool,
    },
    MessageHistoryCatchUpLoaded {
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        messages: Vec<MessageInfo>,
        has_more: bool,
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
    GatewayResumed,
    GatewayReidentified,
    GatewayClosed,
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
    use super::*;

    pub(crate) struct MessageCreateFixture {
        pub(crate) guild_id: Option<Id<GuildMarker>>,
        pub(crate) channel_id: Id<ChannelMarker>,
        pub(crate) message_id: Id<MessageMarker>,
        pub(crate) author_id: Id<UserMarker>,
        pub(crate) author: String,
        pub(crate) author_avatar_url: Option<String>,
        pub(crate) author_is_bot: bool,
        pub(crate) author_role_ids: Vec<Id<RoleMarker>>,
        pub(crate) message_kind: MessageKind,
        pub(crate) interaction: Option<MessageInteractionInfo>,
        pub(crate) reference: Option<MessageReferenceInfo>,
        pub(crate) reply: Option<ReplyInfo>,
        pub(crate) poll: Option<PollInfo>,
        pub(crate) content: Option<String>,
        pub(crate) sticker_names: Vec<String>,
        pub(crate) mentions: Vec<MentionInfo>,
        pub(crate) attachments: Vec<AttachmentInfo>,
        pub(crate) embeds: Vec<EmbedInfo>,
        pub(crate) forwarded_snapshots: Vec<MessageSnapshotInfo>,
    }

    impl Default for MessageCreateFixture {
        fn default() -> Self {
            Self {
                guild_id: None,
                channel_id: Id::new(2),
                message_id: Id::new(1),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_is_bot: false,
                author_role_ids: Vec::new(),
                message_kind: MessageKind::regular(),
                interaction: None,
                reference: None,
                reply: None,
                poll: None,
                content: Some("hello".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            }
        }
    }

    pub(crate) fn guild_message_create_fixture() -> MessageCreateFixture {
        MessageCreateFixture {
            guild_id: Some(Id::new(1)),
            ..MessageCreateFixture::default()
        }
    }

    pub(crate) fn message_create_event(event: MessageCreateFixture) -> AppEvent {
        AppEvent::MessageCreate {
            guild_id: event.guild_id,
            channel_id: event.channel_id,
            message_id: event.message_id,
            author_id: event.author_id,
            author: event.author,
            author_avatar_url: event.author_avatar_url,
            author_is_bot: event.author_is_bot,
            author_role_ids: event.author_role_ids,
            message_kind: event.message_kind,
            interaction: event.interaction,
            reference: event.reference,
            reply: event.reply,
            poll: event.poll,
            content: event.content,
            sticker_names: event.sticker_names,
            mentions: event.mentions,
            attachments: event.attachments,
            embeds: event.embeds,
            forwarded_snapshots: event.forwarded_snapshots,
        }
    }
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
                | AppEvent::ApplicationCommandsLoaded { .. }
                | AppEvent::AttachmentDownloadCompleted { .. }
                | AppEvent::UpdateAvailable { .. }
                | AppEvent::ReactionUsersLoaded { .. }
                | AppEvent::AttachmentPreviewLoaded { .. }
                | AppEvent::AttachmentPreviewLoadFailed { .. }
                | AppEvent::ThreadPreviewLoadFailed { .. }
                | AppEvent::ForumPostsLoadFailed { .. }
                | AppEvent::MessageSearchLoadFailed { .. }
                | AppEvent::MessageHistoryLoadFailed { .. }
                | AppEvent::PinnedMessagesLoadFailed { .. }
                | AppEvent::UserProfileLoadFailed { .. }
                | AppEvent::VoiceServerUpdate { .. }
                | AppEvent::VoiceConnectionStatusChanged { .. }
                | AppEvent::VoiceSound { .. }
                | AppEvent::ActivateChannel { .. }
                | AppEvent::GatewayResumed
                | AppEvent::GatewayReidentified
                | AppEvent::GatewayClosed
        )
    }

    pub fn needs_effect_delivery(&self) -> bool {
        match self {
            AppEvent::ChannelUpsert(channel) => channel_upsert_needs_effect_delivery(channel),
            AppEvent::MessageCreate { .. }
            | AppEvent::MessageHistoryLoaded { .. }
            | AppEvent::MessageHistoryRefreshed { .. }
            | AppEvent::MessageHistoryAfterLoaded { .. }
            | AppEvent::MessageHistoryCatchUpLoaded { .. }
            | AppEvent::MessageHistoryAroundLoaded { .. }
            | AppEvent::MessageHistoryLoadFailed { .. }
            | AppEvent::ThreadPreviewLoaded { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostsLoaded { .. }
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::MessageSearchLoaded { .. }
            | AppEvent::MessageSearchLoadFailed { .. }
            | AppEvent::PinnedMessagesLoaded { .. }
            | AppEvent::PinnedMessagesLoadFailed { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::CurrentUserCapabilities { .. }
            | AppEvent::ApplicationCommandsLoaded { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::UpdateAvailable { .. }
            | AppEvent::ActivateChannel { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::VoiceConnectionStatusChanged { .. }
            | AppEvent::VoiceSound { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::GatewayResumed
            | AppEvent::GatewayReidentified
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
