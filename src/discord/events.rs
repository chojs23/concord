use twilight_gateway::Event;
use twilight_model::{
    channel::{
        Attachment, Channel, Message, message::Embed, message::Mention, message::MessageSnapshot,
    },
    gateway::{
        payload::incoming::{
            GuildCreate as GuildCreatePayload, GuildEmojisUpdate as GuildEmojisUpdatePayload,
            MemberAdd, MemberChunk as TwilightMemberChunk, MemberUpdate,
            PresenceUpdate as PresenceUpdatePayload,
        },
        presence::{Status as TwilightStatus, UserOrId},
    },
    guild::{Emoji as TwilightEmoji, Member as TwilightMember, Role as TwilightRole},
    id::{
        Id,
        marker::{
            AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker,
            UserMarker,
        },
    },
    poll::Poll,
    user::User as TwilightUser,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::DoNotDisturb => "Do Not Disturb",
            Self::Offline => "Offline",
        }
    }
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleInfo {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentionInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
}

/// One entry from the user's `guild_folders` setting. A folder with `id ==
/// None` and a single member is an ungrouped guild — Discord stores those as
/// "folders" too just for ordering. Real folders carry an integer id, an
/// optional name, and an optional RGB color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildFolder {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<Id<GuildMarker>>,
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
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub source_channel_id: Option<Id<ChannelMarker>>,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyInfo {
    pub author: String,
    pub content: Option<String>,
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
pub struct MessageInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub message_kind: MessageKind,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
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
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        channels: Vec<ChannelInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<(Id<UserMarker>, PresenceStatus)>,
        roles: Vec<RoleInfo>,
        emojis: Vec<CustomEmojiInfo>,
    },
    GuildUpdate {
        guild_id: Id<GuildMarker>,
        name: String,
        roles: Option<Vec<RoleInfo>>,
        emojis: Option<Vec<CustomEmojiInfo>>,
    },
    GuildEmojisUpdate {
        guild_id: Id<GuildMarker>,
        emojis: Vec<CustomEmojiInfo>,
    },
    GuildDelete {
        guild_id: Id<GuildMarker>,
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
        message_kind: MessageKind,
        reference: Option<MessageReferenceInfo>,
        reply: Option<ReplyInfo>,
        poll: Option<PollInfo>,
        content: Option<String>,
        mentions: Vec<MentionInfo>,
        attachments: Vec<AttachmentInfo>,
        forwarded_snapshots: Vec<MessageSnapshotInfo>,
    },
    MessageHistoryLoaded {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        messages: Vec<MessageInfo>,
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
        mentions: Option<Vec<MentionInfo>>,
        attachments: AttachmentUpdate,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    GuildMemberUpsert {
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
    },
    GuildFoldersUpdate {
        folders: Vec<GuildFolder>,
    },
    GatewayError {
        message: String,
    },
    StatusMessage {
        message: String,
    },
    AttachmentPreviewLoaded {
        url: String,
        bytes: Vec<u8>,
    },
    AttachmentPreviewLoadFailed {
        url: String,
        message: String,
    },
    GatewayClosed,
}

impl AppEvent {
    pub fn from_message(message: Message) -> Self {
        let message = MessageInfo::from_message(message);
        Self::MessageCreate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            author_id: message.author_id,
            author: message.author,
            author_avatar_url: message.author_avatar_url,
            message_kind: message.message_kind,
            reference: message.reference,
            reply: message.reply,
            poll: message.poll,
            content: message.content,
            mentions: message.mentions,
            attachments: message.attachments,
            forwarded_snapshots: message.forwarded_snapshots,
        }
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

    pub fn from_attachment(attachment: Attachment) -> Self {
        Self {
            id: attachment.id,
            filename: attachment.filename,
            url: attachment.url,
            proxy_url: attachment.proxy_url,
            content_type: attachment.content_type,
            size: attachment.size,
            width: attachment.width,
            height: attachment.height,
            description: attachment.description,
        }
    }
}

