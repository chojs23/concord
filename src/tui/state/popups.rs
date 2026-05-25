use crate::discord::ReactionEmoji;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::ReactionUsersInfo;
use crate::tui::keybindings::{KeyChord, LeaderShortcutItem};

use super::{
    DashboardState, EmojiReactionItem, FocusPane, MessageUrlItem, PollVotePickerItem,
    channel_switcher::ChannelSwitcherState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeaderMode {
    Root,
    Actions,
}

#[derive(Debug, Default)]
pub(super) struct PopupUiState {
    pub(super) message_action_menu: Option<MessageActionMenuState>,
    pub(super) message_url_picker: Option<MessageUrlPickerState>,
    pub(super) message_delete_confirmation: Option<MessageDeleteConfirmationState>,
    pub(super) message_pin_confirmation: Option<MessagePinConfirmationState>,
    pub(super) options_popup: Option<OptionsPopupState>,
    pub(super) attachment_viewer: Option<AttachmentViewerState>,
    pub(super) guild_leader_action: Option<GuildLeaderActionState>,
    pub(super) channel_leader_action: Option<ChannelLeaderActionState>,
    pub(super) member_leader_action: Option<MemberLeaderActionState>,
    pub(super) user_profile_popup: Option<UserProfilePopupState>,
    pub(super) emoji_reaction_picker: Option<EmojiReactionPickerState>,
    pub(super) poll_vote_picker: Option<PollVotePickerState>,
    pub(super) reaction_users_popup: Option<ReactionUsersPopupState>,
    pub(super) debug_log_popup_open: bool,
    pub(super) leader_mode: Option<LeaderMode>,
    pub(super) leader_keymap_prefix: Vec<KeyChord>,
    pub(super) channel_switcher: Option<ChannelSwitcherState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageActionMenuState {
    pub(super) selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUrlPickerState {
    pub(super) selected: usize,
    pub(super) items: Vec<MessageUrlItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDeleteConfirmationState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) author: String,
    pub(super) content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessagePinConfirmationState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) pinned: bool,
    pub(super) author: String,
    pub(super) content: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategory {
    Display,
    Notifications,
    Voice,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct OptionsPopupState {
    pub(super) selected: usize,
    pub(super) category: Option<OptionsCategory>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AttachmentViewerState {
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
    pub(super) filter_editing: bool,
    pub(super) items: Vec<EmojiReactionItem>,
    pub(super) filtered_items: Vec<EmojiReactionItem>,
    pub(super) existing_reactions: Vec<ReactionEmoji>,
    pub(super) own_reactions: Vec<ReactionEmoji>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollVotePickerState {
    pub(super) selected: usize,
    pub(super) allow_multiselect: bool,
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

impl DashboardState {
    pub fn is_leader_active(&self) -> bool {
        self.popups.leader_mode.is_some()
    }

    pub fn is_leader_action_mode(&self) -> bool {
        self.popups.leader_mode == Some(LeaderMode::Actions)
    }

    pub fn open_leader(&mut self) {
        self.popups.leader_mode = Some(LeaderMode::Root);
        self.popups.leader_keymap_prefix = self.options.key_bindings.leader_keymap_prefix();
    }

    pub(in crate::tui) fn open_keymap_prefix(&mut self, prefix: Vec<KeyChord>) {
        self.popups.leader_mode = Some(LeaderMode::Root);
        self.popups.leader_keymap_prefix = prefix;
    }

    pub fn close_leader(&mut self) {
        self.popups.leader_mode = None;
        self.popups.leader_keymap_prefix.clear();
    }

    pub(in crate::tui) fn leader_keymap_prefix(&self) -> &[KeyChord] {
        &self.popups.leader_keymap_prefix
    }

    pub(in crate::tui) fn push_leader_keymap_key(&mut self, key: KeyChord) {
        self.popups.leader_keymap_prefix.push(key);
    }

    pub fn leader_keymap_shortcuts(&self) -> Vec<LeaderShortcutItem> {
        self.options
            .key_bindings
            .leader_keymap_children(&self.popups.leader_keymap_prefix)
    }

    pub(in crate::tui) fn leader_keymap_title(&self) -> String {
        self.options
            .key_bindings
            .keymap_prefix_title(&self.popups.leader_keymap_prefix)
    }

    pub fn open_leader_actions_for_focused_target(&mut self) {
        self.close_all_action_contexts();
        match self.navigation.focus {
            FocusPane::Guilds => self.open_selected_guild_actions(),
            FocusPane::Channels => self.open_selected_channel_actions(),
            FocusPane::Messages => self.open_selected_message_actions(),
            FocusPane::Members => self.open_selected_member_actions(),
        }
        self.popups.leader_mode = Some(LeaderMode::Actions);
    }

    pub fn close_all_action_contexts(&mut self) {
        self.popups.message_action_menu = None;
        self.popups.guild_leader_action = None;
        self.popups.channel_leader_action = None;
        self.popups.member_leader_action = None;
    }

    pub fn is_any_action_context_active(&self) -> bool {
        self.popups.message_action_menu.is_some()
            || self.popups.guild_leader_action.is_some()
            || self.popups.channel_leader_action.is_some()
            || self.popups.member_leader_action.is_some()
    }
}

impl DashboardState {
    pub fn is_channel_leader_action_active(&self) -> bool {
        self.popups.channel_leader_action.is_some()
    }

    pub fn is_guild_leader_action_active(&self) -> bool {
        self.popups.guild_leader_action.is_some()
    }

    pub fn is_channel_action_threads_phase(&self) -> bool {
        matches!(
            self.popups.channel_leader_action,
            Some(ChannelLeaderActionState::Threads { .. })
        )
    }

    pub fn is_channel_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.channel_leader_action,
            Some(ChannelLeaderActionState::MuteDuration { .. })
        )
    }

    pub fn is_guild_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.guild_leader_action,
            Some(GuildLeaderActionState::MuteDuration { .. })
        )
    }
}
