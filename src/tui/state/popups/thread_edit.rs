use crate::discord::AppCommand;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker},
};
use crate::tui::keybindings::ScrollAction;

use super::super::{DashboardState, ThreadEditField, ThreadEditTagView, ThreadEditView};
use super::{ActiveModalPopupKind, ModalPopup, ThreadEditState};

/// Discord allows at most five tags applied to a single forum post.
const MAX_FORUM_POST_TAGS: usize = 5;

/// Slow-mode (`rate_limit_per_user`) options in seconds, with display labels.
/// Mirrors Discord's General settings dropdown.
pub(super) const SLOW_MODE_OPTIONS: [(u64, &str); 14] = [
    (0, "Off"),
    (5, "5s"),
    (10, "10s"),
    (15, "15s"),
    (30, "30s"),
    (60, "1m"),
    (120, "2m"),
    (300, "5m"),
    (600, "10m"),
    (900, "15m"),
    (1800, "30m"),
    (3600, "1h"),
    (7200, "2h"),
    (21600, "6h"),
];

/// Auto-archive (`auto_archive_duration`) options in minutes, with labels.
pub(super) const AUTO_ARCHIVE_OPTIONS: [(u64, &str); 4] = [
    (60, "1 hour"),
    (1440, "1 day"),
    (4320, "3 days"),
    (10080, "1 week"),
];