impl MessageSnapshotInfo {
    pub fn from_snapshot(
        snapshot: MessageSnapshot,
        source_channel_id: Option<Id<ChannelMarker>>,
    ) -> Self {
        let message = snapshot.message;
        let mentions = mention_infos(&message.mentions);
        Self {
            content: Some(message.content),
            mentions,
            attachments: message
                .attachments
                .into_iter()
                .map(AttachmentInfo::from_attachment)
                .collect(),
            source_channel_id,
            timestamp: Some(message.timestamp.iso_8601().to_string()),
        }
    }
}

impl ReplyInfo {
    fn from_message(message: &Message) -> Option<Self> {
        let content = if message.content.is_empty() {
            None
        } else {
            Some(message.content.clone())
        };
        Some(Self {
            author: message_display_name(message),
            content,
            mentions: mention_infos(&message.mentions),
        })
    }
}

impl PollInfo {
    fn from_poll(poll: &Poll) -> Self {
        Self {
            question: poll
                .question
                .text
                .clone()
                .unwrap_or_else(|| "<no question text>".to_owned()),
            answers: poll
                .answers
                .iter()
                .map(|answer| {
                    let result = poll.results.as_ref().and_then(|results| {
                        results
                            .answer_counts
                            .iter()
                            .find(|count| count.id == answer.answer_id)
                    });
                    PollAnswerInfo {
                        answer_id: answer.answer_id,
                        text: answer
                            .poll_media
                            .text
                            .clone()
                            .unwrap_or_else(|| "<no answer text>".to_owned()),
                        vote_count: result.map(|count| count.count),
                        me_voted: result.is_some_and(|count| count.me_voted),
                    }
                })
                .collect(),
            allow_multiselect: poll.allow_multiselect,
            results_finalized: poll.results.as_ref().map(|results| results.is_finalized),
            total_votes: poll
                .results
                .as_ref()
                .map(|results| results.answer_counts.iter().map(|count| count.count).sum()),
        }
    }
}

fn filename_has_extension(filename: &str, extensions: &[&str]) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| {
        extensions
            .iter()
            .any(|value| extension.eq_ignore_ascii_case(value))
    })
}

impl MessageInfo {
    pub fn from_message(message: Message) -> Self {
        let reference = message_reference_info(&message.reference);
        let source_channel_id = reference
            .as_ref()
            .and_then(|reference| reference.channel_id);
        let reply = message
            .referenced_message
            .as_deref()
            .and_then(ReplyInfo::from_message);
        let poll = message.poll.as_ref().map(PollInfo::from_poll);
        let mentions = mention_infos(&message.mentions);
        Self {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message_display_name(&message),
            author_avatar_url: Some(user_avatar_url(&message.author)),
            message_kind: MessageKind::new(message.kind.into()),
            reference,
            reply,
            poll: poll.or_else(|| poll_result_info(&message.embeds)),
            content: Some(message.content),
            mentions,
            attachments: message
                .attachments
                .into_iter()
                .map(AttachmentInfo::from_attachment)
                .collect(),
            forwarded_snapshots: message
                .message_snapshots
                .into_iter()
                .map(|snapshot| MessageSnapshotInfo::from_snapshot(snapshot, source_channel_id))
                .collect(),
        }
    }
}

