use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::ReactionUsersInfo;

use super::{EmojiReactionItem, PollVotePickerItem};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionMenuState {
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OptionsPopupState {
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ImageViewerState {
    pub(super) message_id: Id<MessageMarker>,
    pub(super) selected: usize,
    pub(super) download_message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GuildLeaderActionState {
    Actions { selected: usize },
    MuteDuration { selected: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UserProfilePopupState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) load_error: Option<String>,
    /// First visible row of the popup body. Behaves like the channel/guild
    /// pane scroll: j/k and the mouse wheel adjust this, never moving a
    /// cursor that the renderer would have to chase.
    pub(super) scroll: usize,
    /// Last rendered viewport height for the popup body. The renderer
    /// updates it each frame so scroll-clamping has the latest figure.
    pub(super) view_height: usize,
    /// Last rendered total content height. Same reason as `view_height`.
    pub(super) total_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MemberLeaderActionState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ChannelLeaderActionState {
    Actions {
        channel_id: Id<ChannelMarker>,
        selected: usize,
    },
    MuteDuration {
        channel_id: Id<ChannelMarker>,
        selected: usize,
    },
    Threads {
        channel_id: Id<ChannelMarker>,
        selected: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmojiReactionPickerState {
    pub(super) selected: usize,
    pub(super) filter: Option<String>,
    pub(super) items: Vec<EmojiReactionItem>,
    pub(super) filtered_items: Vec<EmojiReactionItem>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollVotePickerState {
    pub(super) selected: usize,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) answers: Vec<PollVotePickerItem>,
}

impl PollVotePickerState {
    pub fn answers(&self) -> &[PollVotePickerItem] {
        &self.answers
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersPopupState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) reactions: Vec<ReactionUsersInfo>,
    pub(super) scroll: usize,
    pub(super) view_height: usize,
}

impl ReactionUsersPopupState {
    pub fn reactions(&self) -> &[ReactionUsersInfo] {
        &self.reactions
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Total renderable data lines for the current reactions, mirroring the
    /// layout produced by `reaction_users_popup_data_lines` in `ui.rs` so the
    /// scroll bound here stays in sync with what the user actually sees.
    pub fn data_line_count(&self) -> usize {
        if self.reactions.is_empty() {
            return 1;
        }
        self.reactions
            .iter()
            .map(|reaction| 1 + reaction.users.len().max(1))
            .sum()
    }

    fn max_scroll(&self) -> usize {
        let visible = self.view_height.min(self.data_line_count());
        self.data_line_count().saturating_sub(visible)
    }

    pub(super) fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }
}
