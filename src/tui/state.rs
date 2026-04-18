use std::collections::{HashMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::{
    AppCommand, AppEvent, DiscordState, MentionInfo, MessageInfo, MessageSnapshotInfo, MessageState,
};

use super::format::{
    RenderedText, TextHighlightKind, render_user_mentions, render_user_mentions_with_highlights,
};
use super::{media, message_format, ui};

mod channels;
mod composer;
mod diagnostics;
mod emoji;
mod guilds;
mod member_grouping;
mod message_actions;
mod message_render;
mod model;
mod polls;
mod popups;
mod presentation;
mod scroll;
mod subscriptions;
mod user;

use composer::{
    MentionCompletion, build_mention_candidates, expand_mention_completions, is_mention_query_char,
    move_mention_selection, should_start_mention_query,
};
use emoji::{custom_emoji_reaction_item, unicode_emoji_reaction_items};
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use popups::{ChannelActionMenuState, MemberActionMenuState, UserProfilePopupState};
use scroll::{
    SCROLL_OFF, clamp_list_scroll, clamp_selected_index, last_index, move_index_down,
    move_index_down_by, move_index_up, move_index_up_by, normalize_message_line_scroll,
    pane_content_height, scroll_message_row_down, scroll_message_row_up,
};

pub use composer::{MAX_MENTION_PICKER_VISIBLE, MentionPickerEntry};
pub use member_grouping::{MemberEntry, MemberGroup};
pub use model::{
    ChannelActionItem, ChannelPaneEntry, ChannelThreadItem, EmojiReactionItem, FocusPane,
    GuildPaneEntry, MemberActionItem, MessageActionItem, MessageActionKind, PollVotePickerItem,
    ThreadSummary,
};
#[allow(unused_imports)]
pub use model::{ChannelActionKind, ChannelBranch, GuildBranch};
pub use popups::{
    EmojiReactionPickerState, MessageActionMenuState, PollVotePickerState, ReactionUsersPopupState,
};
pub use presentation::{discord_color, folder_color, presence_color, presence_marker};

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

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn is_channel_action_menu_open(&self) -> bool {
        self.channel_action_menu.is_some()
    }

    pub fn is_channel_action_threads_phase(&self) -> bool {
        matches!(
            self.channel_action_menu,
            Some(ChannelActionMenuState::Threads { .. })
        )
    }

    pub fn is_emoji_reaction_picker_open(&self) -> bool {
        self.emoji_reaction_picker.is_some()
    }

    pub fn is_reaction_users_popup_open(&self) -> bool {
        self.reaction_users_popup.is_some()
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

    pub fn close_emoji_reaction_picker(&mut self) {
        self.emoji_reaction_picker = None;
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

    pub fn selected_emoji_reaction_index(&self) -> Option<usize> {
        self.emoji_reaction_picker
            .as_ref()
            .map(|picker| clamp_selected_index(picker.selected, self.emoji_reaction_items().len()))
    }

    pub fn selected_emoji_reaction(&self) -> Option<EmojiReactionItem> {
        let index = self.selected_emoji_reaction_index()?;
        self.emoji_reaction_items().get(index).cloned()
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