pub fn map_event(event: Event) -> Vec<AppEvent> {
    match event {
        Event::Ready(ready) => vec![AppEvent::Ready {
            user: ready.user.name,
            user_id: Some(ready.user.id),
        }],
        Event::GuildCreate(guild) => map_guild_create(*guild).into_iter().collect(),
        Event::GuildDelete(guild) => vec![AppEvent::GuildDelete { guild_id: guild.id }],
        Event::GuildUpdate(guild) => vec![AppEvent::GuildUpdate {
            guild_id: guild.id,
            name: guild.name.clone(),
            roles: Some(guild.roles.iter().map(role_info).collect()),
            emojis: Some(guild.emojis.iter().map(custom_emoji_info).collect()),
        }],
        Event::GuildEmojisUpdate(update) => vec![guild_emojis_update(&update)],
        Event::ChannelCreate(channel) => vec![AppEvent::ChannelUpsert(channel_info(&channel.0))],
        Event::ChannelUpdate(channel) => vec![AppEvent::ChannelUpsert(channel_info(&channel.0))],
        Event::ChannelDelete(channel) => vec![AppEvent::ChannelDelete {
            guild_id: channel.guild_id,
            channel_id: channel.id,
        }],
        Event::ThreadCreate(thread) => vec![AppEvent::ChannelUpsert(channel_info(&thread.0))],
        Event::ThreadUpdate(thread) => vec![AppEvent::ChannelUpsert(channel_info(&thread.0))],
        Event::ThreadDelete(thread) => vec![AppEvent::ChannelDelete {
            guild_id: Some(thread.guild_id),
            channel_id: thread.id,
        }],
        Event::ThreadListSync(sync) => sync
            .threads
            .iter()
            .map(|thread| AppEvent::ChannelUpsert(channel_info(thread)))
            .collect(),
        Event::MessageCreate(message) => {
            let reference = message_reference_info(&message.reference);
            let source_channel_id = reference
                .as_ref()
                .and_then(|reference| reference.channel_id);
            let reply = message
                .referenced_message
                .as_deref()
                .and_then(ReplyInfo::from_message);
            let poll = message
                .poll
                .as_ref()
                .map(PollInfo::from_poll)
                .or_else(|| poll_result_info(&message.embeds));

            vec![AppEvent::MessageCreate {
                guild_id: message.guild_id,
                channel_id: message.channel_id,
                message_id: message.id,
                author_id: message.author.id,
                author: message_display_name(&message),
                author_avatar_url: Some(user_avatar_url(&message.author)),
                message_kind: MessageKind::new(message.kind.into()),
                reference,
                reply,
                poll,
                content: Some(message.content.clone()),
                mentions: mention_infos(&message.mentions),
                attachments: message
                    .attachments
                    .clone()
                    .into_iter()
                    .map(AttachmentInfo::from_attachment)
                    .collect(),
                forwarded_snapshots: message
                    .message_snapshots
                    .clone()
                    .into_iter()
                    .map(|snapshot| MessageSnapshotInfo::from_snapshot(snapshot, source_channel_id))
                    .collect(),
            }]
        }
        Event::MessageUpdate(message) => vec![AppEvent::MessageUpdate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            poll: message
                .poll
                .as_ref()
                .map(PollInfo::from_poll)
                .or_else(|| poll_result_info(&message.embeds)),
            content: Some(message.content.clone()),
            mentions: Some(mention_infos(&message.mentions)),
            attachments: map_attachment_update(message.attachments.clone()),
        }],
        Event::MessageDelete(message) => vec![AppEvent::MessageDelete {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
        }],
        Event::MemberChunk(chunk) => member_chunk_events(&chunk),
        Event::MemberAdd(member_add) => vec![member_upsert_from_add(&member_add)],
        Event::MemberUpdate(update) => vec![member_upsert_from_update(&update)],
        Event::MemberRemove(remove) => vec![AppEvent::GuildMemberRemove {
            guild_id: remove.guild_id,
            user_id: remove.user.id,
        }],
        Event::PresenceUpdate(presence) => vec![presence_update(&presence)],
        _ => Vec::new(),
    }
}

fn message_reference_info(
    reference: &Option<twilight_model::channel::message::MessageReference>,
) -> Option<MessageReferenceInfo> {
    reference.as_ref().map(|reference| MessageReferenceInfo {
        guild_id: reference.guild_id,
        channel_id: reference.channel_id,
        message_id: reference.message_id,
    })
}

fn map_attachment_update(attachments: Vec<Attachment>) -> AttachmentUpdate {
    AttachmentUpdate::Replace(
        attachments
            .into_iter()
            .map(AttachmentInfo::from_attachment)
            .collect(),
    )
}

fn map_guild_create(guild: GuildCreatePayload) -> Option<AppEvent> {
    let guild = match guild {
        GuildCreatePayload::Available(guild) => guild,
        GuildCreatePayload::Unavailable(_) => return None,
    };

    let channels = guild
        .channels
        .iter()
        .chain(guild.threads.iter())
        .map(channel_info)
        .collect();
    let members = guild.members.iter().map(member_info).collect();
    let presences = guild
        .presences
        .iter()
        .map(|presence| (presence.user.id(), map_status(presence.status)))
        .collect();
    let roles = guild.roles.iter().map(role_info).collect();
    let emojis = guild.emojis.iter().map(custom_emoji_info).collect();

    Some(AppEvent::GuildCreate {
        guild_id: guild.id,
        name: guild.name,
        channels,
        members,
        presences,
        roles,
        emojis,
    })
}

