use std::collections::{HashMap, HashSet};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::{
    AppCommand, AppEvent, ChannelState, ChannelVisibilityStats, DiscordState, GuildFolder,
    GuildState, MentionInfo, MessageInfo, MessageSnapshotInfo, MessageState, PresenceStatus,
    UserProfileInfo,
};
use crate::logging;

use super::format::{
    RenderedText, TextHighlightKind, render_user_mentions, render_user_mentions_with_highlights,
};
use super::{media, message_format, ui};

mod composer;
mod emoji;
mod member_grouping;
mod message_render;
mod model;
mod popups;
mod presentation;
mod scroll;

use composer::{
    MentionCompletion, build_mention_candidates, expand_mention_completions, is_mention_query_char,
    move_mention_selection, should_start_mention_query,
};
use emoji::{custom_emoji_reaction_item, unicode_emoji_reaction_items};
use member_grouping::{channel_recipient_group, flatten_member_groups, guild_member_groups};
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use popups::{ChannelActionMenuState, MemberActionMenuState, UserProfilePopupState};
use presentation::{is_direct_message_channel, sort_channels, sort_direct_message_channels};
use scroll::{
    SCROLL_OFF, clamp_list_scroll, clamp_selected_index, close_collapsed_key, last_index,
    move_index_down, move_index_down_by, move_index_up, move_index_up_by,
    normalize_message_line_scroll, open_collapsed_key, pane_content_height,
    scroll_message_row_down, scroll_message_row_up, toggle_collapsed_key,
};

