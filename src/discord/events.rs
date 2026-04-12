use crate::discord::ids::{
    Id,
    marker::{
        AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker,
        UserMarker,
    },
};

use super::commands::ReactionEmoji;

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
    pub fields: Vec<EmbedFieldInfo>,
    pub footer_text: Option<String>,
    pub url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_width: Option<u64>,
    pub thumbnail_height: Option<u64>,
    pub image_url: Option<String>,
    pub image_width: Option<u64>,
    pub image_height: Option<u64>,
    pub video_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlinePreviewInfo<'a> {
    pub url: &'a str,
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
    pub embeds: Vec<EmbedInfo>,
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
    pub message_kind: MessageKind,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub pinned: bool,
    pub reactions: Vec<ReactionInfo>,
    pub content: Option<String>,
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
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
        embeds: Vec<EmbedInfo>,
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
        embeds: Option<Vec<EmbedInfo>>,
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
    },
    UserPresenceUpdate {
        user_id: Id<UserMarker>,
        status: PresenceStatus,
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
    MessagePinnedUpdate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
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
    UserProfileLoaded {
        guild_id: Option<Id<GuildMarker>>,
        profile: UserProfileInfo,
    },
    UserProfileLoadFailed {
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        message: String,
    },
    RelationshipsLoaded {
        relationships: Vec<(Id<UserMarker>, FriendStatus)>,
    },
    RelationshipUpsert {
        user_id: Id<UserMarker>,
        status: FriendStatus,
    },
    RelationshipRemove {
        user_id: Id<UserMarker>,
    },
    GatewayClosed,
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
                filename: "embed-thumbnail",
                width: self.thumbnail_width,
                height: self.thumbnail_height,
                accent_color: Some(self.color.unwrap_or(0xff0000)),
            });
        }

        self.image_url.as_deref().map(|url| InlinePreviewInfo {
            url,
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
