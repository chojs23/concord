mod state;

pub(in crate::discord) use state::ChannelReadState;
pub(in crate::discord) use state::NonChannelReadState;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

/// One entry from `READY.read_state.entries[]`. The Discord wire field
/// `last_message_id` is renamed here because it actually carries the
/// last *ACKED* id, not the newest message in the channel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadStateInfo {
    pub read_state_type: u8,
    pub channel_id: Id<ChannelMarker>,
    pub last_acked_message_id: Option<Id<MessageMarker>>,
    pub mention_count: u32,
    pub badge_count: u32,
    pub last_pin_timestamp: Option<String>,
    pub flags: u64,
    pub last_viewed: Option<u64>,
}

#[cfg(test)]
#[allow(dead_code)]
impl ReadStateInfo {
    pub(crate) fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            read_state_type: 0,
            channel_id,
            last_acked_message_id: None,
            mention_count: 0,
            badge_count: 0,
            last_pin_timestamp: None,
            flags: 0,
            last_viewed: None,
        }
    }
}