pub use composer::{MAX_MENTION_PICKER_VISIBLE, MentionPickerEntry};
pub use member_grouping::{MemberEntry, MemberGroup};
pub use model::{
    ChannelActionItem, ChannelActionKind, ChannelBranch, ChannelPaneEntry, ChannelThreadItem,
    EmojiReactionItem, FocusPane, GuildBranch, GuildPaneEntry, MemberActionItem, MemberActionKind,
    MessageActionItem, MessageActionKind, PollVotePickerItem, ThreadSummary,
};
pub use popups::{
    EmojiReactionPickerState, MessageActionMenuState, PollVotePickerState, ReactionUsersPopupState,
};
pub use presentation::{folder_color, presence_color, presence_marker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OlderHistoryRequestState {
    Requested { before: Id<MessageMarker> },
    Exhausted { before: Id<MessageMarker> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Debug)]
pub struct DashboardState {
    discord: DiscordState,
    focus: FocusPane,
    active_guild: ActiveGuildScope,
    active_channel_id: Option<Id<ChannelMarker>>,
    selected_guild: usize,
    guild_scroll: usize,
    guild_view_height: usize,
    selected_channel: usize,
    channel_scroll: usize,
    channel_view_height: usize,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    message_keep_selection_visible: bool,
    message_auto_follow: bool,
    message_view_height: usize,
    message_content_width: usize,
    message_preview_width: u16,
    message_max_preview_height: u16,
    selected_member: usize,
    member_scroll: usize,
    member_view_height: usize,
    composer_input: String,
    composer_active: bool,
    reply_target_message_id: Option<Id<MessageMarker>>,
    /// Set when the user is in the middle of an `@mention` autocomplete. The
    /// stored string is the characters typed *after* the `@` and is used to
    /// filter the candidate list. `None` means the picker is closed.
    composer_mention_query: Option<String>,
    composer_mention_selected: usize,
    /// Records `@displayname` substrings that the picker inserted, so the
    /// composer can rewrite them to Discord's `<@USER_ID>` wire format on
    /// submit even though the visible text is still the friendly form.
    composer_mention_completions: Vec<MentionCompletion>,
    message_action_menu: Option<MessageActionMenuState>,
    channel_action_menu: Option<ChannelActionMenuState>,
    member_action_menu: Option<MemberActionMenuState>,
    user_profile_popup: Option<UserProfilePopupState>,
    emoji_reaction_picker: Option<EmojiReactionPickerState>,
    poll_vote_picker: Option<PollVotePickerState>,
    reaction_users_popup: Option<ReactionUsersPopupState>,
    debug_log_popup_open: bool,
    current_user: Option<String>,
    current_user_id: Option<Id<UserMarker>>,
    last_error: Option<String>,
    last_status: Option<String>,
    skipped_events: u64,
    should_quit: bool,
    older_history_requests: HashMap<Id<ChannelMarker>, OlderHistoryRequestState>,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    collapsed_folders: HashSet<FolderKey>,
    collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum FolderKey {
    Id(u64),
    Guilds(Vec<Id<GuildMarker>>),
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            discord: DiscordState::default(),
            focus: FocusPane::Guilds,
            active_guild: ActiveGuildScope::Unset,
            active_channel_id: None,
            // Index 0 is the virtual "Direct Messages" entry. Start on the
            // first real guild when one exists; the bounds clamp inside
            // `selected_guild()` falls back to the DM entry while the guild
            // list is still empty.
            selected_guild: 1,
            guild_scroll: 0,
            guild_view_height: 1,
            selected_channel: 0,
            channel_scroll: 0,
            channel_view_height: 1,
            selected_message: 0,
            message_scroll: 0,
            message_line_scroll: 0,
            message_keep_selection_visible: true,
            message_auto_follow: true,
            message_view_height: 1,
            message_content_width: usize::MAX,
            message_preview_width: 0,
            message_max_preview_height: 0,
            selected_member: 0,
            member_scroll: 0,
            member_view_height: 1,
            composer_input: String::new(),
            composer_active: false,
            reply_target_message_id: None,
            composer_mention_query: None,
            composer_mention_selected: 0,
            composer_mention_completions: Vec::new(),
            message_action_menu: None,
            channel_action_menu: None,
            member_action_menu: None,
            user_profile_popup: None,
            emoji_reaction_picker: None,
            poll_vote_picker: None,
            reaction_users_popup: None,
            debug_log_popup_open: false,
            current_user: None,
            current_user_id: None,
            last_error: None,
            last_status: None,
            skipped_events: 0,
            should_quit: false,
            older_history_requests: HashMap::new(),
            collapsed_folders: HashSet::new(),
            collapsed_channel_categories: HashSet::new(),
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        let selected_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.selected_message())
                    .map(|message| message.id)
            })
            .flatten();
        let scroll_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let channel_cursor_id = self.selected_channel_cursor_id();

        match &event {
            AppEvent::Ready { user, user_id } => {
                self.current_user = Some(user.clone());
                self.current_user_id = *user_id;
            }
            AppEvent::GatewayError { message } => {
                self.last_error = Some(message.clone());
            }
            AppEvent::StatusMessage { message } => {
                self.last_status = Some(message.clone());
                self.last_error = None;
            }
            AppEvent::ReactionUsersLoaded {
                channel_id,
                message_id,
                reactions,
            } => {
                self.reaction_users_popup = Some(ReactionUsersPopupState {
                    channel_id: *channel_id,
                    message_id: *message_id,
                    reactions: reactions.clone(),
                    scroll: 0,
                    view_height: 0,
                });
                self.last_status = Some("loaded reacted users".to_owned());
                self.last_error = None;
            }
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                message,
            } => {
                self.last_error = Some(message.clone());
                self.older_history_requests.remove(channel_id);
            }
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => self.record_older_history_loaded(*channel_id, *before, messages),
            AppEvent::UserProfileLoadFailed {
                user_id,
                guild_id,
                message,
            } => {
                if let Some(popup) = self.user_profile_popup.as_mut()
                    && popup.user_id == *user_id
                    && popup.guild_id == *guild_id
                {
                    popup.load_error = Some(message.clone());
                }
            }
            AppEvent::GatewayClosed => {
                self.last_error = Some("gateway closed".to_owned());
            }
            _ => {}
        }
        self.discord.apply_event(&event);
        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.clamp_selection_indices();
        if self.message_auto_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    pub fn record_lag(&mut self, skipped: u64) {
        self.skipped_events += skipped;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn focus(&self) -> FocusPane {
        self.focus
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn last_status(&self) -> Option<&str> {
        self.last_status.as_deref()
    }

    pub fn skipped_events(&self) -> u64 {
        self.skipped_events
    }

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn is_message_action_menu_open(&self) -> bool {
        self.message_action_menu.is_some()
    }

    pub fn is_channel_action_menu_open(&self) -> bool {
        self.channel_action_menu.is_some()
    }

    pub fn is_user_profile_popup_open(&self) -> bool {
        self.user_profile_popup.is_some()
    }

    pub fn is_member_action_menu_open(&self) -> bool {
        self.member_action_menu.is_some()
    }

    pub fn member_action_menu_title(&self) -> Option<String> {
        let menu = self.member_action_menu.as_ref()?;
        let entries = self.flattened_members();
        let entry = entries.iter().find(|m| m.user_id() == menu.user_id)?;
        Some(entry.display_name())
    }

    /// Direct shortcut from the member pane: open the profile popup for the
    /// currently selected member without going through the action menu.
    pub fn show_selected_member_profile(&mut self) -> Option<AppCommand> {
        if self.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_selected_member_actions(&mut self) {
        if self.focus != FocusPane::Members {
            return;
        }
        let entries = self.flattened_members();
        let Some(entry) = entries.get(self.selected_member()) else {
            return;
        };
        let user_id = entry.user_id();
        // For DM/group-DM panes there is no guild context; pass it through so
        // the profile fetch can omit `guild_id` and skip the guild_member
        // section gracefully.
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.member_action_menu = Some(MemberActionMenuState {
            user_id,
            guild_id,
            selected: 0,
        });
    }

    pub fn close_member_action_menu(&mut self) {
        self.member_action_menu = None;
    }

    pub fn selected_member_action_items(&self) -> Vec<MemberActionItem> {
        if self.member_action_menu.is_none() {
            return Vec::new();
        }
        vec![MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }]
    }

    pub fn selected_member_action_index(&self) -> Option<usize> {
        let menu = self.member_action_menu.as_ref()?;
        Some(clamp_selected_index(
            menu.selected,
            self.selected_member_action_items().len(),
        ))
    }

    pub fn move_member_action_down(&mut self) {
        let len = self.selected_member_action_items().len();
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_down(&mut menu.selected, len);
        }
    }

    pub fn move_member_action_up(&mut self) {
        if let Some(menu) = self.member_action_menu.as_mut() {
            move_index_up(&mut menu.selected);
        }
    }

    pub fn activate_selected_member_action(&mut self) -> Option<AppCommand> {
        let menu = self.member_action_menu.clone()?;
        let items = self.selected_member_action_items();
        let item = items
            .get(clamp_selected_index(menu.selected, items.len()))?
            .clone();
        if !item.enabled {
            return None;
        }
        match item.kind {
            MemberActionKind::ShowProfile => {
                self.close_member_action_menu();
                self.open_user_profile_popup(menu.user_id, menu.guild_id)
            }
        }
    }

    /// Opens the profile popup for `user_id`. Returns
    /// `AppCommand::LoadUserProfile` to fetch fresh data when nothing is
    /// cached yet — the popup itself shows a loading state in the meantime.
    pub fn open_user_profile_popup(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        self.user_profile_popup = Some(UserProfilePopupState {
            user_id,
            guild_id,
            load_error: None,
            mutual_cursor: None,
        });
        if self.discord.user_profile(user_id, guild_id).is_some() {
            None
        } else {
            Some(AppCommand::LoadUserProfile { user_id, guild_id })
        }
    }

    pub fn close_user_profile_popup(&mut self) {
        self.user_profile_popup = None;
    }

    pub fn user_profile_popup_data(&self) -> Option<&UserProfileInfo> {
        let popup = self.user_profile_popup.as_ref()?;
        self.discord.user_profile(popup.user_id, popup.guild_id)
    }

    pub fn user_profile_popup_load_error(&self) -> Option<&str> {
        self.user_profile_popup
            .as_ref()
            .and_then(|popup| popup.load_error.as_deref())
    }

    pub fn user_profile_popup_status(&self) -> PresenceStatus {
        let Some(popup) = self.user_profile_popup.as_ref() else {
            return PresenceStatus::Unknown;
        };

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .members_for_guild(guild_id)
                .into_iter()
                .find(|member| member.user_id == popup.user_id)
                .map(|member| member.status)
        {
            return status;
        }

        self.discord
            .channels_for_guild(None)
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == popup.user_id)
            .map(|recipient| recipient.status)
            .unwrap_or(PresenceStatus::Unknown)
    }

    /// URL of the avatar image to render into the open profile popup. None
    /// when the popup is closed, the profile has not loaded yet, or the user
    /// has no avatar attachment.
    pub fn user_profile_popup_avatar_url(&self) -> Option<&str> {
        self.user_profile_popup_data()?.avatar_url.as_deref()
    }

    /// Index of the currently highlighted mutual-server line, if the user has
    /// moved into the list with j/k. None while the cursor sits idle.
    pub fn user_profile_popup_mutual_cursor(&self) -> Option<usize> {
        let popup = self.user_profile_popup.as_ref()?;
        let cursor = popup.mutual_cursor?;
        let len = self.user_profile_popup_data()?.mutual_guilds.len();
        if len == 0 {
            None
        } else {
            Some(cursor.min(len - 1))
        }
    }

    fn user_profile_popup_mutual_len(&self) -> usize {
        self.user_profile_popup_data()
            .map(|profile| profile.mutual_guilds.len())
            .unwrap_or(0)
    }

    pub fn move_user_profile_popup_down(&mut self) {
        let len = self.user_profile_popup_mutual_len();
        if len == 0 {
            return;
        }
        if let Some(popup) = self.user_profile_popup.as_mut() {
            let next = popup.mutual_cursor.map(|c| c + 1).unwrap_or(0);
            popup.mutual_cursor = Some(next.min(len - 1));
        }
    }

    pub fn move_user_profile_popup_up(&mut self) {
        if let Some(popup) = self.user_profile_popup.as_mut() {
            popup.mutual_cursor = match popup.mutual_cursor {
                Some(0) | None => Some(0),
                Some(c) => Some(c - 1),
            };
        }
    }

    /// Activates the mutual server highlighted in the popup and closes the
    /// popup. Returns None when no cursor is set or the data isn't loaded
    /// yet — Enter then falls through to a no-op (the caller can still rely
    /// on Esc to close).
    pub fn activate_selected_user_profile_mutual(&mut self) -> Option<AppCommand> {
        let cursor = self.user_profile_popup_mutual_cursor()?;
        let guild_id = self
            .user_profile_popup_data()?
            .mutual_guilds
            .get(cursor)?
            .guild_id;
        // Bail out if we don't actually know the guild yet (rare; the popup
        // can list mutual_guilds whose GUILD_CREATE hasn't been delivered for
        // this session).
        self.discord.guild(guild_id)?;
        self.activate_guild(ActiveGuildScope::Guild(guild_id));
        if let Some(index) = self.guild_pane_entries().iter().position(
            |entry| matches!(entry, GuildPaneEntry::Guild { state, .. } if state.id == guild_id),
        ) {
            self.selected_guild = index;
        }
        self.close_user_profile_popup();
        None
    }

    pub fn guild_name(&self, guild_id: Id<GuildMarker>) -> Option<&str> {
        self.discord
            .guild(guild_id)
            .map(|state| state.name.as_str())
    }

    pub fn is_channel_action_threads_phase(&self) -> bool {
        matches!(
            self.channel_action_menu,
            Some(ChannelActionMenuState::Threads { .. })
        )
    }

    pub fn channel_action_menu_title(&self) -> Option<String> {
        let channel_id = match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { channel_id, .. }
            | ChannelActionMenuState::Threads { channel_id, .. } => *channel_id,
        };
        let channel = self.discord.channel(channel_id)?;
        Some(format!("#{}", channel.name))
    }

    pub fn is_emoji_reaction_picker_open(&self) -> bool {
        self.emoji_reaction_picker.is_some()
    }

    pub fn is_poll_vote_picker_open(&self) -> bool {
        self.poll_vote_picker.is_some()
    }

    pub fn poll_vote_picker_items(&self) -> Option<&[PollVotePickerItem]> {
        self.poll_vote_picker
            .as_ref()
            .map(PollVotePickerState::answers)
    }

    pub fn is_reaction_users_popup_open(&self) -> bool {
        self.reaction_users_popup.is_some()
    }

    pub fn is_debug_log_popup_open(&self) -> bool {
        self.debug_log_popup_open
    }

    pub fn toggle_debug_log_popup(&mut self) {
        self.debug_log_popup_open = !self.debug_log_popup_open;
    }

    pub fn close_debug_log_popup(&mut self) {
        self.debug_log_popup_open = false;
    }

    pub fn debug_log_lines(&self) -> Vec<String> {
        logging::error_entries()
            .into_iter()
            .map(|entry| entry.line())
            .collect()
    }

    /// Visible vs. permission-hidden channel counts for the active scope.
    /// Surfaced in the debug-log popup so the user can verify whether a
    /// missing channel is actually being filtered by `can_view_channel` or
    /// just isn't in the cache. DM scope always reports `(N, 0)`.
    pub fn debug_channel_visibility(&self) -> ChannelVisibilityStats {
        match self.active_guild {
            ActiveGuildScope::Unset => ChannelVisibilityStats::default(),
            ActiveGuildScope::DirectMessages => self.discord.channel_visibility_stats(None),
            ActiveGuildScope::Guild(guild_id) => {
                self.discord.channel_visibility_stats(Some(guild_id))
            }
        }
    }

    pub fn reaction_users_popup(&self) -> Option<&ReactionUsersPopupState> {
        self.reaction_users_popup.as_ref()
    }

    pub fn emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        let mut items = unicode_emoji_reaction_items();
        let guild_id = self.picker_guild_id();

        if let Some(guild_id) = guild_id {
            items.extend(
                self.discord
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .filter(|emoji| emoji.available)
                    .map(custom_emoji_reaction_item),
            );
        }

        items
    }

    pub fn open_selected_message_actions(&mut self) {
        if self.focus == FocusPane::Messages && self.selected_message_state().is_some() {
            self.message_action_menu = Some(MessageActionMenuState { selected: 0 });
        }
    }

    pub fn close_message_action_menu(&mut self) {
        self.message_action_menu = None;
    }

    pub fn open_selected_channel_actions(&mut self) {
        if self.focus != FocusPane::Channels {
            return;
        }
        let Some(channel_id) = self.selected_channel_cursor_id() else {
            return;
        };
        let Some(channel) = self.discord.channel(channel_id) else {
            return;
        };
        if channel.is_category() || channel.is_thread() {
            return;
        }
        self.channel_action_menu = Some(ChannelActionMenuState::Actions {
            channel_id,
            selected: 0,
        });
    }

    pub fn close_channel_action_menu(&mut self) {
        self.channel_action_menu = None;
    }

    pub fn back_channel_action_menu(&mut self) {
        if let Some(ChannelActionMenuState::Threads { channel_id, .. }) =
            self.channel_action_menu.as_ref()
        {
            let channel_id = *channel_id;
            self.channel_action_menu = Some(ChannelActionMenuState::Actions {
                channel_id,
                selected: 0,
            });
        } else {
            self.channel_action_menu = None;
        }
    }

    pub fn selected_channel_action_items(&self) -> Vec<ChannelActionItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let thread_count = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .count();
        let label = if thread_count == 0 {
            "Show threads (none)".to_owned()
        } else {
            format!("Show threads ({thread_count})")
        };
        vec![ChannelActionItem {
            kind: ChannelActionKind::ShowThreads,
            label,
            enabled: thread_count > 0,
        }]
    }

    pub fn channel_action_thread_items(&self) -> Vec<ChannelThreadItem> {
        let channel_id = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Threads { channel_id, .. }) => *channel_id,
            _ => return Vec::new(),
        };
        let mut threads: Vec<&ChannelState> = self
            .channels()
            .into_iter()
            .filter(|c| c.is_thread() && c.parent_id == Some(channel_id))
            .collect();
        sort_channels(&mut threads);
        threads
            .into_iter()
            .map(|c| ChannelThreadItem {
                channel_id: c.id,
                label: c.name.clone(),
                archived: c.thread_archived.unwrap_or(false),
                locked: c.thread_locked.unwrap_or(false),
            })
            .collect()
    }

    pub fn selected_channel_action_index(&self) -> Option<usize> {
        match self.channel_action_menu.as_ref()? {
            ChannelActionMenuState::Actions { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.selected_channel_action_items().len(),
            )),
            ChannelActionMenuState::Threads { selected, .. } => Some(clamp_selected_index(
                *selected,
                self.channel_action_thread_items().len(),
            )),
        }
    }

    pub fn move_channel_action_down(&mut self) {
        let len = match self.channel_action_menu.as_ref() {
            Some(ChannelActionMenuState::Actions { .. }) => {
                self.selected_channel_action_items().len()
            }
            Some(ChannelActionMenuState::Threads { .. }) => {
                self.channel_action_thread_items().len()
            }
            None => return,
        };
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_down(selected, len);
        }
    }

    pub fn move_channel_action_up(&mut self) {
        if let Some(menu) = self.channel_action_menu.as_mut() {
            let selected = match menu {
                ChannelActionMenuState::Actions { selected, .. }
                | ChannelActionMenuState::Threads { selected, .. } => selected,
            };
            move_index_up(selected);
        }
    }

    pub fn activate_selected_channel_action(&mut self) -> Option<AppCommand> {
        let menu = self.channel_action_menu.clone()?;
        match menu {
            ChannelActionMenuState::Actions {
                channel_id,
                selected,
            } => {
                let items = self.selected_channel_action_items();
                let item = items
                    .get(clamp_selected_index(selected, items.len()))?
                    .clone();
                if !item.enabled {
                    return None;
                }
                match item.kind {
                    ChannelActionKind::ShowThreads => {
                        self.channel_action_menu = Some(ChannelActionMenuState::Threads {
                            channel_id,
                            selected: 0,
                        });
                        None
                    }
                }
            }
            ChannelActionMenuState::Threads { .. } => {
                let items = self.channel_action_thread_items();
                let index = self.selected_channel_action_index()?;
                let item = items.get(index)?.clone();
                let guild_id = self
                    .discord
                    .channel(item.channel_id)
                    .and_then(|c| c.guild_id);
                self.activate_channel(item.channel_id);
                self.close_channel_action_menu();
                guild_id.map(|guild_id| AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id: item.channel_id,
                })
            }
        }
    }

    pub fn close_emoji_reaction_picker(&mut self) {
        self.emoji_reaction_picker = None;
    }

    pub fn close_poll_vote_picker(&mut self) {
        self.poll_vote_picker = None;
    }

    pub fn close_reaction_users_popup(&mut self) {
        self.reaction_users_popup = None;
    }

    pub fn scroll_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(1);
            popup.clamp_scroll();
        }
    }

    pub fn scroll_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(1);
        }
    }

    pub fn page_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(10);
            popup.clamp_scroll();
        }
    }

    pub fn page_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(10);
        }
    }

    pub fn set_reaction_users_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.view_height = height;
            popup.clamp_scroll();
        }
    }

    pub fn move_message_action_down(&mut self) {
        let actions_len = self.selected_message_action_items().len();
        if let Some(menu) = &mut self.message_action_menu {
            move_index_down(&mut menu.selected, actions_len);
        }
    }

    pub fn move_message_action_up(&mut self) {
        if let Some(menu) = &mut self.message_action_menu {
            move_index_up(&mut menu.selected);
        }
    }

    pub fn move_emoji_reaction_down(&mut self) {
        let reactions_len = self.emoji_reaction_items().len();
        if let Some(picker) = &mut self.emoji_reaction_picker {
            move_index_down(&mut picker.selected, reactions_len);
        }
    }

    pub fn move_emoji_reaction_up(&mut self) {
        if let Some(picker) = &mut self.emoji_reaction_picker {
            move_index_up(&mut picker.selected);
        }
    }

    pub fn move_poll_vote_picker_down(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            move_index_down(&mut picker.selected, picker.answers.len());
        }
    }

    pub fn move_poll_vote_picker_up(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            move_index_up(&mut picker.selected);
        }
    }

    pub fn toggle_selected_poll_vote_answer(&mut self) {
        if let Some(picker) = &mut self.poll_vote_picker {
            let index = clamp_selected_index(picker.selected, picker.answers.len());
            if let Some(answer) = picker.answers.get_mut(index) {
                answer.selected = !answer.selected;
            }
        }
    }

    pub fn selected_poll_vote_picker_index(&self) -> Option<usize> {
        self.poll_vote_picker
            .as_ref()
            .map(|picker| clamp_selected_index(picker.selected, picker.answers.len()))
    }

    pub fn selected_message_action_items(&self) -> Vec<MessageActionItem> {
        let Some(message) = self.selected_message_state() else {
            return Vec::new();
        };
        let mut actions = vec![MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "Reply".to_owned(),
            enabled: true,
        }];

        let capabilities = message.capabilities();
        if self.thread_summary_for_message(message).is_some() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::OpenThread,
                label: "Open thread".to_owned(),
                enabled: true,
            });
        }
        if capabilities.has_image {
            actions.push(MessageActionItem {
                kind: MessageActionKind::DownloadImage,
                label: "Download image".to_owned(),
                enabled: true,
            });
        }
        actions.push(MessageActionItem {
            kind: MessageActionKind::AddReaction,
            label: "Add reaction".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::LoadPinnedMessages,
            label: "Show pinned messages".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::SetPinned(!message.pinned),
            label: if message.pinned {
                "Unpin message".to_owned()
            } else {
                "Pin message".to_owned()
            },
            enabled: true,
        });
        if !message.reactions.is_empty() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::ShowReactionUsers,
                label: "Show reacted users".to_owned(),
                enabled: true,
            });
        }
        for (index, reaction) in message.reactions.iter().enumerate() {
            if reaction.me {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::RemoveReaction(index),
                    label: format!("Remove {} reaction", reaction.emoji.status_label()),
                    enabled: true,
                });
            }
        }
        if let Some(poll) = &message.poll
            && !poll.results_finalized.unwrap_or(false)
        {
            if poll.allow_multiselect {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::OpenPollVotePicker,
                    label: "Choose poll votes".to_owned(),
                    enabled: true,
                });
            } else {
                for answer in &poll.answers {
                    actions.push(MessageActionItem {
                        kind: MessageActionKind::VotePollAnswer(answer.answer_id),
                        label: if answer.me_voted {
                            format!("Remove poll vote: {}", answer.text)
                        } else {
                            format!("Vote poll: {}", answer.text)
                        },
                        enabled: true,
                    });
                }
            }
        }
        actions
    }

    pub fn selected_message_action_index(&self) -> Option<usize> {
        self.message_action_menu.as_ref().map(|menu| {
            clamp_selected_index(menu.selected, self.selected_message_action_items().len())
        })
    }

    pub fn selected_message_action(&self) -> Option<MessageActionItem> {
        let index = self.selected_message_action_index()?;
        self.selected_message_action_items().get(index).cloned()
    }

    pub fn selected_emoji_reaction_index(&self) -> Option<usize> {
        self.emoji_reaction_picker
            .as_ref()
            .map(|picker| clamp_selected_index(picker.selected, self.emoji_reaction_items().len()))
    }

    pub fn selected_emoji_reaction(&self) -> Option<EmojiReactionItem> {
        let index = self.selected_emoji_reaction_index()?;
        self.emoji_reaction_items().get(index).cloned()
    }

    pub fn activate_selected_message_action(&mut self) -> Option<AppCommand> {
        let action = self.selected_message_action()?;
        if !action.enabled {
            return None;
        }

        match action.kind {
            MessageActionKind::Reply => {
                self.start_reply_composer();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::OpenThread => {
                let channel_id = self
                    .selected_message_state()
                    .and_then(|message| self.thread_summary_for_message(message))?
                    .channel_id;
                self.activate_channel(channel_id);
                self.close_message_action_menu();
                None
            }
            MessageActionKind::DownloadImage => {
                let preview = self.selected_message_state()?.first_inline_preview()?;
                let url = preview.url.to_owned();
                let filename = preview.filename.to_owned();
                self.close_message_action_menu();
                Some(AppCommand::DownloadAttachment { url, filename })
            }
            MessageActionKind::AddReaction => {
                self.open_emoji_reaction_picker();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::RemoveReaction(index) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reaction = message.reactions.get(index)?.clone();
                self.close_message_action_menu();
                Some(AppCommand::RemoveReaction {
                    channel_id,
                    message_id,
                    emoji: reaction.emoji,
                })
            }
            MessageActionKind::ShowProfile => {
                let message = self.selected_message_state()?;
                let user_id = message.author_id;
                let guild_id = message.guild_id;
                self.close_message_action_menu();
                self.open_user_profile_popup(user_id, guild_id)
            }
            MessageActionKind::ShowReactionUsers => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reactions = message
                    .reactions
                    .iter()
                    .map(|reaction| reaction.emoji.clone())
                    .collect::<Vec<_>>();
                if reactions.is_empty() {
                    self.close_message_action_menu();
                    return None;
                }
                self.close_message_action_menu();
                Some(AppCommand::LoadReactionUsers {
                    channel_id,
                    message_id,
                    reactions,
                })
            }
            MessageActionKind::LoadPinnedMessages => {
                let channel_id = self.selected_message_state()?.channel_id;
                self.close_message_action_menu();
                Some(AppCommand::LoadPinnedMessages { channel_id })
            }
            MessageActionKind::SetPinned(pinned) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                self.close_message_action_menu();
                Some(AppCommand::SetMessagePinned {
                    channel_id,
                    message_id,
                    pinned,
                })
            }
            MessageActionKind::OpenPollVotePicker => {
                self.open_poll_vote_picker();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::VotePollAnswer(answer_id) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let poll = message.poll.as_ref()?;
                let mut answer_ids = if poll.allow_multiselect {
                    poll.answers
                        .iter()
                        .filter(|answer| answer.me_voted && answer.answer_id != answer_id)
                        .map(|answer| answer.answer_id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if !poll
                    .answers
                    .iter()
                    .any(|answer| answer.answer_id == answer_id && answer.me_voted)
                {
                    answer_ids.push(answer_id);
                }
                self.close_message_action_menu();
                Some(AppCommand::VotePoll {
                    channel_id,
                    message_id,
                    answer_ids,
                })
            }
        }
    }

    pub fn activate_selected_emoji_reaction(&mut self) -> Option<AppCommand> {
        let picker = self.emoji_reaction_picker.clone()?;
        let reaction = self.selected_emoji_reaction()?;
        let command = AppCommand::AddReaction {
            channel_id: picker.channel_id,
            message_id: picker.message_id,
            emoji: reaction.emoji,
        };
        self.close_emoji_reaction_picker();
        Some(command)
    }

    pub fn activate_poll_vote_picker(&mut self) -> Option<AppCommand> {
        let picker = self.poll_vote_picker.clone()?;
        let answer_ids = picker
            .answers
            .iter()
            .filter(|answer| answer.selected)
            .map(|answer| answer.answer_id)
            .collect::<Vec<_>>();
        self.close_poll_vote_picker();
        Some(AppCommand::VotePoll {
            channel_id: picker.channel_id,
            message_id: picker.message_id,
            answer_ids,
        })
    }

    fn open_emoji_reaction_picker(&mut self) {
        if let Some(message) = self.selected_message_state() {
            self.emoji_reaction_picker = Some(EmojiReactionPickerState {
                selected: 0,
                guild_id: message
                    .guild_id
                    .or_else(|| self.selected_channel_guild_id()),
                channel_id: message.channel_id,
                message_id: message.id,
            });
        }
    }

    fn open_poll_vote_picker(&mut self) {
        if let Some(message) = self.selected_message_state()
            && let Some(poll) = &message.poll
        {
            self.poll_vote_picker = Some(PollVotePickerState {
                selected: 0,
                channel_id: message.channel_id,
                message_id: message.id,
                answers: poll
                    .answers
                    .iter()
                    .map(|answer| PollVotePickerItem {
                        answer_id: answer.answer_id,
                        label: answer.text.clone(),
                        selected: answer.me_voted,
                    })
                    .collect(),
            });
        }
    }

    fn picker_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.emoji_reaction_picker
            .as_ref()
            .and_then(|picker| picker.guild_id)
            .or_else(|| {
                self.selected_message_state()
                    .and_then(|message| message.guild_id)
            })
            .or_else(|| self.selected_channel_guild_id())
    }

    fn selected_channel_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
    }

    fn start_reply_composer(&mut self) {
        let Some(message_id) = self.selected_message_state().map(|message| message.id) else {
            return;
        };
        // Same gating as `start_composer` — replies are sends, so the channel
        // must allow SEND_MESSAGES for the action to be useful.
        if !self.can_send_in_selected_channel() {
            return;
        }
        self.composer_input.clear();
        self.reply_target_message_id = Some(message_id);
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
    }

    /// Builds the guild pane in display order: a virtual "Direct Messages"
    /// row, then each `guild_folders` entry expanded into either a single
    /// guild row (`id == None`, one member) or a folder header followed by
    /// indented children. Collapsed folders hide their children. Guilds that
    /// the user is in but the folder list omits get appended at the bottom.
    pub fn guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        let mut entries: Vec<GuildPaneEntry<'_>> = vec![GuildPaneEntry::DirectMessages];
        let by_id: HashMap<Id<GuildMarker>, &GuildState> = self
            .discord
            .guilds()
            .into_iter()
            .map(|guild| (guild.id, guild))
            .collect();
        let mut placed: HashSet<Id<GuildMarker>> = HashSet::new();
        let folders = self.discord.guild_folders();

        if folders.is_empty() {
            // Iterating `by_id.values()` here is non-deterministic because
            // it's a HashMap, which makes the sidebar shuffle on every render.
            // Fall back to the discord state's own (insertion-ordered) guild
            // list so the order stays stable until folder data arrives.
            for guild in self.discord.guilds() {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
            return entries;
        }

        for folder in folders {
            let is_single_container = folder.id.is_none() && folder.guild_ids.len() == 1;
            if is_single_container {
                if let Some(guild) = by_id.get(&folder.guild_ids[0]) {
                    entries.push(GuildPaneEntry::Guild {
                        state: guild,
                        branch: GuildBranch::None,
                    });
                    placed.insert(folder.guild_ids[0]);
                }
                continue;
            }

            let folder_key = Self::folder_key(folder);
            let collapsed = folder_key
                .as_ref()
                .is_some_and(|key| self.collapsed_folders.contains(key));
            entries.push(GuildPaneEntry::FolderHeader { folder, collapsed });

            // Always mark children as placed even when collapsed so we don't
            // duplicate them in the trailing "ungrouped" loop.
            for guild_id in &folder.guild_ids {
                placed.insert(*guild_id);
            }

            if collapsed {
                continue;
            }

            let child_guilds: Vec<&GuildState> = folder
                .guild_ids
                .iter()
                .filter_map(|guild_id| by_id.get(guild_id).copied())
                .collect();
            let last_child_index = child_guilds.len().saturating_sub(1);
            for (index, guild) in child_guilds.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    GuildBranch::Last
                } else {
                    GuildBranch::Middle
                };
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch,
                });
            }
        }

        // Same reasoning as the folder-empty branch above: walk the discord
        // state's BTreeMap-backed list so the trailing "ungrouped" guilds
        // appear in a stable, deterministic order.
        for guild in self.discord.guilds() {
            if !placed.contains(&guild.id) {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
        }

        entries
    }

    pub fn selected_guild(&self) -> usize {
        clamp_selected_index(self.selected_guild, self.guild_pane_entries().len())
    }

    pub fn guild_scroll(&self) -> usize {
        self.guild_scroll
    }

    pub fn visible_guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        self.guild_pane_entries()
            .into_iter()
            .skip(self.guild_scroll)
            .take(pane_content_height(self.guild_view_height))
            .collect()
    }

    pub fn focused_guild_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Guilds && !self.guild_pane_entries().is_empty() {
            Some(self.selected_guild().saturating_sub(self.guild_scroll))
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        self.guild_view_height = height;
        self.clamp_guild_viewport();
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => None,
        }
    }

    pub fn is_active_guild_entry(&self, entry: &GuildPaneEntry<'_>) -> bool {
        match (self.active_guild, entry) {
            (ActiveGuildScope::DirectMessages, GuildPaneEntry::DirectMessages) => true,
            (ActiveGuildScope::Guild(active_id), GuildPaneEntry::Guild { state, .. }) => {
                state.id == active_id
            }
            (ActiveGuildScope::Unset, _)
            | (ActiveGuildScope::DirectMessages, _)
            | (ActiveGuildScope::Guild(_), _) => false,
        }
    }

    /// Toggles the collapse state of the folder under the selection. Does
    /// nothing if the cursor isn't on a folder header.
    pub fn toggle_selected_folder(&mut self) {
        let folder_key = self.selected_folder_key();
        if let Some(key) = folder_key {
            toggle_collapsed_key(&mut self.collapsed_folders, key);
        }
    }

    pub fn open_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            open_collapsed_key(&mut self.collapsed_folders, &key);
        }
    }

    pub fn close_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            close_collapsed_key(&mut self.collapsed_folders, key);
        }
    }

    pub fn confirm_selected_guild(&mut self) {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::DirectMessages) => {
                self.activate_guild(ActiveGuildScope::DirectMessages)
            }
            Some(GuildPaneEntry::Guild { state, .. }) => {
                self.activate_guild(ActiveGuildScope::Guild(state.id))
            }
            Some(GuildPaneEntry::FolderHeader { .. }) => self.toggle_selected_folder(),
            None => {}
        }
    }

    fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.active_guild = scope;
        self.selected_channel = 0;
        self.channel_scroll = 0;
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.selected_member = 0;
    }

    fn selected_folder_key(&self) -> Option<FolderKey> {
        let entries = self.guild_pane_entries();
        let selected = self.selected_guild();
        match entries.get(selected) {
            Some(GuildPaneEntry::FolderHeader { folder, .. }) => Self::folder_key(folder),
            Some(GuildPaneEntry::Guild { branch, .. }) if branch.is_folder_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    GuildPaneEntry::FolderHeader { folder, .. } => Self::folder_key(folder),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn folder_key(folder: &GuildFolder) -> Option<FolderKey> {
        if let Some(id) = folder.id {
            Some(FolderKey::Id(id))
        } else if folder.guild_ids.len() > 1 {
            Some(FolderKey::Guilds(folder.guild_ids.clone()))
        } else {
            None
        }
    }

    /// Returns the active guild plus the channel concord should attach the
    /// op-37 member-list subscription to. Prefers the user's currently open
    /// channel and falls back to the first text channel in the guild so the
    /// sidebar still updates while no channel is selected.
    pub fn member_list_subscription_target(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => guild_id,
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => return None,
        };
        let channel_id = self
            .active_channel_id
            .filter(|channel_id| {
                self.discord
                    .channel(*channel_id)
                    .is_some_and(|channel| self.is_member_list_subscription_channel(channel))
            })
            .or_else(|| self.guild_member_list_channel(guild_id))?;
        Some((guild_id, channel_id))
    }

    /// Highest 100-member bucket the user has scrolled the member sidebar
    /// into. Bucket 0 covers indexes 0..=99, bucket 1 covers 100..=199, etc.
    pub fn member_subscription_top_bucket(&self) -> u32 {
        let scroll = u32::try_from(self.member_scroll).unwrap_or(u32::MAX);
        let view = u32::try_from(self.member_view_height).unwrap_or(0);
        scroll.saturating_add(view) / 100
    }

    /// op-37 channel ranges that cover the member viewport plus a small
    /// trailing window. We anchor `[0, 99]` so the top of the sidebar always
    /// stays populated, then add up to two more buckets near the visible end
    /// so presence events keep flowing as the user scrolls. Capped at four
    /// ranges total because Discord rejects oversized channel range lists.
    pub fn member_subscription_ranges(&self) -> Vec<(u32, u32)> {
        let top = self.member_subscription_top_bucket();
        if top <= 2 {
            return (0..=top).map(|b| (b * 100, b * 100 + 99)).collect();
        }
        let near_start = top.saturating_sub(1);
        vec![
            (0, 99),
            (near_start * 100, near_start * 100 + 99),
            (top * 100, top * 100 + 99),
        ]
    }

    /// Picks a channel suitable for sending a guild op-37 subscription so
    /// Discord starts shipping `GUILD_MEMBER_LIST_UPDATE` events. Member-list
    /// updates only flow once the client subscribes to *some* channel in the
    /// guild; this lets the sidebar populate before the user opens a channel.
    pub fn guild_member_list_channel(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Option<Id<ChannelMarker>> {
        let mut candidates: Vec<&ChannelState> = self
            .discord
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(|channel| self.is_member_list_subscription_channel(channel))
            .collect();
        sort_channels(&mut candidates);
        candidates.first().map(|channel| channel.id)
    }

    fn is_member_list_subscription_channel(&self, channel: &ChannelState) -> bool {
        !channel.is_category()
            && !channel.is_thread()
            && !matches!(channel.kind.as_str(), "voice" | "GuildVoice")
            && self.discord.can_view_channel(channel)
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            // DMs do not carry guild-style permissions; show every channel.
            ActiveGuildScope::DirectMessages => self.discord.channels_for_guild(None),
            // Filter to channels we have VIEW_CHANNEL on, otherwise the
            // sidebar surfaces channels that REST refuses with 403.
            ActiveGuildScope::Guild(guild_id) => {
                self.discord.viewable_channels_for_guild(Some(guild_id))
            }
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        // Threads are reached through the channel action menu instead of
        // appearing as top-level entries; without this filter their parent
        // channel would not be in `category_ids`, so the roots filter below
        // would let them through and render them under the channel list.
        channels.retain(|channel| !channel.is_thread());
        if self.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels
                .into_iter()
                .map(|state| ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                })
                .collect();
        }

        let category_ids: HashSet<Id<ChannelMarker>> = channels
            .iter()
            .filter(|channel| channel.is_category())
            .map(|channel| channel.id)
            .collect();

        let mut roots: Vec<&ChannelState> = channels
            .iter()
            .copied()
            .filter(|channel| {
                channel.is_category()
                    || channel
                        .parent_id
                        .is_none_or(|parent_id| !category_ids.contains(&parent_id))
            })
            .collect();
        sort_channels(&mut roots);

        let mut entries = Vec::new();
        for root in roots {
            if !root.is_category() {
                entries.push(ChannelPaneEntry::Channel {
                    state: root,
                    branch: ChannelBranch::None,
                });
                continue;
            }

            let collapsed = self.collapsed_channel_categories.contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });
            if collapsed {
                continue;
            }

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            let last_child_index = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    ChannelBranch::Last
                } else {
                    ChannelBranch::Middle
                };
                entries.push(ChannelPaneEntry::Channel {
                    state: child,
                    branch,
                });
            }
        }

        entries
    }

    pub fn selected_channel(&self) -> usize {
        clamp_selected_index(self.selected_channel, self.channel_pane_entries().len())
    }

    fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::CategoryHeader { .. }) | None => None,
        }
    }

    pub fn channel_scroll(&self) -> usize {
        self.channel_scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_entries()
            .into_iter()
            .skip(self.channel_scroll)
            .take(pane_content_height(self.channel_view_height))
            .collect()
    }

    pub fn focused_channel_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Channels && !self.channel_pane_entries().is_empty() {
            Some(self.selected_channel().saturating_sub(self.channel_scroll))
        } else {
            None
        }
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        self.channel_view_height = height;
        self.clamp_channel_viewport();
    }

    fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self.channel_pane_entries().iter().position(|entry| {
            matches!(entry, ChannelPaneEntry::Channel { state, .. } if state.id == channel_id)
        }) {
            self.selected_channel = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
    }

    /// Builds the "X is typing…" line for the currently selected channel, or
    /// `None` when nobody is typing (or the only typer is us). Resolution
    /// order for each user: cached guild member alias → DM recipient
    /// display name → `user-{id}` fallback. Caps at three names and
    /// collapses to "Several people are typing…" beyond that.
    pub fn typing_footer_for_selected_channel(&self) -> Option<String> {
        let channel_id = self.selected_channel_id()?;
        let channel = self.discord.channel(channel_id)?;
        let guild_id = channel.guild_id;
        let typers: Vec<Id<UserMarker>> = self
            .discord
            .typing_users(channel_id)
            .into_iter()
            .filter(|user_id| Some(*user_id) != self.current_user_id)
            .collect();
        if typers.is_empty() {
            return None;
        }

        let resolve_name = |user_id: Id<UserMarker>| -> String {
            if let Some(name) =
                guild_id.and_then(|guild_id| self.discord.member_display_name(guild_id, user_id))
            {
                return name.to_owned();
            }
            if let Some(recipient) = channel
                .recipients
                .iter()
                .find(|recipient| recipient.user_id == user_id)
            {
                return recipient.display_name.clone();
            }
            format!("user-{}", user_id.get())
        };

        let total = typers.len();
        let names: Vec<String> = typers.iter().take(3).copied().map(resolve_name).collect();
        let footer = match total {
            1 => format!("{} is typing…", names[0]),
            2 => format!("{} and {} are typing…", names[0], names[1]),
            3 => format!("{}, {}, and {} are typing…", names[0], names[1], names[2]),
            _ => "Several people are typing…".to_owned(),
        };
        Some(footer)
    }

    pub fn channel_label(&self, channel_id: Id<ChannelMarker>) -> String {
        self.discord
            .channel(channel_id)
            .map(|channel| match channel.kind.as_str() {
                "dm" | "Private" => format!("@{}", channel.name),
                "group-dm" | "Group" => channel.name.clone(),
                _ => format!("#{}", channel.name),
            })
            .unwrap_or_else(|| format!("#channel-{}", channel_id.get()))
    }

    pub(crate) fn thread_summary_for_message(
        &self,
        message: &MessageState,
    ) -> Option<ThreadSummary> {
        if message.message_kind.code() != 18 {
            return None;
        }
        let referenced_thread = message
            .reference
            .as_ref()
            .and_then(|reference| reference.channel_id)
            .and_then(|channel_id| self.discord.channel(channel_id))
            .filter(|channel| channel.is_thread() && self.discord.can_view_channel(channel));
        let thread = referenced_thread.or_else(|| {
            let thread_name = message.content.as_deref()?.trim();
            if thread_name.is_empty() {
                return None;
            }
            self.discord
                .viewable_channels_for_guild(message.guild_id)
                .into_iter()
                .find(|channel| {
                    channel.is_thread()
                        && channel.parent_id == Some(message.channel_id)
                        && channel.name == thread_name
                })
        });
        thread.map(|channel| ThreadSummary {
            channel_id: channel.id,
            name: channel.name.clone(),
            message_count: channel.message_count,
            total_message_sent: channel.total_message_sent,
            archived: channel.thread_archived,
            locked: channel.thread_locked,
        })
    }

    pub(crate) fn render_user_mentions(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> String {
        render_user_mentions(value, |user_id| {
            self.resolve_mention_display_name(guild_id, mentions, user_id)
        })
    }

    pub(crate) fn render_user_mentions_with_highlights(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> RenderedText {
        let current_user_id = self.current_user_id.map(|id| id.get());
        let mut rendered = render_user_mentions_with_highlights(
            value,
            |user_id| self.resolve_mention_display_name(guild_id, mentions, user_id),
            |user_id| {
                if current_user_id == Some(user_id) {
                    Some(TextHighlightKind::SelfMention)
                } else {
                    Some(TextHighlightKind::OtherMention)
                }
            },
        );
        if current_user_id.is_some() {
            add_literal_mention_highlights(&mut rendered, "@everyone");
            add_literal_mention_highlights(&mut rendered, "@here");
        }
        normalize_text_highlights(&mut rendered.highlights);
        rendered
    }

    fn resolve_mention_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        user_id: u64,
    ) -> Option<String> {
        let mention = mentions
            .iter()
            .find(|mention| mention.user_id.get() == user_id);
        if let Some(guild_nick) = mention.and_then(|mention| mention.guild_nick.as_deref()) {
            return Some(guild_nick.to_owned());
        }
        if let Some(display_name) = guild_id.and_then(|guild_id| {
            let user_id = Id::<UserMarker>::new(user_id);
            self.discord.member_display_name(guild_id, user_id)
        }) {
            return Some(display_name.to_owned());
        }
        mention.map(|mention| mention.display_name.clone())
    }

    pub(crate) fn forwarded_snapshot_mention_guild_id(
        &self,
        snapshot: &MessageSnapshotInfo,
    ) -> Option<Id<GuildMarker>> {
        snapshot
            .source_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
            .and_then(|channel| channel.guild_id)
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } if Some(state.id) == self.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        toggle_collapsed_key(&mut self.collapsed_channel_categories, category_id);
    }

    pub fn open_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            open_collapsed_key(&mut self.collapsed_channel_categories, &category_id);
        }
    }

    pub fn close_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            close_collapsed_key(&mut self.collapsed_channel_categories, category_id);
        }
    }

    #[cfg(test)]
    pub fn confirm_selected_channel(&mut self) {
        let _ = self.confirm_selected_channel_command();
    }

    pub fn confirm_selected_channel_command(&mut self) -> Option<AppCommand> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { .. }) => {
                self.toggle_selected_channel_category();
                None
            }
            Some(ChannelPaneEntry::Channel { state, .. }) => {
                let channel_id = state.id;
                let command = if is_direct_message_channel(state) {
                    Some(AppCommand::SubscribeDirectMessage { channel_id })
                } else {
                    state
                        .guild_id
                        .map(|guild_id| AppCommand::SubscribeGuildChannel {
                            guild_id,
                            channel_id,
                        })
                };
                self.activate_channel(channel_id);
                command
            }
            None => None,
        }
    }

    fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.active_channel_id = Some(channel_id);
        self.message_auto_follow = true;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.selected_message = self.messages().len().saturating_sub(1);
        self.clamp_message_viewport();
    }

    fn record_older_history_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
        response_before: Option<Id<MessageMarker>>,
        messages: &[MessageInfo],
    ) {
        let Some(OlderHistoryRequestState::Requested { before }) =
            self.older_history_requests.get(&channel_id).copied()
        else {
            return;
        };
        if response_before != Some(before) {
            return;
        }

        if messages.is_empty() {
            self.older_history_requests
                .insert(channel_id, OlderHistoryRequestState::Exhausted { before });
        } else {
            self.older_history_requests.remove(&channel_id);
        }
    }

    fn selected_channel_category_id(&self) -> Option<Id<ChannelMarker>> {
        let entries = self.channel_pane_entries();
        let selected = self.selected_channel();
        match entries.get(selected) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { branch, .. }) if branch.is_category_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
                    _ => None,
                }),
            _ => None,
        }
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        self.selected_channel_id()
            .map(|channel_id| self.discord.messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    pub fn selected_message(&self) -> usize {
        clamp_selected_index(self.selected_message, self.messages().len())
    }

    pub fn selected_message_state(&self) -> Option<&MessageState> {
        let channel_id = self.selected_channel_id()?;
        self.discord
            .messages_for_channel(channel_id)
            .get(self.selected_message())
            .copied()
    }

    pub(crate) fn reply_target_message_state(&self) -> Option<&MessageState> {
        let message_id = self.reply_target_message_id?;
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
    }

    pub fn next_older_history_command(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let before = self.older_history_cursor()?;
        match self.older_history_requests.get(&channel_id) {
            Some(OlderHistoryRequestState::Requested { .. }) => return None,
            Some(OlderHistoryRequestState::Exhausted { before: exhausted })
                if *exhausted == before =>
            {
                return None;
            }
            _ => {}
        }

        self.older_history_requests
            .insert(channel_id, OlderHistoryRequestState::Requested { before });
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(before),
        })
    }

    fn older_history_cursor(&self) -> Option<Id<MessageMarker>> {
        if self.focus != FocusPane::Messages
            || self.messages().is_empty()
            || self.selected_message() != 0
        {
            return None;
        }

        self.messages().first().map(|message| message.id)
    }

    pub(crate) fn message_scroll(&self) -> usize {
        self.message_scroll
    }

    pub(crate) fn message_scroll_row_position(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        (0..self.message_scroll)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .saturating_add(self.message_line_scroll)
    }

    pub(crate) fn message_total_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        (0..self.messages().len())
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    /// Returns true when the message at `index` (within `self.messages()`)
    /// should be preceded by a date separator because its local date differs
    /// from the previous message's. The first message in the loaded history
    /// receives no separator on its own — the renderer waits for an actual
    /// day boundary between two visible messages.
    pub(crate) fn message_starts_new_day_at(&self, index: usize) -> bool {
        let messages = self.messages();
        let Some(current) = messages.get(index) else {
            return false;
        };
        let previous_id = index
            .checked_sub(1)
            .and_then(|prev_index| messages.get(prev_index).map(|message| message.id));
        super::ui::message_starts_new_day(current.id, previous_id)
    }

    /// Number of extra rows that the message at `index` reserves above its
    /// avatar/header line. Today this is 1 when a date separator should
    /// appear, 0 otherwise.
    pub(crate) fn message_extra_top_lines(&self, index: usize) -> usize {
        usize::from(self.message_starts_new_day_at(index))
    }

    #[cfg(test)]
    pub fn message_auto_follow(&self) -> bool {
        self.message_auto_follow
    }

    #[cfg(test)]
    pub fn message_view_height(&self) -> usize {
        self.message_view_height
    }

    pub fn visible_messages(&self) -> Vec<&MessageState> {
        self.messages()
            .into_iter()
            .skip(self.message_scroll)
            .take(self.message_content_height())
            .collect()
    }

    pub fn message_line_scroll(&self) -> usize {
        self.message_line_scroll
    }

    pub fn set_message_view_height(&mut self, height: usize) {
        self.message_view_height = height;
        self.clamp_message_viewport();
    }

    pub fn clamp_message_viewport_for_image_previews(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        self.message_content_width = content_width;
        self.message_preview_width = preview_width;
        self.message_max_preview_height = max_preview_height;
        self.clamp_message_viewport();
        if self.message_auto_follow {
            if self.message_view_height <= 1 {
                self.message_scroll = self.selected_message();
                self.message_line_scroll = 0;
            } else {
                self.align_message_viewport_to_bottom(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
            }
            return;
        }
        self.normalize_message_line_scroll(content_width, preview_width, max_preview_height);
        if self.messages().is_empty() || !self.message_keep_selection_visible {
            return;
        }

        if self.center_selected_message(content_width, preview_width, max_preview_height) {
            return;
        }

        let height = self.message_content_height();
        let upper_scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);
        let max_iterations = self
            .messages()
            .into_iter()
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .max(1);

        for _ in 0..max_iterations {
            let lower_scrolloff = self
                .following_message_rendered_rows(
                    content_width,
                    preview_width,
                    max_preview_height,
                    SCROLL_OFF,
                )
                .min(height.saturating_sub(1));
            let lower_bound = height.saturating_sub(1).saturating_sub(lower_scrolloff);
            let selected_row = self.selected_message_rendered_row(
                content_width,
                preview_width,
                max_preview_height,
            );
            let selected_bottom = selected_row.saturating_add(
                self.selected_message_rendered_height(
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .saturating_sub(1),
            );
            if selected_bottom > lower_bound && self.message_scroll < self.selected_message {
                self.scroll_message_viewport_down_one_row(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                continue;
            }

            if selected_row < upper_scrolloff && self.message_scroll > 0 {
                let previous_height = self.message_rendered_height_at(
                    self.message_scroll.saturating_sub(1),
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                let candidate_bottom = selected_bottom.saturating_add(previous_height);
                if candidate_bottom < height {
                    self.scroll_message_viewport_up_one_row(
                        content_width,
                        preview_width,
                        max_preview_height,
                    );
                    continue;
                }
            }

            break;
        }
    }

    pub fn focused_message_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Messages && !self.messages().is_empty() {
            let selected = self.selected_message();
            let visible_count = self.visible_messages().len();
            if selected >= self.message_scroll && selected < self.message_scroll + visible_count {
                Some(selected - self.message_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let members = self.discord.members_for_guild(guild_id);
        let roles = self.discord.roles_for_guild(guild_id);
        guild_member_groups(members, roles)
    }

    pub fn member_panel_title(&self) -> String {
        let Some(guild_id) = self.selected_guild_id() else {
            return "Members".to_owned();
        };
        let Some(member_count) = self
            .discord
            .guild(guild_id)
            .and_then(|guild| guild.member_count)
        else {
            return "Members".to_owned();
        };

        let loaded = self.discord.members_for_guild(guild_id).len();
        format!("Members {loaded}/{member_count} loaded")
    }

    fn selected_channel_recipient_group(&self) -> Vec<MemberGroup<'_>> {
        let Some(channel) = self.selected_channel_state() else {
            return Vec::new();
        };
        channel_recipient_group(channel)
    }

    pub fn flattened_members(&self) -> Vec<MemberEntry<'_>> {
        flatten_member_groups(self.members_grouped())
    }

    pub fn selected_member(&self) -> usize {
        clamp_selected_index(self.selected_member, self.flattened_members().len())
    }

    pub fn focused_member_selection_line(&self) -> Option<usize> {
        if self.focus == FocusPane::Members && !self.flattened_members().is_empty() {
            Some(
                self.selected_member_line()
                    .saturating_sub(self.member_scroll),
            )
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.member_scroll
    }

    pub fn member_content_height(&self) -> usize {
        pane_content_height(self.member_view_height)
    }

    pub fn member_line_count(&self) -> usize {
        self.count_member_lines()
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        self.member_view_height = height;
        self.clamp_member_viewport();
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let len = self.guild_pane_entries().len();
                move_index_down(&mut self.selected_guild, len);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let len = self.channel_pane_entries().len();
                move_index_down(&mut self.selected_channel, len);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let len = self.messages().len();
                move_index_down(&mut self.selected_message, len);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let len = self.flattened_members().len();
                move_index_down(&mut self.selected_member, len);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                move_index_up(&mut self.selected_guild);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                move_index_up(&mut self.selected_channel);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                move_index_up(&mut self.selected_message);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                move_index_up(&mut self.selected_member);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = 0;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = 0;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                self.selected_message = 0;
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = 0;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = last_index(self.guild_pane_entries().len());
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = last_index(self.channel_pane_entries().len());
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = last_index(self.messages().len());
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = last_index(self.flattened_members().len());
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                let len = self.guild_pane_entries().len();
                move_index_down_by(&mut self.selected_guild, len, distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                let len = self.channel_pane_entries().len();
                move_index_down_by(&mut self.selected_channel, len, distance.max(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                let len = self.messages().len();
                move_index_down_by(&mut self.selected_message, len, distance.max(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                move_index_up_by(&mut self.selected_guild, distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                move_index_up_by(&mut self.selected_channel, distance.max(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                let distance = self.message_content_height() / 2;
                self.selected_message = self.selected_message.saturating_sub(distance.max(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn toggle_message_auto_follow(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }

        self.message_auto_follow = !self.message_auto_follow;
        if self.message_auto_follow {
            self.message_keep_selection_visible = true;
            self.follow_latest_message();
        }
        self.clamp_message_viewport();
    }

    pub fn scroll_message_viewport_down(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_down_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn scroll_message_viewport_up(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_up_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn scroll_message_viewport_top(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    pub fn scroll_message_viewport_bottom(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.align_message_viewport_to_bottom(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Members,
            FocusPane::Members => FocusPane::Guilds,
        };
    }

    pub fn focus_pane(&mut self, pane: FocusPane) {
        self.focus = pane;
    }

    /// Whether the user can post messages in the currently selected channel.
    /// Returns `true` when no channel is selected so callers don't have to
    /// special-case the empty state.
    pub fn can_send_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) => self.discord.can_send_in_channel(channel),
            None => true,
        }
    }

    /// Whether the user can attach files in the currently selected channel.
    /// Wired up so a future attachment picker can disable itself; the
    /// composer doesn't expose attachment input today.
    pub fn can_attach_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) => self.discord.can_attach_in_channel(channel),
            None => true,
        }
    }

    pub fn start_composer(&mut self) {
        if self.selected_channel_id().is_none() {
            return;
        }
        // Refusing here keeps the keymap simple: the same key that opens the
        // composer in writable channels just no-ops in read-only ones, so the
        // user never lands in a typing state for a channel that would 403 on
        // submit.
        if !self.can_send_in_selected_channel() {
            return;
        }
        self.reply_target_message_id = None;
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn cancel_composer(&mut self) {
        self.composer_active = false;
        self.composer_input.clear();
        self.reply_target_message_id = None;
        self.reset_mention_picker_state();
    }

    pub fn push_composer_char(&mut self, value: char) {
        // The `@` key triggers the picker only at the start of a word so that
        // typing inside an email or another @mention doesn't reopen the popup
        // unexpectedly.
        if value == '@' {
            let triggers_picker = should_start_mention_query(&self.composer_input);
            self.composer_input.push('@');
            if triggers_picker {
                self.composer_mention_query = Some(String::new());
            } else {
                self.composer_mention_query = None;
            }
            self.composer_mention_selected = 0;
            return;
        }

        if let Some(query) = self.composer_mention_query.as_mut() {
            // Discord-style mention queries accept letters, digits, and the
            // characters that show up in usernames or display names. Any other
            // character commits the user to a literal `@text` and closes the
            // picker.
            if is_mention_query_char(value) {
                query.push(value);
                self.composer_input.push(value);
                self.composer_mention_selected = 0;
                return;
            }
            self.composer_mention_query = None;
            self.composer_mention_selected = 0;
        }
        self.composer_input.push(value);
    }

    pub fn pop_composer_char(&mut self) {
        if let Some(query) = self.composer_mention_query.as_mut() {
            if query.pop().is_some() {
                self.composer_input.pop();
                self.composer_mention_selected = 0;
                return;
            }
            // Query was empty so the popped character is the `@` that opened
            // the picker. Drop it and close.
            self.composer_input.pop();
            self.composer_mention_query = None;
            self.composer_mention_selected = 0;
            return;
        }
        self.composer_input.pop();
        self.invalidate_dropped_mention_completions();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let expanded =
            expand_mention_completions(&self.composer_input, &self.composer_mention_completions);
        let content = expanded.trim().to_owned();
        if content.is_empty() {
            return None;
        }
        // Defense in depth: the channel could have lost SEND_MESSAGES while
        // the composer was open (role change, channel overwrite update). Drop
        // the message rather than fire a request that would 403.
        if !self.can_send_in_selected_channel() {
            self.composer_input.clear();
            self.composer_active = false;
            self.reply_target_message_id = None;
            self.reset_mention_picker_state();
            return None;
        }

        self.composer_input.clear();
        self.composer_active = false;
        self.reset_mention_picker_state();
        let reply_to = self.reply_target_message_id.take();
        Some(AppCommand::SendMessage {
            channel_id,
            content,
            reply_to,
        })
    }

    /// Returns the characters typed after the `@` if the picker is open.
    pub fn composer_mention_query(&self) -> Option<&str> {
        self.composer_mention_query.as_deref()
    }

    pub fn composer_mention_selected(&self) -> usize {
        self.composer_mention_selected
    }

    /// Builds the visible list of suggestions for the picker. Returns at most
    /// `MAX_MENTION_PICKER_VISIBLE` entries, ordered by best match across the
    /// member's display name AND username: prefix matches beat substring
    /// matches, alias matches beat username matches at the same rank, and
    /// ties are broken alphabetically by display name.
    pub fn composer_mention_candidates(&self) -> Vec<MentionPickerEntry> {
        let Some(query) = self.composer_mention_query.as_deref() else {
            return Vec::new();
        };
        build_mention_candidates(query, self.flattened_members())
    }

    pub fn move_composer_mention_selection(&mut self, delta: isize) {
        if self.composer_mention_query.is_none() {
            return;
        }
        let len = self.composer_mention_candidates().len();
        self.composer_mention_selected =
            move_mention_selection(self.composer_mention_selected, len, delta);
    }

    /// Confirms the currently highlighted mention. Replaces the trailing
    /// `@query` with `@displayname ` (so the user sees what they wrote) and
    /// records the byte range so `submit_composer` can rewrite it to
    /// `<@USER_ID>` later. Returns `false` when the picker has no candidate
    /// to apply.
    pub fn confirm_composer_mention(&mut self) -> bool {
        let Some(query) = self.composer_mention_query.clone() else {
            return false;
        };
        let candidates = self.composer_mention_candidates();
        let Some(entry) = candidates.get(self.composer_mention_selected) else {
            return false;
        };
        let entry = entry.clone();

        // Drop the trailing `@<query>` exactly: `@` is one ASCII byte and the
        // query was built from user characters that may be multi-byte.
        let suffix_byte_count = '@'.len_utf8() + query.len();
        let new_len = self.composer_input.len().saturating_sub(suffix_byte_count);
        self.composer_input.truncate(new_len);

        let start = self.composer_input.len();
        self.composer_input.push('@');
        self.composer_input.push_str(&entry.display_name);
        let end = self.composer_input.len();
        self.composer_input.push(' ');

        self.composer_mention_completions.push(MentionCompletion {
            byte_start: start,
            byte_end: end,
            user_id: entry.user_id,
        });
        self.composer_mention_query = None;
        self.composer_mention_selected = 0;
        true
    }

    /// Closes the picker without inserting anything. The literal `@query`
    /// stays in the composer.
    pub fn cancel_composer_mention(&mut self) {
        self.composer_mention_query = None;
        self.composer_mention_selected = 0;
    }

    fn reset_mention_picker_state(&mut self) {
        self.composer_mention_query = None;
        self.composer_mention_selected = 0;
        self.composer_mention_completions.clear();
    }

    fn invalidate_dropped_mention_completions(&mut self) {
        let len = self.composer_input.len();
        self.composer_mention_completions
            .retain(|completion| completion.byte_end <= len);
    }

    fn clamp_selection_indices(&mut self) {
        self.selected_guild = self.selected_guild();
        self.selected_channel = self.selected_channel();
        self.selected_message = self.selected_message();
        self.selected_member = self.selected_member();
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    fn clamp_active_selection(&mut self) {
        if let ActiveGuildScope::Guild(guild_id) = self.active_guild
            && !self
                .discord
                .guilds()
                .iter()
                .any(|guild| guild.id == guild_id)
        {
            self.active_guild = ActiveGuildScope::Unset;
        }

        let active_channel_is_valid = self
            .active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
            .is_some_and(|channel| match self.active_guild {
                ActiveGuildScope::Unset => false,
                ActiveGuildScope::DirectMessages => {
                    channel.guild_id.is_none() && !channel.is_category()
                }
                ActiveGuildScope::Guild(guild_id) => {
                    channel.guild_id == Some(guild_id)
                        && !channel.is_category()
                        && self.discord.can_view_channel(channel)
                }
            });
        if self.active_channel_id.is_some() && !active_channel_is_valid {
            self.clear_active_channel();
        }
    }

    fn clear_active_channel(&mut self) {
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.cancel_composer();
        self.close_message_action_menu();
        self.close_channel_action_menu();
        self.close_emoji_reaction_picker();
        self.close_poll_vote_picker();
        self.close_reaction_users_popup();
    }

    fn clamp_list_viewports(&mut self) {
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    fn clamp_guild_viewport(&mut self) {
        let entries_len = self.guild_pane_entries().len();
        self.selected_guild = clamp_selected_index(self.selected_guild, entries_len);
        self.guild_scroll = clamp_list_scroll(
            self.selected_guild,
            self.guild_scroll,
            pane_content_height(self.guild_view_height),
            entries_len,
        );
    }

    fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_entries().len();
        self.selected_channel = clamp_selected_index(self.selected_channel, entries_len);
        self.channel_scroll = clamp_list_scroll(
            self.selected_channel,
            self.channel_scroll,
            pane_content_height(self.channel_view_height),
            entries_len,
        );
    }

    fn clamp_member_viewport(&mut self) {
        let members_len = self.flattened_members().len();
        if members_len == 0 {
            self.selected_member = 0;
            self.member_scroll = 0;
            return;
        }

        self.selected_member = self.selected_member.min(members_len - 1);
        self.member_scroll = clamp_list_scroll(
            self.selected_member_line(),
            self.member_scroll,
            pane_content_height(self.member_view_height),
            self.count_member_lines(),
        );
    }

    fn selected_member_line(&self) -> usize {
        let selected_member = self.selected_member();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for _member in group.entries {
                if member_index == selected_member {
                    return line_index;
                }
                member_index += 1;
                line_index += 1;
            }
        }
        0
    }

    fn select_member_near_line(&mut self, target_line: usize) {
        let mut last_member = None;
        for (member_index, line_index) in self.member_line_indices() {
            if line_index >= target_line {
                self.selected_member = member_index;
                return;
            }
            last_member = Some(member_index);
        }

        if let Some(member_index) = last_member {
            self.selected_member = member_index;
        }
    }

    fn member_line_indices(&self) -> Vec<(usize, usize)> {
        let mut indices = Vec::new();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for _member in group.entries {
                indices.push((member_index, line_index));
                member_index += 1;
                line_index += 1;
            }
        }
        indices
    }

    fn count_member_lines(&self) -> usize {
        let mut lines = 0usize;
        for group in self.members_grouped() {
            if lines > 0 {
                lines += 1;
            }
            lines += 1 + group.entries.len();
        }
        lines
    }

    fn follow_latest_message(&mut self) {
        self.selected_message = self.messages().len().saturating_sub(1);
        self.message_scroll = self.selected_message;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
    }

    fn align_message_viewport_to_bottom(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .message_rendered_height_at(index, content_width, preview_width, max_preview_height)
                .max(1);
            if message_height >= remaining {
                self.message_scroll = index;
                self.message_line_scroll = message_height.saturating_sub(remaining);
                return;
            }
            remaining = remaining.saturating_sub(message_height);
        }
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    fn restore_message_position(
        &mut self,
        selected_message_id: Option<Id<MessageMarker>>,
        scroll_message_id: Option<Id<MessageMarker>>,
    ) {
        let message_ids: Vec<_> = self
            .messages()
            .into_iter()
            .map(|message| message.id)
            .collect();
        if let Some(message_id) = selected_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.selected_message = index;
        }
        if let Some(message_id) = scroll_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.message_scroll = index;
        }
    }

    fn clamp_message_viewport(&mut self) {
        let messages_len = self.messages().len();
        if messages_len == 0 {
            self.selected_message = 0;
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        self.selected_message = self.selected_message.min(messages_len - 1);
        self.message_scroll = self.message_scroll.min(messages_len - 1);
        if self.message_content_width == usize::MAX {
            self.message_scroll = clamp_list_scroll(
                self.selected_message,
                self.message_scroll,
                self.message_content_height(),
                messages_len,
            );
            if self.message_scroll != self.selected_message {
                self.message_line_scroll = 0;
            }
        }
    }

    fn center_selected_message(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> bool {
        let selected = self.selected_message();
        let height = self.message_content_height();
        if self.messages().get(selected).is_none() {
            return false;
        }
        let selected_height = self
            .message_rendered_height_at(selected, content_width, preview_width, max_preview_height)
            .max(1);
        let mut top = selected;
        let mut offset = 0usize;
        let mut remaining = (height / 2).saturating_sub(selected_height / 2);

        while remaining > 0 && top > 0 {
            let previous_index = top.saturating_sub(1);
            if self.messages().get(previous_index).is_none() {
                break;
            }
            let previous_height = self
                .message_rendered_height_at(
                    previous_index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .max(1);
            if remaining >= previous_height {
                remaining = remaining.saturating_sub(previous_height);
                top = previous_index;
                offset = 0;
            } else {
                top = previous_index;
                offset = previous_height.saturating_sub(remaining);
                remaining = 0;
            }
        }

        if remaining > 0 || !self.message_viewport_has_rows_below(top, offset, height) {
            return false;
        }

        self.message_scroll = top;
        self.message_line_scroll = offset;
        true
    }

    fn message_viewport_has_rows_below(&self, top: usize, offset: usize, height: usize) -> bool {
        let mut visible_rows = 0usize;
        for offset_from_top in 0..self.messages().len().saturating_sub(top) {
            let global_index = top + offset_from_top;
            let message_height = self
                .message_rendered_height_at(
                    global_index,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
                )
                .max(1);
            let visible_height = if offset_from_top == 0 {
                message_height.saturating_sub(offset)
            } else {
                message_height
            };
            visible_rows = visible_rows.saturating_add(visible_height);
            if visible_rows >= height {
                return true;
            }
        }
        false
    }

    fn scroll_message_viewport_down_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let messages_len = self.messages().len();
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        scroll_message_row_down(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            messages_len,
            current_message_height,
        );
    }

    fn scroll_message_viewport_up_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.message_line_scroll > 0 {
            scroll_message_row_up(
                &mut self.message_scroll,
                &mut self.message_line_scroll,
                None,
            );
            return;
        }
        let previous_message_index = self.message_scroll.checked_sub(1);
        let previous_message_height = previous_message_index.map(|index| {
            self.message_rendered_height_at(index, content_width, preview_width, max_preview_height)
        });
        scroll_message_row_up(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            previous_message_height,
        );
    }

    fn normalize_message_line_scroll(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        normalize_message_line_scroll(&mut self.message_line_scroll, current_message_height);
    }

    fn message_content_height(&self) -> usize {
        pane_content_height(self.message_view_height)
    }

    fn selected_message_rendered_row(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let span = self.selected_message.saturating_sub(self.message_scroll);
        let row: usize = (0..span)
            .map(|offset| {
                self.message_rendered_height_at(
                    self.message_scroll + offset,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum();
        row.saturating_sub(self.message_line_scroll)
    }

    fn selected_message_rendered_height(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.messages().get(self.selected_message).is_none() {
            return 1;
        }
        self.message_rendered_height_at(
            self.selected_message,
            content_width,
            preview_width,
            max_preview_height,
        )
    }

    fn following_message_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
        count: usize,
    ) -> usize {
        let messages_len = self.messages().len();
        let start = self.selected_message.saturating_add(1);
        (0..count)
            .map(|offset| start + offset)
            .take_while(|&index| index < messages_len)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    pub(crate) fn message_base_line_count_for_width(
        &self,
        message: &MessageState,
        content_width: usize,
    ) -> usize {
        1 + message_format::format_message_content_lines(message, self, content_width).len()
    }

    fn message_rendered_height(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let preview_height = message
            .first_inline_preview()
            .map(|preview| {
                media::image_preview_height_for_dimensions(
                    preview_width,
                    max_preview_height,
                    preview.width,
                    preview.height,
                )
            })
            .unwrap_or(0);
        self.message_base_line_count_for_width(message, content_width)
            + usize::from(preview_height)
            + ui::MESSAGE_ROW_GAP
    }

    /// Same as `message_rendered_height` but also accounts for an optional
    /// date-separator line above the message body. Use this everywhere the
    /// caller knows the message's index inside `self.messages()` so scroll
    /// math stays consistent with what the renderer actually paints.
    fn message_rendered_height_at(
        &self,
        index: usize,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let messages = self.messages();
        let Some(message) = messages.get(index).copied() else {
            return 0;
        };
        self.message_rendered_height(message, content_width, preview_width, max_preview_height)
            + self.message_extra_top_lines(index)
    }
}

#[cfg(test)]
fn message_rendered_height(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    DashboardState::new().message_rendered_height(
        message,
        content_width,
        preview_width,
        max_preview_height,
    )
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
