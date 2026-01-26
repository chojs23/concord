use twilight_gateway::Event;
use twilight_model::{
    channel::{Attachment, Channel, Message, message::MessageSnapshot},
    gateway::{
        payload::incoming::{
            GuildCreate as GuildCreatePayload, MemberAdd, MemberUpdate,
            PresenceUpdate as PresenceUpdatePayload,
        },
        presence::{Status as TwilightStatus, UserOrId},
    },
    guild::Member as TwilightMember,
    id::{
        Id,
        marker::{AttachmentMarker, ChannelMarker, GuildMarker, MessageMarker, UserMarker},
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
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

    pub const fn label(self) -> &'static str {
        match self.code {
            0 => "Default",
            1 => "Recipient add",
            2 => "Recipient remove",
            3 => "Call",
            4 => "Channel name change",
            5 => "Channel icon change",
            6 => "Pinned message",
            7 => "User join",
            8 => "Guild boost",
            9 => "Guild boost tier 1",
            10 => "Guild boost tier 2",
            11 => "Guild boost tier 3",
            12 => "Channel follow add",
            14 => "Guild discovery disqualified",
            15 => "Guild discovery requalified",
            16 => "Guild discovery initial warning",
            17 => "Guild discovery final warning",
            18 => "Thread created",
            19 => "Reply",
            20 => "Chat input command",
            21 => "Thread starter message",
            22 => "Guild invite reminder",
            23 => "Context menu command",
            24 => "Auto moderation action",
            25 => "Role subscription purchase",
            26 => "Premium upsell",
            27 => "Stage start",
            28 => "Stage end",
            29 => "Stage speaker",
            31 => "Stage topic",
            32 => "Application premium subscription",
            36 => "Incident alert mode enabled",
            37 => "Incident alert mode disabled",
            38 => "Incident raid report",
            39 => "Incident false alarm report",
            44 => "Purchase notification",
            46 => "Poll result",
            _ => "Unknown message type",
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
    pub attachments: Vec<AttachmentInfo>,
    pub source_channel_id: Option<Id<ChannelMarker>>,
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyInfo {
    pub author: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollInfo {
    pub question: String,
    pub answers: Vec<PollAnswerInfo>,
    pub allow_multiselect: bool,
    pub results_finalized: Option<bool>,
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
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub content: Option<String>,
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
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        channels: Vec<ChannelInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<(Id<UserMarker>, PresenceStatus)>,
    },
    GuildUpdate {
        guild_id: Id<GuildMarker>,
        name: String,
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
        reply: Option<ReplyInfo>,
        poll: Option<PollInfo>,
        content: Option<String>,
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
            reply: message.reply,
            poll: message.poll,
            content: message.content,
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
        Self {
            content: Some(message.content),
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
        let source_channel_id = message
            .reference
            .as_ref()
            .and_then(|reference| reference.channel_id);
        let reply = message
            .referenced_message
            .as_deref()
            .and_then(ReplyInfo::from_message);
        let poll = message.poll.as_ref().map(PollInfo::from_poll);
        Self {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message_display_name(&message),
            author_avatar_url: Some(user_avatar_url(&message.author)),
            message_kind: MessageKind::new(message.kind.into()),
            reply,
            poll,
            content: Some(message.content),
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

pub fn map_event(event: Event) -> Option<AppEvent> {
    match event {
        Event::Ready(ready) => Some(AppEvent::Ready {
            user: ready.user.name,
        }),
        Event::GuildCreate(guild) => map_guild_create(*guild),
        Event::GuildDelete(guild) => Some(AppEvent::GuildDelete { guild_id: guild.id }),
        Event::GuildUpdate(guild) => Some(AppEvent::GuildUpdate {
            guild_id: guild.id,
            name: guild.name.clone(),
        }),
        Event::ChannelCreate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelUpdate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelDelete(channel) => Some(AppEvent::ChannelDelete {
            guild_id: channel.guild_id,
            channel_id: channel.id,
        }),
        Event::MessageCreate(message) => {
            let source_channel_id = message
                .reference
                .as_ref()
                .and_then(|reference| reference.channel_id);
            let reply = message
                .referenced_message
                .as_deref()
                .and_then(ReplyInfo::from_message);
            let poll = message.poll.as_ref().map(PollInfo::from_poll);

            Some(AppEvent::MessageCreate {
                guild_id: message.guild_id,
                channel_id: message.channel_id,
                message_id: message.id,
                author_id: message.author.id,
                author: message_display_name(&message),
                author_avatar_url: Some(user_avatar_url(&message.author)),
                message_kind: MessageKind::new(message.kind.into()),
                reply,
                poll,
                content: Some(message.content.clone()),
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
            })
        }
        Event::MessageUpdate(message) => Some(AppEvent::MessageUpdate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            poll: message.poll.as_ref().map(PollInfo::from_poll),
            content: Some(message.content.clone()),
            attachments: map_attachment_update(message.attachments.clone()),
        }),
        Event::MessageDelete(message) => Some(AppEvent::MessageDelete {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
        }),
        Event::MemberAdd(member_add) => Some(member_upsert_from_add(&member_add)),
        Event::MemberUpdate(update) => Some(member_upsert_from_update(&update)),
        Event::MemberRemove(remove) => Some(AppEvent::GuildMemberRemove {
            guild_id: remove.guild_id,
            user_id: remove.user.id,
        }),
        Event::PresenceUpdate(presence) => Some(presence_update(&presence)),
        _ => None,
    }
}

fn map_attachment_update(attachments: Vec<Attachment>) -> AttachmentUpdate {
    if attachments.is_empty() {
        AttachmentUpdate::Unchanged
    } else {
        AttachmentUpdate::Replace(
            attachments
                .into_iter()
                .map(AttachmentInfo::from_attachment)
                .collect(),
        )
    }
}

fn map_guild_create(guild: GuildCreatePayload) -> Option<AppEvent> {
    let guild = match guild {
        GuildCreatePayload::Available(guild) => guild,
        GuildCreatePayload::Unavailable(_) => return None,
    };

    let channels = guild.channels.iter().map(channel_info).collect();
    let members = guild.members.iter().map(member_info).collect();
    let presences = guild
        .presences
        .iter()
        .map(|presence| (presence.user.id(), map_status(presence.status)))
        .collect();

    Some(AppEvent::GuildCreate {
        guild_id: guild.id,
        name: guild.name,
        channels,
        members,
        presences,
    })
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
    }
}

fn member_info(member: &TwilightMember) -> MemberInfo {
    MemberInfo {
        user_id: member.user.id,
        display_name: display_name(member.nick.as_deref(), &member.user),
        is_bot: member.user.bot,
        avatar_url: Some(user_avatar_url(&member.user)),
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
        },
    }
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
