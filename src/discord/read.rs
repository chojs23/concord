mod state;

pub(in crate::discord) use state::ChannelReadState;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

/// One entry from `READY.read_state.entries[]`. The Discord wire field
/// `last_message_id` is renamed here because it actually carries the
/// last *ACKED* id, not the newest message in the channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadStateInfo {
    pub channel_id: Id<ChannelMarker>,
    pub last_acked_message_id: Option<Id<MessageMarker>>,
    pub mention_count: u32,
}

#[cfg(test)]
#[allow(dead_code)]
impl ReadStateInfo {
    pub(crate) fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            last_acked_message_id: None,
            mention_count: 0,
        }
    }
}
