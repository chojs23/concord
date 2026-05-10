use std::path::PathBuf;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, UserMarker},
};

pub const MAX_UPLOAD_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_UPLOAD_TOTAL_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_UPLOAD_ATTACHMENT_COUNT: usize = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAttachmentUpload {
    pub path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
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
    LoadGuildMembers {
        guild_id: Id<GuildMarker>,
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
    LoadAttachmentPreview {
        url: String,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
        reply_to: Option<Id<MessageMarker>>,
        attachments: Vec<MessageAttachmentUpload>,
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
    DownloadAttachment {
        url: String,
        filename: String,
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
    AckChannel {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    AckChannels {
        targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)>,
    },
}