/// The closest option index for `value`, so a thread whose stored cooldown is
/// not exactly one of the presented options still seeds a sensible selection.
fn nearest_option_index(options: &[(u64, &str)], value: u64) -> usize {
    options
        .iter()
        .enumerate()
        .min_by_key(|(_, (option, _))| option.abs_diff(value))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

impl super::super::DashboardState {
    pub fn open_thread_edit(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return;
        };
        let title = channel.name.clone();
        // Tags live on the parent forum, so a thread under a non-forum parent has
        // no Tags field at all.
        let is_forum_post = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum());
        let selected_tag_ids = channel.applied_tags.clone();
        let rate_limit_index =
            nearest_option_index(&SLOW_MODE_OPTIONS, channel.rate_limit_per_user.unwrap_or(0));
        let auto_archive_minutes = channel
            .thread_metadata
            .as_ref()
            .and_then(|metadata| metadata.auto_archive_duration)
            .unwrap_or(AUTO_ARCHIVE_OPTIONS[1].0);
        let auto_archive_index = nearest_option_index(&AUTO_ARCHIVE_OPTIONS, auto_archive_minutes);
        // Slow mode is only editable with the manage-channel permission, the
        // same gate the action menu uses for moderator-only post actions.
        let can_set_slow_mode = self
            .discord
            .cache
            .can_manage_channel_structure_in_channel(channel);

        let mut edit_title = crate::tui::text_input::TextInputState::default();
        edit_title.set_value(title.clone());

        self.popups.modal = Some(ModalPopup::ThreadEdit(ThreadEditState {
            channel_id,
            is_forum_post,
            title: edit_title,
            editing_title: false,
            edit_input: crate::tui::text_input::TextInputState::default(),
            selected_tag_ids,
            tag_order: Vec::new(),
            selected_tag_index: 0,
            editing_tags: false,
            rate_limit_index,
            auto_archive_index,
            can_set_slow_mode,
            active_field: ThreadEditField::Title,
            status: None,
            scroll: super::ScrollablePopupState::default(),
            pending_scroll_reveal: true,
        }));
    }

    pub fn close_thread_edit(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ThreadEdit) {
            self.popups.clear_modal();
        }
    }

    pub fn is_thread_edit_tag_picker_active(&self) -> bool {
        self.popups
            .thread_edit()
            .is_some_and(|popup| popup.editing_tags)
    }

    pub fn is_thread_edit_title_editing(&self) -> bool {
        self.popups
            .thread_edit()
            .is_some_and(|popup| popup.editing_title)
    }

    pub fn thread_edit_scroll(&self) -> usize {
        self.popups
            .thread_edit()
            .map(|popup| popup.scroll.scroll())
            .unwrap_or(0)
    }

    pub fn scroll_thread_edit(&mut self, action: ScrollAction) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            match action {
                ScrollAction::Down => popup.scroll.scroll_down(),
                ScrollAction::Up => popup.scroll.scroll_up(),
            }
        }
    }

    pub fn request_thread_edit_scroll_reveal(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            popup.pending_scroll_reveal = true;
        }
    }

    pub fn set_thread_edit_metrics(&mut self, view_height: usize, total_lines: usize) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            popup.scroll.set_view_height(view_height);
            popup.scroll.set_total_lines(total_lines);
        }
    }

    pub fn reveal_thread_edit_rows(&mut self, start: usize, end: usize) {
        if let Some(popup) = self.popups.thread_edit_mut()
            && popup.pending_scroll_reveal
        {
            popup.scroll.reveal(start, end);
            popup.pending_scroll_reveal = false;
        }
    }

    pub fn thread_edit_view(&self) -> Option<ThreadEditView> {
        let popup = self.popups.thread_edit()?;
        let channel = self.discord.cache.channel(popup.channel_id)?;
        // The available tags and the require-tag rule live on the PARENT forum,
        // not on the post thread, so resolve them through `parent_id`.
        let forum = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id));
        let available_tags: &[crate::discord::ForumTagInfo] =
            forum.map_or(&[], |forum| forum.available_tags.as_slice());
        let requires_tag = forum.is_some_and(|forum| forum.requires_forum_tag());
        // While the picker is open we render the snapshot order captured on
        // entry; otherwise we sort selected tags to the top live.
        let display_ids: Vec<Id<ForumTagMarker>> =
            if popup.editing_tags && !popup.tag_order.is_empty() {
                popup.tag_order.clone()
            } else {
                available_tags
                    .iter()
                    .map(|tag| tag.id)
                    .filter(|id| popup.selected_tag_ids.contains(id))
                    .chain(
                        available_tags
                            .iter()
                            .map(|tag| tag.id)
                            .filter(|id| !popup.selected_tag_ids.contains(id)),
                    )
                    .collect()
            };
        let cap_reached = popup.selected_tag_ids.len() >= MAX_FORUM_POST_TAGS;
        let guild_id = channel.guild_id;
        let tags = display_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let tag = available_tags.iter().find(|tag| tag.id == *id)?;
                let selected = popup.selected_tag_ids.contains(&tag.id);
                let emoji = self.forum_tag_emoji_fields(tag, guild_id);
                Some(ThreadEditTagView {
                    name: tag.name.clone(),
                    unicode_emoji: emoji.unicode_emoji,
                    custom_emoji_url: emoji.custom_emoji_url,
                    custom_emoji_label: emoji.custom_emoji_label,
                    selected,
                    active: popup.editing_tags && index == popup.selected_tag_index,
                    selectable: selected || !cap_reached,
                })
            })
            .collect();

        let title = if popup.editing_title {
            popup.edit_input.value().to_owned()
        } else {
            popup.title.value().to_owned()
        };
        let title_cursor = if popup.editing_title {
            popup.edit_input.cursor_byte_index()
        } else {
            popup.title.cursor_byte_index()
        };

        Some(ThreadEditView {
            channel_label: format!("#{}", channel.name),
            active_field: popup.active_field,
            editing_title: popup.editing_title,
            editing_tags: popup.editing_tags,
            title,
            title_cursor,
            is_forum_post: popup.is_forum_post,
            tags,
            requires_tag,
            slow_mode_label: SLOW_MODE_OPTIONS[popup.rate_limit_index].1.to_owned(),
            can_set_slow_mode: popup.can_set_slow_mode,
            auto_archive_label: AUTO_ARCHIVE_OPTIONS[popup.auto_archive_index].1.to_owned(),
            status: popup.status.clone(),
        })
    }

    pub fn cycle_thread_edit_field_next(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            if popup.editing_title || popup.editing_tags {
                return;
            }
            // Tags only exist on forum posts, so a regular thread steps straight
            // from Title to SlowMode.
            popup.active_field = match popup.active_field {
                ThreadEditField::Title if popup.is_forum_post => ThreadEditField::Tags,
                ThreadEditField::Title => ThreadEditField::SlowMode,
                ThreadEditField::Tags => ThreadEditField::SlowMode,
                ThreadEditField::SlowMode => ThreadEditField::AutoArchive,
                ThreadEditField::AutoArchive => ThreadEditField::Submit,
                ThreadEditField::Submit => ThreadEditField::Cancel,
                // Selection stops at the last field instead of wrapping around.
                ThreadEditField::Cancel => ThreadEditField::Cancel,
            };
        }
    }

    pub fn cycle_thread_edit_field_previous(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            if popup.editing_title || popup.editing_tags {
                return;
            }
            // Mirror the forward direction: a regular thread has no Tags field, so
            // SlowMode steps back to Title directly.
            popup.active_field = match popup.active_field {
                // Selection stops at the first field instead of wrapping around.
                ThreadEditField::Title => ThreadEditField::Title,
                ThreadEditField::Tags => ThreadEditField::Title,
                ThreadEditField::SlowMode if popup.is_forum_post => ThreadEditField::Tags,
                ThreadEditField::SlowMode => ThreadEditField::Title,
                ThreadEditField::AutoArchive => ThreadEditField::SlowMode,
                ThreadEditField::Submit => ThreadEditField::AutoArchive,
                ThreadEditField::Cancel => ThreadEditField::Submit,
            };
        }
    }

    pub fn move_thread_edit_selection_down(&mut self) {
        let Some((editing_tags, tag_count)) = self
            .popups
            .thread_edit()
            .map(|popup| (popup.editing_tags, popup.tag_order.len()))
        else {
            return;
        };
        if editing_tags {
            if tag_count > 0 {
                if let Some(popup) = self.popups.thread_edit_mut() {
                    popup.selected_tag_index =
                        (popup.selected_tag_index + 1).min(tag_count.saturating_sub(1));
                }
            }
        } else {
            self.cycle_thread_edit_field_next();
        }
    }

    pub fn move_thread_edit_selection_up(&mut self) {
        let editing_tags = self
            .popups
            .thread_edit()
            .is_some_and(|popup| popup.editing_tags);
        if editing_tags {
            if let Some(popup) = self.popups.thread_edit_mut() {
                popup.selected_tag_index = popup.selected_tag_index.saturating_sub(1);
            }
        } else {
            self.cycle_thread_edit_field_previous();
        }
    }

    /// Cycle the focused selector (slow mode or auto-archive). Slow mode only
    /// moves with the manage-channel permission.
    pub fn cycle_thread_edit_selector(&mut self, forward: bool) {
        let Some(popup) = self.popups.thread_edit_mut() else {
            return;
        };
        match popup.active_field {
            ThreadEditField::SlowMode => {
                if !popup.can_set_slow_mode {
                    return;
                }
                popup.rate_limit_index =
                    cycle_index(popup.rate_limit_index, SLOW_MODE_OPTIONS.len(), forward);
                popup.status = None;
            }
            ThreadEditField::AutoArchive => {
                popup.auto_archive_index = cycle_index(
                    popup.auto_archive_index,
                    AUTO_ARCHIVE_OPTIONS.len(),
                    forward,
                );
                popup.status = None;
            }
            ThreadEditField::Title
            | ThreadEditField::Tags
            | ThreadEditField::Submit
            | ThreadEditField::Cancel => {}
        }
    }

    pub fn push_thread_edit_char(&mut self, value: char) {
        if let Some(popup) = self.popups.thread_edit_mut()
            && popup.editing_title
            && value != '\n'
        {
            popup.edit_input.insert_char(value);
            popup.status = None;
        }
    }

    pub fn insert_thread_edit_text(&mut self, value: &str) -> bool {
        let Some(popup) = self.popups.thread_edit_mut() else {
            return false;
        };
        if !popup.editing_title {
            return false;
        }
        let pasted: String = value.chars().filter(|value| *value != '\r').collect();
        let single_line = pasted.lines().next().unwrap_or_default();
        if single_line.is_empty() {
            return false;
        }
        popup.edit_input.insert_str(single_line);
        popup.status = None;
        true
    }

    pub fn delete_thread_edit_previous_char(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut()
            && popup.editing_title
            && popup.edit_input.delete_previous_grapheme()
        {
            popup.status = None;
        }
    }

    pub fn delete_thread_edit_previous_word(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut()
            && popup.editing_title
            && popup.edit_input.delete_previous_word()
        {
            popup.status = None;
        }
    }

    pub fn move_thread_edit_cursor_left(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_left());
    }

    pub fn move_thread_edit_cursor_right(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_right());
    }

    pub fn move_thread_edit_cursor_word_left(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_word_left());
    }

    pub fn move_thread_edit_cursor_word_right(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_word_right());
    }

    pub fn move_thread_edit_cursor_home(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_home());
    }

    pub fn move_thread_edit_cursor_end(&mut self) {
        self.with_thread_edit_title_input(|input| input.move_end());
    }

    fn with_thread_edit_title_input(
        &mut self,
        action: impl FnOnce(&mut crate::tui::text_input::TextInputState),
    ) {
        if let Some(popup) = self.popups.thread_edit_mut()
            && popup.editing_title
        {
            action(&mut popup.edit_input);
        }
    }

    pub fn clear_thread_edit_active_field(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            if popup.editing_title {
                popup.edit_input.clear();
                popup.status = None;
                return;
            }
            if popup.active_field == ThreadEditField::Tags {
                popup.selected_tag_ids.clear();
                popup.status = None;
            }
        }
    }

    pub fn toggle_selected_thread_edit_tag(&mut self) {
        let Some(tag_id) = self
            .popups
            .thread_edit()
            .and_then(|popup| popup.tag_order.get(popup.selected_tag_index).copied())
        else {
            return;
        };
        if let Some(popup) = self.popups.thread_edit_mut() {
            if let Some(position) = popup.selected_tag_ids.iter().position(|id| *id == tag_id) {
                popup.selected_tag_ids.remove(position);
                popup.status = None;
            } else if popup.selected_tag_ids.len() < MAX_FORUM_POST_TAGS {
                popup.selected_tag_ids.push(tag_id);
                popup.status = None;
            }
        }
    }

    /// Activate the focused cell. Title starts inline editing (or commits it if
    /// already editing); Tags opens the picker (or toggles inside it); the
    /// selectors do nothing on Enter (they cycle with the arrows); Submit saves;
    /// Cancel closes.
    pub fn activate_thread_edit(&mut self) -> Option<AppCommand> {
        let (active_field, editing_title, editing_tags) = self
            .popups
            .thread_edit()
            .map(|popup| (popup.active_field, popup.editing_title, popup.editing_tags))?;
        if editing_tags && active_field == ThreadEditField::Tags {
            self.toggle_selected_thread_edit_tag();
            return None;
        }
        if editing_title && active_field == ThreadEditField::Title {
            self.commit_thread_edit_title();
            return None;
        }
        match active_field {
            ThreadEditField::Title => self.start_thread_edit_title(),
            ThreadEditField::Tags => self.start_thread_edit_tag_selection(),
            ThreadEditField::Submit => return self.submit_thread_edit(),
            ThreadEditField::Cancel => self.close_thread_edit(),
            // The selectors only respond to the arrow keys.
            ThreadEditField::SlowMode | ThreadEditField::AutoArchive => {}
        }
        None
    }

    pub fn close_or_cancel_thread_edit(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            if popup.editing_title {
                self.commit_thread_edit_title();
                return;
            }
            if popup.editing_tags {
                popup.editing_tags = false;
                popup.tag_order.clear();
                popup.status = None;
                return;
            }
        }
        self.close_thread_edit();
    }

    fn start_thread_edit_title(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            let value = popup.title.value().to_owned();
            popup.editing_title = true;
            popup.edit_input.set_value(value);
            popup.status = None;
        }
    }

    fn commit_thread_edit_title(&mut self) {
        if let Some(popup) = self.popups.thread_edit_mut() {
            let value = popup.edit_input.value().to_owned();
            popup.title.set_value(value);
            popup.editing_title = false;
            popup.edit_input.clear();
            popup.status = None;
        }
    }

    /// Tags a post can apply are configured on the parent forum channel, not on
    /// the post thread, so resolve them through `parent_id`.
    fn thread_edit_available_tags(
        &self,
        thread_id: Id<ChannelMarker>,
    ) -> Vec<crate::discord::ForumTagInfo> {
        self.discord
            .cache
            .channel(thread_id)
            .and_then(|thread| thread.parent_id)
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .map(|forum| forum.available_tags.clone())
            .unwrap_or_default()
    }

    fn start_thread_edit_tag_selection(&mut self) {
        // A regular thread has no tags, so there is no picker to open.
        let Some(channel_id) = self
            .popups
            .thread_edit()
            .filter(|popup| popup.is_forum_post)
            .map(|popup| popup.channel_id)
        else {
            return;
        };
        let selected_ids = self
            .popups
            .thread_edit()
            .map(|popup| popup.selected_tag_ids.clone())
            .unwrap_or_default();
        // Available tags come from the parent forum, not the post thread.
        let available_tags = self.thread_edit_available_tags(channel_id);
        let ordered: Vec<Id<ForumTagMarker>> = available_tags
            .iter()
            .map(|tag| tag.id)
            .filter(|id| selected_ids.contains(id))
            .chain(
                available_tags
                    .iter()
                    .map(|tag| tag.id)
                    .filter(|id| !selected_ids.contains(id)),
            )
            .collect();
        let Some(popup) = self.popups.thread_edit_mut() else {
            return;
        };
        if ordered.is_empty() {
            popup.status = Some("no tags available".to_owned());
            return;
        }
        popup.tag_order = ordered;
        popup.selected_tag_index = 0;
        popup.editing_tags = true;
        popup.status = None;
    }

    pub fn submit_thread_edit(&mut self) -> Option<AppCommand> {
        match self.build_thread_edit() {
            Ok(command) => {
                self.close_thread_edit();
                Some(command)
            }
            Err(message) => {
                if let Some(popup) = self.popups.thread_edit_mut() {
                    popup.status = Some(message);
                }
                None
            }
        }
    }

    fn build_thread_edit(&self) -> Result<AppCommand, String> {
        let Some(popup) = self.popups.thread_edit() else {
            return Err("thread editor is not open".to_owned());
        };
        let channel_id = popup.channel_id;
        let name = popup.title.value().trim().to_owned();
        if name.is_empty() {
            return Err("title is required".to_owned());
        }
        if name.chars().count() > 100 {
            return Err("title must be 100 characters or fewer".to_owned());
        }
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return Err("thread is no longer available".to_owned());
        };
        let applied_tags = popup.selected_tag_ids.clone();
        // The require-tag rule is a parent-forum setting, not a thread setting.
        let requires_tag = channel
            .parent_id
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|forum| forum.requires_forum_tag());
        if requires_tag && applied_tags.is_empty() {
            return Err("at least one tag is required".to_owned());
        }
        let rate_limit_per_user = SLOW_MODE_OPTIONS[popup.rate_limit_index].0;
        let auto_archive_duration = AUTO_ARCHIVE_OPTIONS[popup.auto_archive_index].0;
        Ok(AppCommand::EditThread {
            channel_id,
            name,
            applied_tags,
            rate_limit_per_user,
            auto_archive_duration,
            label: self.channel_label(channel_id),
        })
    }
}

