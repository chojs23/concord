use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::ReactionUsersInfo;

use super::PollVotePickerItem;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionMenuState {
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UserProfilePopupState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) load_error: Option<String>,
    /// `Some(index)` once the user has moved into the mutual server list with
    /// j/k. `None` while the popup is purely informational. Enter on a
    /// selected mutual server activates that guild and closes the popup.
    pub(super) mutual_cursor: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MemberActionMenuState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ChannelActionMenuState {
    Actions {
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