fn role_info(role: &TwilightRole) -> RoleInfo {
    let color = role_color(role.colors.primary_color).or_else(|| {
        #[allow(deprecated)]
        {
            role_color(role.color)
        }
    });

    RoleInfo {
        id: role.id,
        name: role.name.clone(),
        color,
        position: role.position,
        hoist: role.hoist,
    }
}

fn role_color(color: u32) -> Option<u32> {
    (color != 0).then_some(color)
}

fn custom_emoji_info(emoji: &TwilightEmoji) -> CustomEmojiInfo {
    CustomEmojiInfo {
        id: emoji.id,
        name: emoji.name.clone(),
        animated: emoji.animated,
        available: emoji.available,
    }
}

fn guild_emojis_update(payload: &GuildEmojisUpdatePayload) -> AppEvent {
    AppEvent::GuildEmojisUpdate {
        guild_id: payload.guild_id,
        emojis: payload.emojis.iter().map(custom_emoji_info).collect(),
    }
}

fn channel_info(channel: &Channel) -> ChannelInfo {
    ChannelInfo {
        guild_id: channel.guild_id,
        channel_id: channel.id,
        parent_id: channel.parent_id,
        position: channel.position,
        last_message_id: channel.last_message_id.map(|id| Id::new(id.get())),
        name: channel
            .name
            .clone()
            .unwrap_or_else(|| format!("channel-{}", channel.id.get())),
        kind: format!("{:?}", channel.kind),
        message_count: channel.message_count.map(u64::from),
        total_message_sent: None,
        thread_archived: channel
            .thread_metadata
            .as_ref()
            .map(|metadata| metadata.archived),
        thread_locked: channel
            .thread_metadata
            .as_ref()
            .map(|metadata| metadata.locked),
    }
}

fn poll_result_info(embeds: &[Embed]) -> Option<PollInfo> {
    let embed = embeds.iter().find(|embed| embed.kind == "poll_result")?;
    poll_result_info_from_fields(
        embed
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.value.as_str())),
    )
}

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

fn member_info(member: &TwilightMember) -> MemberInfo {
    MemberInfo {
        user_id: member.user.id,
        display_name: display_name(member.nick.as_deref(), &member.user),
        is_bot: member.user.bot,
        avatar_url: Some(user_avatar_url(&member.user)),
        role_ids: member.roles.clone(),
    }
}

fn member_upsert_from_add(payload: &MemberAdd) -> AppEvent {
    AppEvent::GuildMemberUpsert {
        guild_id: payload.guild_id,
        member: member_info(&payload.member),
    }
}

fn member_upsert_from_update(update: &MemberUpdate) -> AppEvent {
    AppEvent::GuildMemberUpsert {
        guild_id: update.guild_id,
        member: MemberInfo {
            user_id: update.user.id,
            display_name: display_name(update.nick.as_deref(), &update.user),
            is_bot: update.user.bot,
            avatar_url: Some(user_avatar_url(&update.user)),
            role_ids: update.roles.clone(),
        },
    }
}

fn member_chunk_events(chunk: &TwilightMemberChunk) -> Vec<AppEvent> {
    let mut events: Vec<AppEvent> = chunk
        .members
        .iter()
        .map(|member| AppEvent::GuildMemberUpsert {
            guild_id: chunk.guild_id,
            member: member_info(member),
        })
        .collect();

    events.extend(
        chunk
            .presences
            .iter()
            .map(|presence| AppEvent::PresenceUpdate {
                guild_id: chunk.guild_id,
                user_id: presence.user.id(),
                status: map_status(presence.status),
            }),
    );

    events
}

fn presence_update(payload: &PresenceUpdatePayload) -> AppEvent {
    AppEvent::PresenceUpdate {
        guild_id: payload.0.guild_id,
        user_id: match &payload.0.user {
            UserOrId::User(user) => user.id,
            UserOrId::UserId { id } => *id,
        },
        status: map_status(payload.0.status),
    }
}

fn map_status(status: TwilightStatus) -> PresenceStatus {
    match status {
        TwilightStatus::Online => PresenceStatus::Online,
        TwilightStatus::Idle => PresenceStatus::Idle,
        TwilightStatus::DoNotDisturb => PresenceStatus::DoNotDisturb,
        TwilightStatus::Offline | TwilightStatus::Invisible => PresenceStatus::Offline,
    }
}

