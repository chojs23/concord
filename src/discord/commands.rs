use twilight_model::id::{Id, marker::ChannelMarker};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    SendMessage {
        channel_id: Id<ChannelMarker>,
        content: String,
    },
}
