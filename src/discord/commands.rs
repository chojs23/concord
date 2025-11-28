use twilight_model::id::{Id, marker::ChannelMarker};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    LoadMessageHistory {
        channel_id: Id<ChannelMarker>,
    },
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
    },
}
