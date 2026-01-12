use twilight_model::id::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
    },
    LoadAttachmentPreview {
        url: String,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
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
        emoji: String,
    },
}
