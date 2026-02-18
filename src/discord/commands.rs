use twilight_model::id::{
    Id,
    marker::{ChannelMarker, EmojiMarker, GuildMarker, MessageMarker},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReactionEmoji {
    Unicode(String),
    Custom {
        id: Id<EmojiMarker>,
        name: Option<String>,
        animated: bool,
    },
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
    },
    LoadGuildMembers {
        guild_id: Id<GuildMarker>,
    },
    LoadAttachmentPreview {
        url: String,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
        reply_to: Option<Id<MessageMarker>>,
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
}