fn mention_infos(mentions: &[Mention]) -> Vec<MentionInfo> {
    mentions.iter().map(mention_info).collect()
}

fn mention_info(mention: &Mention) -> MentionInfo {
    let display_name = mention
        .member
        .as_ref()
        .and_then(|member| member.nick.as_deref())
        .filter(|value| !value.is_empty())
        .unwrap_or(&mention.name)
        .to_owned();
    MentionInfo {
        user_id: mention.id,
        display_name,
    }
}

fn display_name(nick: Option<&str>, user: &TwilightUser) -> String {
    if let Some(nick) = nick.filter(|value| !value.is_empty()) {
        return nick.to_owned();
    }
    if let Some(global) = user
        .global_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return global.to_owned();
    }
    user.name.clone()
}

fn user_avatar_url(user: &TwilightUser) -> String {
    match user.avatar.as_ref() {
        Some(hash) => {
            let extension = if hash.is_animated() { "gif" } else { "png" };
            format!(
                "https://cdn.discordapp.com/avatars/{}/{}.{}",
                user.id, hash, extension
            )
        }
        None => default_avatar_url(user.id, user.discriminator),
    }
}

pub(crate) fn default_avatar_url(user_id: Id<UserMarker>, discriminator: u16) -> String {
    let index = if discriminator == 0 {
        (user_id.get() >> 22) % 6
    } else {
        u64::from(discriminator % 5)
    };

    format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
}