/// Step `index` forward or backward within `len`, clamping at the ends so the
/// selectors do not wrap (matching Discord's bounded dropdowns).
fn cycle_index(index: usize, len: usize, forward: bool) -> usize {
    if forward {
        (index + 1).min(len.saturating_sub(1))
    } else {
        index.saturating_sub(1)
    }
}

/// Display-ready emoji fields for one forum tag, resolved for the tag picker.
pub(in crate::tui::state) struct ForumTagEmojiFields {
    pub unicode_emoji: Option<String>,
    pub custom_emoji_url: Option<String>,
    pub custom_emoji_label: Option<String>,
}

impl DashboardState {
    pub(in crate::tui::state) fn forum_tag_emoji_fields(
        &self,
        tag: &crate::discord::ForumTagInfo,
        guild_id: Option<Id<crate::discord::ids::marker::GuildMarker>>,
    ) -> ForumTagEmojiFields {
        if let Some(emoji_id) = tag.emoji_id {
            // The tag payload omits the custom emoji name, so the `:name:`
            // fallback is resolved from the guild emoji cache.
            let url = format!("https://cdn.discordapp.com/emojis/{}.png", emoji_id.get());
            let resolved_name = guild_id.and_then(|guild_id| {
                self.discord
                    .cache
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .find(|emoji| emoji.id == emoji_id)
                    .map(|emoji| emoji.name.clone())
            });
            let label = resolved_name
                .map(|name| format!(":{name}:"))
                .unwrap_or_else(|| ":custom:".to_owned());
            return ForumTagEmojiFields {
                unicode_emoji: None,
                custom_emoji_url: Some(url),
                custom_emoji_label: Some(label),
            };
        }

        let unicode = tag
            .emoji_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        ForumTagEmojiFields {
            unicode_emoji: unicode,
            custom_emoji_url: None,
            custom_emoji_label: None,
        }
    }
}