fn message_display_name(message: &Message) -> String {
    display_name(
        message
            .member
            .as_ref()
            .and_then(|member| member.nick.as_deref()),
        &message.author,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::{
        gateway::payload::incoming::{
            GuildEmojisUpdate as TwilightGuildEmojisUpdate, GuildUpdate as TwilightGuildUpdate,
        },
        guild::{
            AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, MfaLevel,
            NSFWLevel, PartialGuild, PremiumTier, SystemChannelFlags, VerificationLevel,
        },
    };

    #[test]
    fn video_attachment_with_dimensions_is_not_an_image_preview() {
        let attachment = attachment_info("clip.mp4", Some("video/mp4"));

        assert!(!attachment.is_image());
        assert!(attachment.is_video());
        assert_eq!(attachment.inline_preview_url(), None);
    }

    #[test]
    fn image_attachment_uses_preferred_inline_preview_url() {
        let attachment = attachment_info("cat.png", Some("image/png"));

        assert!(attachment.is_image());
        assert!(!attachment.is_video());
        assert_eq!(
            attachment.inline_preview_url(),
            Some("https://cdn.discordapp.com/cat.png")
        );
    }

    #[test]
    fn filename_extension_classifies_unknown_media_type() {
        assert!(attachment_info("CAT.PNG", None).is_image());
        assert!(attachment_info("CLIP.MP4", None).is_video());
    }

    #[test]
    fn display_name_prefers_nick_then_global_name_then_username() {
        let user_with_global = user("neo", Some("global alias"));

        assert_eq!(
            display_name(Some("server alias"), &user_with_global),
            "server alias"
        );
        assert_eq!(display_name(None, &user_with_global), "global alias");
        assert_eq!(display_name(None, &user("neo", None)), "neo");
    }

    #[test]
    fn message_update_empty_mentions_can_clear_cached_mentions() {
        assert_eq!(mention_infos(&[]), Vec::<MentionInfo>::new());
    }

    #[test]
    fn message_update_empty_attachments_can_clear_cached_attachments() {
        assert!(matches!(
            map_attachment_update(Vec::new()),
            AttachmentUpdate::Replace(attachments) if attachments.is_empty()
        ));
    }

    #[test]
    fn twilight_custom_emoji_maps_to_app_emoji_info() {
        let emoji = twilight_emoji(50, "party", true, true);

        let info = custom_emoji_info(&emoji);

        assert_eq!(info.id, Id::new(50));
        assert_eq!(info.name, "party");
        assert!(info.animated);
        assert!(info.available);
    }

    #[test]
    fn typed_guild_emojis_update_maps_custom_emojis() {
        let event = Event::GuildEmojisUpdate(TwilightGuildEmojisUpdate {
            guild_id: Id::new(10),
            emojis: vec![twilight_emoji(50, "party", true, true)],
        });

        let app_event = map_event(event);

        assert!(matches!(
            app_event.as_slice(),
            [AppEvent::GuildEmojisUpdate { guild_id, emojis }]
                if *guild_id == Id::new(10)
                    && *emojis == vec![CustomEmojiInfo {
                        id: Id::new(50),
                        name: "party".to_owned(),
                        animated: true,
                        available: true,
                    }]
        ));
    }

    #[test]
    fn typed_guild_update_maps_custom_emojis() {
        let event = Event::GuildUpdate(Box::new(TwilightGuildUpdate(partial_guild(
            10,
            "Renamed Guild",
            vec![twilight_emoji(51, "wave", false, false)],
        ))));

        let app_event = map_event(event);

        assert!(matches!(
            app_event.as_slice(),
            [AppEvent::GuildUpdate {
                guild_id,
                name,
                roles: Some(roles),
                emojis: Some(emojis),
            }] if *guild_id == Id::new(10)
                && name == "Renamed Guild"
                && roles.is_empty()
                && *emojis == vec![CustomEmojiInfo {
                    id: Id::new(51),
                    name: "wave".to_owned(),
                    animated: false,
                    available: false,
                }]
        ));
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
    fn message_reference_maps_thread_ids() {
        let reference = Some(twilight_model::channel::message::MessageReference {
            channel_id: Some(Id::new(10)),
            guild_id: Some(Id::new(1)),
            kind: twilight_model::channel::message::MessageReferenceType::Default,
            message_id: Some(Id::new(20)),
            fail_if_not_exists: None,
        });

        let mapped = message_reference_info(&reference).expect("reference should map");

        assert_eq!(mapped.guild_id, Some(Id::new(1)));
        assert_eq!(mapped.channel_id, Some(Id::new(10)));
        assert_eq!(mapped.message_id, Some(Id::new(20)));
    }

    fn twilight_emoji(id: u64, name: &str, animated: bool, available: bool) -> TwilightEmoji {
        TwilightEmoji {
            animated,
            available,
            id: Id::new(id),
            managed: false,
            name: name.to_owned(),
            require_colons: true,
            roles: Vec::new(),
            user: None,
        }
    }

    fn partial_guild(id: u64, name: &str, emojis: Vec<TwilightEmoji>) -> PartialGuild {
        PartialGuild {
            afk_channel_id: None,
            afk_timeout: AfkTimeout::FIFTEEN_MINUTES,
            application_id: None,
            banner: None,
            default_message_notifications: DefaultMessageNotificationLevel::Mentions,
            description: None,
            discovery_splash: None,
            emojis,
            explicit_content_filter: ExplicitContentFilter::MembersWithoutRole,
            features: Vec::new(),
            icon: None,
            id: Id::new(id),
            max_members: None,
            max_presences: None,
            member_count: None,
            mfa_level: MfaLevel::Elevated,
            name: name.to_owned(),
            nsfw_level: NSFWLevel::Default,
            owner_id: Id::new(5),
            owner: None,
            permissions: None,
            preferred_locale: "en-us".to_owned(),
            premium_progress_bar_enabled: false,
            premium_subscription_count: None,
            premium_tier: PremiumTier::Tier1,
            public_updates_channel_id: None,
            roles: Vec::new(),
            rules_channel_id: None,
            splash: None,
            system_channel_flags: SystemChannelFlags::empty(),
            system_channel_id: None,
            verification_level: VerificationLevel::Medium,
            vanity_url_code: None,
            widget_channel_id: None,
            widget_enabled: None,
        }
    }

    fn user(name: &str, global_name: Option<&str>) -> TwilightUser {
        TwilightUser {
            accent_color: None,
            avatar: None,
            avatar_decoration: None,
            avatar_decoration_data: None,
            banner: None,
            bot: false,
            discriminator: 0,
            email: None,
            flags: None,
            global_name: global_name.map(str::to_owned),
            id: Id::new(1),
            locale: None,
            mfa_enabled: None,
            name: name.to_owned(),
            premium_type: None,
            primary_guild: None,
            public_flags: None,
            system: None,
            verified: None,
        }
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
