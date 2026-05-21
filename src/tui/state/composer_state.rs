use std::ops::Range;

use crate::discord::{
    AppCommand, ApplicationCommandInfo, ApplicationCommandInteraction,
    ApplicationCommandInteractionOption, ApplicationCommandOptionInfo, MAX_UPLOAD_ATTACHMENT_COUNT,
    MessageAttachmentUpload,
};
use serde_json::{Number, Value};

use super::composer::{
    ComposerEmojiImageCompletion, EmojiCompletion, MentionCompletion, build_command_candidates,
    build_command_choice_candidates, build_command_option_candidates, build_emoji_candidates,
    build_mention_candidates, expand_composer_completions, expand_emoji_shortcodes,
    is_command_query_char, is_emoji_query_char, is_mention_query_char, move_picker_selection,
    should_start_completion_query,
};
use super::{CommandPickerEntry, DashboardState, EmojiPickerEntry, FocusPane, MentionPickerEntry};

impl DashboardState {
    pub fn is_composing(&self) -> bool {
        self.composer.composer_active
    }

    pub(super) fn start_reply_composer(&mut self) {
        let Some(message_id) = self.selected_message_state().map(|message| message.id) else {
            return;
        };
        // Replies are sends, so the channel must allow SEND_MESSAGES for the
        // action to be useful.
        if !self.can_send_in_selected_channel() {
            return;
        }
        self.composer.composer_input.clear();
        self.composer.composer_cursor_byte_index = 0;
        self.composer.pending_composer_attachments.clear();
        self.runtime.clipboard_paste_pending = false;
        self.composer.reply_target_message_id = Some(message_id);
        self.composer.edit_target_message = None;
        self.reset_mention_picker_state();
        self.composer.composer_active = true;
        self.navigation.focus = FocusPane::Messages;
    }

    pub(super) fn start_edit_composer(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if Some(message.author_id) != self.discord.current_user_id
            || !message.message_kind.is_regular_or_reply()
        {
            return;
        }
        let Some(content) = message.content.clone() else {
            return;
        };
        let channel_id = message.channel_id;
        let message_id = message.id;
        self.composer.composer_input = content;
        self.composer.composer_cursor_byte_index = self.composer.composer_input.len();
        self.composer.pending_composer_attachments.clear();
        self.runtime.clipboard_paste_pending = false;
        self.composer.reply_target_message_id = None;
        self.composer.edit_target_message = Some((channel_id, message_id));
        self.reset_mention_picker_state();
        self.composer.composer_active = true;
        self.navigation.focus = FocusPane::Messages;
    }

    pub fn composer_input(&self) -> &str {
        &self.composer.composer_input
    }

    pub fn composer_cursor_byte_index(&self) -> usize {
        clamp_cursor_index(
            &self.composer.composer_input,
            self.composer.composer_cursor_byte_index,
        )
    }

    pub fn pending_composer_attachments(&self) -> &[MessageAttachmentUpload] {
        &self.composer.pending_composer_attachments
    }

    pub fn composer_title(&self) -> &'static str {
        if self.composer.edit_target_message.is_some() {
            " Edit Message "
        } else if self.composer.reply_target_message_id.is_some() {
            " Reply "
        } else {
            " Message Input "
        }
    }

    pub fn add_pending_composer_attachments(&mut self, attachments: Vec<MessageAttachmentUpload>) {
        if attachments.is_empty() || !self.composer_accepts_attachments() {
            return;
        }
        let available = MAX_UPLOAD_ATTACHMENT_COUNT
            .saturating_sub(self.composer.pending_composer_attachments.len());
        self.composer
            .pending_composer_attachments
            .extend(attachments.into_iter().take(available));
    }

    pub fn pop_pending_composer_attachment(&mut self) {
        self.composer.pending_composer_attachments.pop();
    }

    pub fn composer_accepts_attachments(&self) -> bool {
        self.composer.edit_target_message.is_none() && self.can_attach_in_selected_channel()
    }

    /// Whether the user can post messages in the currently selected channel.
    /// Returns `true` when no channel is selected so callers don't have to
    /// special-case the empty state.
    pub fn can_send_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) if channel.is_forum() => false,
            Some(channel) => self.discord.cache.can_send_in_channel(channel),
            None => true,
        }
    }

    /// Whether the user can attach files in the currently selected channel.
    /// Paste-based attachment input uses this to decide whether file paths
    /// become pending uploads or plain composer text.
    pub fn can_attach_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) if channel.is_forum() => false,
            Some(channel) => self.discord.cache.can_attach_in_channel(channel),
            None => true,
        }
    }

    pub fn start_composer(&mut self) {
        if self.selected_channel_id().is_none() {
            return;
        }
        // Refusing here keeps the shortcut simple: the same key that opens the
        // composer in writable channels just no-ops in read-only ones, so the
        // user never lands in a typing state for a channel that would 403 on
        // submit.
        if !self.can_send_in_selected_channel() {
            return;
        }
        self.composer.reply_target_message_id = None;
        self.composer.edit_target_message = None;
        self.composer.composer_active = true;
        self.queue_application_commands_for_selected_channel();
        self.move_composer_cursor_end();
        self.navigation.focus = FocusPane::Messages;
    }

    pub fn replace_composer_input_from_editor(&mut self, value: String) {
        self.composer.composer_input = value;
        self.composer.composer_cursor_byte_index = self.composer.composer_input.len();
        self.reset_mention_picker_state();
        self.refresh_active_mention_query();
    }

    pub fn cancel_composer(&mut self) {
        self.composer.composer_active = false;
        self.composer.composer_input.clear();
        self.composer.composer_cursor_byte_index = 0;
        self.composer.pending_composer_attachments.clear();
        self.runtime.clipboard_paste_pending = false;
        self.composer.reply_target_message_id = None;
        self.composer.edit_target_message = None;
        self.reset_mention_picker_state();
    }

    pub fn close_composer(&mut self) {
        if self.composer.reply_target_message_id.is_some()
            || self.composer.edit_target_message.is_some()
        {
            self.cancel_composer();
            return;
        }
        self.composer.composer_active = false;
        self.runtime.clipboard_paste_pending = false;
        self.reset_mention_picker_state();
    }

    pub fn clear_composer_input(&mut self) {
        self.composer.composer_input.clear();
        self.composer.composer_cursor_byte_index = 0;
        self.composer.pending_composer_attachments.clear();
        self.runtime.clipboard_paste_pending = false;
        self.reset_mention_picker_state();
    }

    pub fn push_composer_char(&mut self, value: char) {
        let mut text = String::new();
        text.push(value);
        self.insert_composer_text_at_cursor(&text);
    }

    pub fn insert_composer_text_at_cursor(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        let cursor = self.composer.composer_cursor_byte_index();
        self.replace_composer_range(cursor..cursor, value);
    }

    pub fn pop_composer_char(&mut self) {
        let end = self.composer.composer_cursor_byte_index();
        if end == 0 {
            return;
        }
        let start = previous_char_boundary(&self.composer.composer_input, end);
        self.replace_composer_range(start..end, "");
    }

    pub fn delete_previous_composer_word(&mut self) {
        let end = self.composer.composer_cursor_byte_index();
        if end == 0 {
            return;
        }
        let start = previous_word_boundary(&self.composer.composer_input, end);
        self.replace_composer_range(start..end, "");
    }

    pub fn move_composer_cursor_left(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        self.composer.composer_cursor_byte_index =
            previous_char_boundary(&self.composer.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_right(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        self.composer.composer_cursor_byte_index =
            next_char_boundary(&self.composer.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_up(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        if let Some(target) = vertical_cursor_target(&self.composer.composer_input, cursor, -1) {
            self.composer.composer_cursor_byte_index = target;
            self.refresh_active_mention_query();
        }
    }

    pub fn move_composer_cursor_down(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        if let Some(target) = vertical_cursor_target(&self.composer.composer_input, cursor, 1) {
            self.composer.composer_cursor_byte_index = target;
            self.refresh_active_mention_query();
        }
    }

    pub fn move_composer_cursor_word_left(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        self.composer.composer_cursor_byte_index =
            previous_word_boundary(&self.composer.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_word_right(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        self.composer.composer_cursor_byte_index =
            next_word_boundary(&self.composer.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_home(&mut self) {
        self.composer.composer_cursor_byte_index = 0;
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_end(&mut self) {
        self.composer.composer_cursor_byte_index = self.composer.composer_input.len();
        self.refresh_active_mention_query();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let expanded = expand_composer_completions(
            &self.composer.composer_input,
            &self.composer.composer_mention_completions,
            &self.composer.composer_emoji_completions,
        );
        let expanded = expand_emoji_shortcodes(&expanded);
        let content = expanded.trim().to_owned();
        let has_attachments = !self.composer.pending_composer_attachments.is_empty();
        if content.is_empty() && !has_attachments {
            return None;
        }
        if let Some((channel_id, message_id)) = self.composer.edit_target_message.take() {
            if content.is_empty() {
                self.composer.edit_target_message = Some((channel_id, message_id));
                return None;
            }
            self.composer.composer_input.clear();
            self.composer.composer_cursor_byte_index = 0;
            self.composer.pending_composer_attachments.clear();
            self.runtime.clipboard_paste_pending = false;
            self.composer.composer_active = false;
            self.composer.reply_target_message_id = None;
            self.reset_mention_picker_state();
            return Some(AppCommand::EditMessage {
                channel_id,
                message_id,
                content,
            });
        }
        let channel_id = self.selected_channel_id()?;
        // Defense in depth: the channel could have lost SEND_MESSAGES while
        // the composer was open (role change, channel overwrite update). Drop
        // the message rather than fire a request that would 403.
        if !self.can_send_in_selected_channel() {
            self.composer.composer_input.clear();
            self.composer.composer_cursor_byte_index = 0;
            self.composer.pending_composer_attachments.clear();
            self.runtime.clipboard_paste_pending = false;
            self.composer.composer_active = false;
            self.composer.reply_target_message_id = None;
            self.composer.edit_target_message = None;
            self.reset_mention_picker_state();
            return None;
        }
        if has_attachments && !self.can_attach_in_selected_channel() {
            self.composer.composer_input.clear();
            self.composer.composer_cursor_byte_index = 0;
            self.composer.pending_composer_attachments.clear();
            self.runtime.clipboard_paste_pending = false;
            self.composer.composer_active = false;
            self.composer.reply_target_message_id = None;
            self.composer.edit_target_message = None;
            self.reset_mention_picker_state();
            return None;
        }

        if !has_attachments && self.composer.reply_target_message_id.is_none() {
            match self.application_command_submit_for_content(&content) {
                ApplicationCommandSubmit::Ready(interaction) => {
                    self.composer.composer_input.clear();
                    self.composer.composer_cursor_byte_index = 0;
                    self.reset_mention_picker_state();
                    self.composer.reply_target_message_id = None;
                    self.composer.pending_composer_attachments.clear();
                    return Some(AppCommand::RunApplicationCommand { interaction });
                }
                ApplicationCommandSubmit::Incomplete => return None,
                ApplicationCommandSubmit::NotCommand => {}
            }
        }

        self.composer.composer_input.clear();
        self.composer.composer_cursor_byte_index = 0;
        self.reset_mention_picker_state();
        let reply_to = self.composer.reply_target_message_id.take();
        let attachments = std::mem::take(&mut self.composer.pending_composer_attachments);
        // Stay in insert mode so the user can send several messages in a
        // row without re-pressing `i`. The composer closes only when the
        // user explicitly bails with Esc or the channel revokes
        // SEND_MESSAGES (handled above).
        Some(AppCommand::SendMessage {
            channel_id,
            content,
            reply_to,
            attachments,
        })
    }

    /// Returns the characters typed after the `@` if the picker is open.
    pub fn composer_mention_query(&self) -> Option<&str> {
        self.composer.composer_mention_query.as_deref()
    }

    pub fn composer_mention_selected(&self) -> usize {
        self.composer.composer_mention_selected
    }

    /// Builds the full suggestion list for the picker, ordered by best match
    /// across the member's display name AND username: prefix matches beat
    /// substring matches, alias matches beat username matches at the same rank,
    /// and ties are broken alphabetically by display name.
    pub fn composer_mention_candidates(&self) -> Vec<MentionPickerEntry> {
        let Some(query) = self.composer.composer_mention_query.as_deref() else {
            return Vec::new();
        };
        build_mention_candidates(query, self.flattened_members())
    }

    pub fn move_composer_mention_selection(&mut self, delta: isize) {
        if self.composer.composer_mention_query.is_none() {
            return;
        }
        let len = self.composer_mention_candidates().len();
        self.composer.composer_mention_selected =
            move_picker_selection(self.composer.composer_mention_selected, len, delta);
    }

    pub fn composer_emoji_query(&self) -> Option<&str> {
        self.composer.composer_emoji_query.as_deref()
    }

    pub fn composer_emoji_selected(&self) -> usize {
        self.composer.composer_emoji_selected
    }

    pub fn composer_emoji_candidates(&self) -> Vec<EmojiPickerEntry> {
        self.composer.composer_emoji_candidates.clone()
    }

    pub fn composer_command_query(&self) -> Option<&str> {
        self.composer.composer_command_query.as_deref()
    }

    pub fn composer_command_selected(&self) -> usize {
        self.composer.composer_command_selected
    }

    pub fn composer_command_candidates(&self) -> Vec<CommandPickerEntry> {
        self.composer.composer_command_candidates.clone()
    }

    pub fn move_composer_command_selection(&mut self, delta: isize) {
        if self.composer.composer_command_query.is_none() {
            return;
        }
        let len = self.composer.composer_command_candidates.len();
        self.composer.composer_command_selected =
            move_picker_selection(self.composer.composer_command_selected, len, delta);
    }

    pub fn move_composer_emoji_selection(&mut self, delta: isize) {
        if self.composer.composer_emoji_query.is_none() {
            return;
        }
        let len = self.composer.composer_emoji_candidates.len();
        self.composer.composer_emoji_selected =
            move_picker_selection(self.composer.composer_emoji_selected, len, delta);
    }

    /// Confirms the currently highlighted mention. Replaces the trailing
    /// `@query` with `@displayname ` (so the user sees what they wrote) and
    /// records the byte range so `submit_composer` can rewrite it to
    /// `<@USER_ID>` later. Returns `false` when the picker has no candidate
    /// to apply.
    pub fn confirm_composer_mention(&mut self) -> bool {
        let Some(_query) = self.composer.composer_mention_query.clone() else {
            return false;
        };
        let Some(mention_start) = self.composer.composer_mention_start else {
            return false;
        };
        let candidates = self.composer_mention_candidates();
        let Some(entry) = candidates.get(self.composer.composer_mention_selected) else {
            return false;
        };
        let entry = entry.clone();

        let cursor = self.composer.composer_cursor_byte_index();
        if mention_start > cursor {
            return false;
        }

        let replacement = format!("@{} ", entry.display_name);
        self.replace_composer_range(mention_start..cursor, &replacement);
        let end = mention_start + '@'.len_utf8() + entry.display_name.len();

        self.composer
            .composer_mention_completions
            .push(MentionCompletion {
                byte_start: mention_start,
                byte_end: end,
                user_id: entry.user_id,
            });
        self.close_composer_mention_query();
        true
    }

    /// Confirms the highlighted emoji. Unicode emoji are inserted directly.
    /// available custom emoji keep their readable `:name:` form and record a
    /// byte range so submit can send Discord's wire markup. Unavailable custom
    /// emoji stay visible in the picker as a hint, but cannot be confirmed.
    pub fn confirm_composer_emoji(&mut self) -> bool {
        let Some(_query) = self.composer.composer_emoji_query.clone() else {
            return false;
        };
        let Some(emoji_start) = self.composer.composer_emoji_start else {
            return false;
        };
        let Some(entry) = self
            .composer
            .composer_emoji_candidates
            .get(self.composer.composer_emoji_selected)
        else {
            return false;
        };
        let entry = entry.clone();
        if !entry.available {
            return false;
        }

        let cursor = self.composer.composer_cursor_byte_index();
        if emoji_start > cursor {
            return false;
        }

        let replacement = if entry.wire_format.is_some() {
            format!(":{}: ", entry.shortcode)
        } else {
            format!("{} ", entry.emoji)
        };
        self.replace_composer_range(emoji_start..cursor, &replacement);
        if let Some(wire_format) = entry.wire_format {
            let end = emoji_start + ':'.len_utf8() + entry.shortcode.len() + ':'.len_utf8();
            self.composer
                .composer_emoji_completions
                .push(EmojiCompletion {
                    byte_start: emoji_start,
                    byte_end: end,
                    replacement: wire_format,
                    custom_image_url: entry.custom_image_url,
                });
        }
        self.close_composer_emoji_query();
        true
    }

    pub fn confirm_composer_command(&mut self) -> bool {
        let Some(command_start) = self.composer.composer_command_start else {
            return false;
        };
        let Some(entry) = self
            .composer
            .composer_command_candidates
            .get(self.composer.composer_command_selected)
            .cloned()
        else {
            return false;
        };
        let cursor = self.composer_cursor_byte_index();
        if command_start > cursor {
            return false;
        }

        self.replace_composer_range(command_start..cursor, &entry.replacement);
        self.close_composer_command_query();
        self.refresh_active_mention_query();
        true
    }

    pub(in crate::tui) fn composer_emoji_image_completions(
        &self,
    ) -> Vec<ComposerEmojiImageCompletion> {
        self.composer
            .composer_emoji_completions
            .iter()
            .filter(|completion| completion.byte_end <= self.composer.composer_input.len())
            .filter_map(|completion| {
                completion
                    .custom_image_url
                    .as_ref()
                    .map(|url| ComposerEmojiImageCompletion {
                        byte_start: completion.byte_start,
                        byte_end: completion.byte_end,
                        url: url.clone(),
                    })
            })
            .collect()
    }

    /// Closes the picker without inserting anything. The literal `@query`
    /// stays in the composer.
    pub fn cancel_composer_mention(&mut self) {
        self.close_composer_mention_query();
    }

    pub fn cancel_composer_emoji(&mut self) {
        self.close_composer_emoji_query();
    }

    pub fn cancel_composer_command(&mut self) {
        self.close_composer_command_query();
    }

    fn reset_mention_picker_state(&mut self) {
        self.close_composer_mention_query();
        self.close_composer_emoji_query();
        self.close_composer_command_query();
        self.composer.composer_mention_completions.clear();
        self.composer.composer_emoji_completions.clear();
    }

    fn close_composer_mention_query(&mut self) {
        self.composer.composer_mention_query = None;
        self.composer.composer_mention_start = None;
        self.composer.composer_mention_selected = 0;
    }

    fn close_composer_emoji_query(&mut self) {
        self.composer.composer_emoji_query = None;
        self.composer.composer_emoji_start = None;
        self.composer.composer_emoji_selected = 0;
        self.composer.composer_emoji_candidates.clear();
    }

    fn close_composer_command_query(&mut self) {
        self.composer.composer_command_query = None;
        self.composer.composer_command_start = None;
        self.composer.composer_command_selected = 0;
        self.composer.composer_command_candidates.clear();
    }

    pub(super) fn refresh_composer_emoji_candidates_for_current_query(&mut self) {
        let Some(query) = self.composer.composer_emoji_query.clone() else {
            self.composer.composer_emoji_candidates.clear();
            return;
        };

        let candidates = self.emoji_candidates_for_query(&query);
        if candidates.is_empty() {
            self.close_composer_emoji_query();
            return;
        }

        self.composer.composer_emoji_selected = self
            .composer
            .composer_emoji_selected
            .min(candidates.len() - 1);
        self.composer.composer_emoji_candidates = candidates;
    }

    fn replace_composer_range(&mut self, range: Range<usize>, replacement: &str) {
        if range.start > range.end
            || range.end > self.composer.composer_input.len()
            || !self.composer.composer_input.is_char_boundary(range.start)
            || !self.composer.composer_input.is_char_boundary(range.end)
        {
            return;
        }
        self.adjust_mention_completions_for_replace(range.clone(), replacement.len());
        self.adjust_emoji_completions_for_replace(range.clone(), replacement.len());
        self.composer
            .composer_input
            .replace_range(range.clone(), replacement);
        self.composer.composer_cursor_byte_index = range.start + replacement.len();
        self.refresh_active_mention_query();
    }

    pub(super) fn refresh_active_mention_query(&mut self) {
        let cursor = self.composer.composer_cursor_byte_index();
        let mut query_start = cursor;

        while query_start > 0 {
            let previous = previous_char_boundary(&self.composer.composer_input, query_start);
            let value = self.composer.composer_input[previous..query_start]
                .chars()
                .next()
                .expect("character boundary slice contains one character");
            if !is_mention_query_char(value) {
                break;
            }
            query_start = previous;
        }

        if query_start > 0 {
            let mention_start = previous_char_boundary(&self.composer.composer_input, query_start);
            if &self.composer.composer_input[mention_start..query_start] == "@"
                && should_start_completion_query(&self.composer.composer_input[..mention_start])
            {
                self.composer.composer_mention_query =
                    Some(self.composer.composer_input[query_start..cursor].to_owned());
                self.composer.composer_mention_start = Some(mention_start);
                self.composer.composer_mention_selected = 0;
                self.close_composer_emoji_query();
                self.close_composer_command_query();
                return;
            }
        }

        let mut query_start = cursor;

        while query_start > 0 {
            let previous = previous_char_boundary(&self.composer.composer_input, query_start);
            let value = self.composer.composer_input[previous..query_start]
                .chars()
                .next()
                .expect("character boundary slice contains one character");
            if !is_emoji_query_char(value) {
                break;
            }
            query_start = previous;
        }

        if query_start > 0 {
            let emoji_start = previous_char_boundary(&self.composer.composer_input, query_start);
            let query = &self.composer.composer_input[query_start..cursor];
            if &self.composer.composer_input[emoji_start..query_start] == ":"
                && query.chars().count() >= 2
                && should_start_completion_query(&self.composer.composer_input[..emoji_start])
            {
                let candidates = self.emoji_candidates_for_query(query);
                if candidates.is_empty() {
                    self.close_composer_mention_query();
                    self.close_composer_emoji_query();
                    return;
                }
                self.composer.composer_emoji_query = Some(query.to_owned());
                self.composer.composer_emoji_start = Some(emoji_start);
                self.composer.composer_emoji_selected = 0;
                self.composer.composer_emoji_candidates = candidates;
                self.close_composer_mention_query();
                self.close_composer_command_query();
                return;
            }
        }

        if let Some((query_start, candidates)) = self.command_completion_at_cursor() {
            if candidates.is_empty() {
                self.close_composer_command_query();
            } else {
                self.composer.composer_command_query =
                    Some(self.composer.composer_input[query_start..cursor].to_owned());
                self.composer.composer_command_start = Some(query_start);
                self.composer.composer_command_selected = self
                    .composer
                    .composer_command_selected
                    .min(candidates.len() - 1);
                self.composer.composer_command_candidates = candidates;
                self.close_composer_mention_query();
                self.close_composer_emoji_query();
                return;
            }
        }

        self.close_composer_mention_query();
        self.close_composer_emoji_query();
        self.close_composer_command_query();
    }

    fn queue_application_commands_for_selected_channel(&mut self) {
        let guild_id = self
            .selected_channel_state()
            .and_then(|channel| channel.guild_id);
        if self.discord.application_commands.contains_key(&guild_id)
            || self
                .discord
                .application_command_requests
                .contains(&guild_id)
        {
            return;
        }
        self.discord.application_command_requests.insert(guild_id);
        self.requests
            .pending_commands
            .push_back(AppCommand::LoadApplicationCommands { guild_id });
    }

    fn command_completion_at_cursor(&mut self) -> Option<(usize, Vec<CommandPickerEntry>)> {
        if self.composer.composer_input.is_empty() || !self.composer.composer_input.starts_with('/')
        {
            return None;
        }
        self.queue_application_commands_for_selected_channel();

        let cursor = self.composer.composer_cursor_byte_index();
        if cursor == 0 || cursor > self.composer.composer_input.len() {
            return None;
        }
        let before_cursor = &self.composer.composer_input[..cursor];
        let token_start = before_cursor
            .rfind(char::is_whitespace)
            .map(|index| index + before_cursor[index..].chars().next().unwrap().len_utf8())
            .unwrap_or(0);
        let token = &self.composer.composer_input[token_start..cursor];
        let commands = self.application_commands_for_selected_channel();

        if token_start == 0 {
            let query = token.strip_prefix('/')?;
            if query.chars().all(is_command_query_char) {
                return Some((0, build_command_candidates(query, commands)));
            }
            return None;
        }

        let command = self.application_command_for_input()?;
        let option_scope = application_command_option_scope(command, before_cursor)?;
        if let Some((option_name, value_query)) = token.split_once(':') {
            let option = option_scope
                .iter()
                .find(|option| option.name == option_name)?;
            if !option.choices.is_empty() {
                return Some((
                    token_start + option_name.len() + ':'.len_utf8(),
                    build_command_choice_candidates(value_query, option),
                ));
            }
            return None;
        }

        if token.chars().all(is_command_query_char) {
            let used = parsed_application_command_option_names(
                &self.composer.composer_input,
                command,
                option_scope,
            );
            let options = option_scope
                .iter()
                .filter(|option| !used.contains(&option.name))
                .cloned()
                .collect::<Vec<_>>();
            return Some((
                token_start,
                build_command_option_candidates(token, &options),
            ));
        }

        None
    }

    fn application_commands_for_selected_channel(&self) -> &[ApplicationCommandInfo] {
        let guild_id = self
            .selected_channel_state()
            .and_then(|channel| channel.guild_id);
        self.discord
            .application_commands
            .get(&guild_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn application_command_for_input(&self) -> Option<&ApplicationCommandInfo> {
        let name = self
            .composer
            .composer_input
            .strip_prefix('/')?
            .split_whitespace()
            .next()?;
        self.application_commands_for_selected_channel()
            .iter()
            .find(|command| command.name == name)
    }

    fn application_command_submit_for_content(&self, content: &str) -> ApplicationCommandSubmit {
        let Some(channel_id) = self.selected_channel_id() else {
            return ApplicationCommandSubmit::Incomplete;
        };
        let guild_id = self
            .selected_channel_state()
            .and_then(|channel| channel.guild_id);
        let Some(command_name) = content
            .strip_prefix('/')
            .and_then(|rest| rest.split_whitespace().next())
        else {
            return ApplicationCommandSubmit::NotCommand;
        };
        let Some(command) = self
            .discord
            .application_commands
            .get(&guild_id)
            .and_then(|commands| commands.iter().find(|command| command.name == command_name))
            .cloned()
        else {
            return ApplicationCommandSubmit::NotCommand;
        };
        let Some(session_id) = self.discord.gateway_session_id.clone() else {
            return ApplicationCommandSubmit::Incomplete;
        };
        let Some(options) = parsed_application_command_options(content, &command) else {
            return ApplicationCommandSubmit::Incomplete;
        };
        ApplicationCommandSubmit::Ready(ApplicationCommandInteraction {
            guild_id,
            channel_id,
            session_id,
            command,
            options,
        })
    }

    fn adjust_mention_completions_for_replace(
        &mut self,
        replaced: Range<usize>,
        replacement_len: usize,
    ) {
        let replaced_len = replaced.end - replaced.start;
        let delta = replacement_len as isize - replaced_len as isize;
        let mut completions = Vec::with_capacity(self.composer.composer_mention_completions.len());

        for mut completion in self.composer.composer_mention_completions.drain(..) {
            if completion.byte_end <= replaced.start {
                completions.push(completion);
            } else if completion.byte_start >= replaced.end {
                completion.byte_start = shift_byte_index(completion.byte_start, delta);
                completion.byte_end = shift_byte_index(completion.byte_end, delta);
                completions.push(completion);
            }
        }

        self.composer.composer_mention_completions = completions;
    }

    fn adjust_emoji_completions_for_replace(
        &mut self,
        replaced: Range<usize>,
        replacement_len: usize,
    ) {
        let replaced_len = replaced.end - replaced.start;
        let delta = replacement_len as isize - replaced_len as isize;
        let mut completions = Vec::with_capacity(self.composer.composer_emoji_completions.len());

        for mut completion in self.composer.composer_emoji_completions.drain(..) {
            if completion.byte_end <= replaced.start {
                completions.push(completion);
            } else if completion.byte_start >= replaced.end {
                completion.byte_start = shift_byte_index(completion.byte_start, delta);
                completion.byte_end = shift_byte_index(completion.byte_end, delta);
                completions.push(completion);
            }
        }

        self.composer.composer_emoji_completions = completions;
    }

    fn emoji_candidates_for_query(&self, query: &str) -> Vec<EmojiPickerEntry> {
        let custom_emojis = self
            .selected_channel_guild_id()
            .map(|guild_id| self.discord.cache.custom_emojis_for_guild(guild_id))
            .unwrap_or_default();
        build_emoji_candidates(
            query,
            custom_emojis,
            self.discord
                .current_user_can_use_animated_custom_emojis
                .unwrap_or(false),
        )
    }
}

fn clamp_cursor_index(input: &str, index: usize) -> usize {
    let mut index = index.min(input.len());
    while index > 0 && !input.is_char_boundary(index) {
        index -= 1;
    }
    index
}

enum ApplicationCommandSubmit {
    Ready(ApplicationCommandInteraction),
    Incomplete,
    NotCommand,
}

const APPLICATION_COMMAND_SUBCOMMAND_KIND: u64 = 1;
const APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND: u64 = 2;
const APPLICATION_COMMAND_INTEGER_KIND: u64 = 4;
const APPLICATION_COMMAND_BOOLEAN_KIND: u64 = 5;
const APPLICATION_COMMAND_USER_KIND: u64 = 6;
const APPLICATION_COMMAND_CHANNEL_KIND: u64 = 7;
const APPLICATION_COMMAND_ROLE_KIND: u64 = 8;
const APPLICATION_COMMAND_MENTIONABLE_KIND: u64 = 9;
const APPLICATION_COMMAND_NUMBER_KIND: u64 = 10;

fn parsed_application_command_options(
    content: &str,
    command: &ApplicationCommandInfo,
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let Some(rest) = content.strip_prefix('/') else {
        return None;
    };
    let mut parts = rest.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return None;
    }

    let parts = parts.collect::<Vec<_>>();
    parsed_application_command_options_from_parts(&parts, command)
}

fn parsed_application_command_options_from_parts(
    parts: &[&str],
    command: &ApplicationCommandInfo,
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let has_structural_options = command.options.iter().any(is_structural_command_option);

    if let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) {
        if let Some(group) = command.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
        }) {
            let subcommand_name = parts.get(1).copied().filter(|part| !part.contains(':'))?;
            let subcommand = group.options.iter().find(|option| {
                option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == subcommand_name
            })?;
            let options = parse_leaf_application_command_options(&parts[2..], &subcommand.options)?;
            return Some(vec![structural_interaction_option(
                group,
                vec![structural_interaction_option(subcommand, options)],
            )]);
        }

        if let Some(subcommand) = command.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == first
        }) {
            let options = parse_leaf_application_command_options(&parts[1..], &subcommand.options)?;
            return Some(vec![structural_interaction_option(subcommand, options)]);
        }
    }

    if has_structural_options {
        return None;
    }

    parse_leaf_application_command_options(parts, &command.options)
}

fn structural_interaction_option(
    option: &ApplicationCommandOptionInfo,
    options: Vec<ApplicationCommandInteractionOption>,
) -> ApplicationCommandInteractionOption {
    ApplicationCommandInteractionOption {
        kind: option.kind,
        name: option.name.clone(),
        value: None,
        options,
    }
}

fn parse_leaf_application_command_options(
    parts: &[&str],
    options: &[ApplicationCommandOptionInfo],
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let mut parsed = Vec::new();
    let mut current: Option<(&ApplicationCommandOptionInfo, String)> = None;

    for part in parts {
        if let Some((name, raw_value)) = part.split_once(':')
            && let Some(option) = options
                .iter()
                .find(|option| !is_structural_command_option(option) && option.name == name)
        {
            push_leaf_application_command_option(&mut parsed, current.take());
            current = Some((option, raw_value.to_owned()));
            continue;
        }

        if let Some((_, value)) = current.as_mut() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(part);
        } else if options
            .iter()
            .any(|option| !is_structural_command_option(option))
        {
            return None;
        }
    }

    push_leaf_application_command_option(&mut parsed, current.take());
    required_application_command_options_present(options, &parsed).then_some(parsed)
}

fn push_leaf_application_command_option(
    parsed: &mut Vec<ApplicationCommandInteractionOption>,
    current: Option<(&ApplicationCommandOptionInfo, String)>,
) {
    let Some((option, value)) = current else {
        return;
    };
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    parsed.push(ApplicationCommandInteractionOption {
        kind: option.kind,
        name: option.name.clone(),
        value: Some(application_command_option_value(option, value)),
        options: Vec::new(),
    });
}

fn required_application_command_options_present(
    options: &[ApplicationCommandOptionInfo],
    parsed: &[ApplicationCommandInteractionOption],
) -> bool {
    options
        .iter()
        .filter(|option| option.required && !is_structural_command_option(option))
        .all(|option| parsed.iter().any(|parsed| parsed.name == option.name))
}

fn parsed_application_command_option_names(
    content: &str,
    command: &ApplicationCommandInfo,
    options: &[ApplicationCommandOptionInfo],
) -> std::collections::HashSet<String> {
    let Some(rest) = content.strip_prefix('/') else {
        return std::collections::HashSet::new();
    };
    let mut parts = rest.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return std::collections::HashSet::new();
    }
    let all_parts = parts.collect::<Vec<_>>();
    let leaf_parts = leaf_application_command_parts(&all_parts, command, options);
    parse_leaf_application_command_options(leaf_parts, options)
        .unwrap_or_default()
        .into_iter()
        .map(|option| option.name)
        .collect()
}

fn leaf_application_command_parts<'a>(
    parts: &'a [&'a str],
    command: &ApplicationCommandInfo,
    options: &[ApplicationCommandOptionInfo],
) -> &'a [&'a str] {
    if std::ptr::eq(options, command.options.as_slice()) {
        return parts;
    }
    let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) else {
        return parts;
    };
    if let Some(group) = command.options.iter().find(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
    }) {
        if std::ptr::eq(options, group.options.as_slice()) {
            return &parts[1..];
        }
        let Some(second) = parts.get(1).copied().filter(|part| !part.contains(':')) else {
            return parts;
        };
        if group.options.iter().any(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND
                && option.name == second
                && std::ptr::eq(options, option.options.as_slice())
        }) {
            return &parts[2..];
        }
        return parts;
    }
    if command.options.iter().any(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND
            && option.name == first
            && std::ptr::eq(options, option.options.as_slice())
    }) {
        return &parts[1..];
    }
    parts
}

fn application_command_option_scope<'a>(
    command: &'a ApplicationCommandInfo,
    before_cursor: &str,
) -> Option<&'a [ApplicationCommandOptionInfo]> {
    let mut parts = before_cursor.strip_prefix('/')?.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return None;
    }
    let parts = parts.collect::<Vec<_>>();
    let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) else {
        return Some(&command.options);
    };

    if let Some(group) = command.options.iter().find(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
    }) {
        let Some(second) = parts.get(1).copied().filter(|part| !part.contains(':')) else {
            return Some(&group.options);
        };
        if let Some(subcommand) = group.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == second
        }) {
            return Some(&subcommand.options);
        }
        return Some(&group.options);
    }

    if let Some(subcommand) = command
        .options
        .iter()
        .find(|option| option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == first)
    {
        return Some(&subcommand.options);
    }

    Some(&command.options)
}

fn is_structural_command_option(option: &ApplicationCommandOptionInfo) -> bool {
    matches!(
        option.kind,
        APPLICATION_COMMAND_SUBCOMMAND_KIND | APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND
    )
}

fn application_command_option_value(option: &ApplicationCommandOptionInfo, raw: &str) -> Value {
    match option.kind {
        APPLICATION_COMMAND_INTEGER_KIND => raw
            .parse::<i64>()
            .map(Number::from)
            .map(Value::Number)
            .unwrap_or_else(|_| Value::String(raw.to_owned())),
        APPLICATION_COMMAND_BOOLEAN_KIND => Value::Bool(matches!(raw, "true" | "yes" | "1" | "on")),
        APPLICATION_COMMAND_NUMBER_KIND => raw
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_owned())),
        APPLICATION_COMMAND_USER_KIND
        | APPLICATION_COMMAND_CHANNEL_KIND
        | APPLICATION_COMMAND_ROLE_KIND
        | APPLICATION_COMMAND_MENTIONABLE_KIND => Value::String(snowflake_option_value(raw)),
        _ => Value::String(raw.to_owned()),
    }
}

fn snowflake_option_value(raw: &str) -> String {
    raw.trim_matches(|value| matches!(value, '<' | '>' | '@' | '!' | '#' | '&'))
        .to_owned()
}

fn previous_char_boundary(input: &str, index: usize) -> usize {
    let index = clamp_cursor_index(input, index);
    if index == 0 {
        return 0;
    }
    let mut previous = index - 1;
    while previous > 0 && !input.is_char_boundary(previous) {
        previous -= 1;
    }
    previous
}

fn next_char_boundary(input: &str, index: usize) -> usize {
    let mut next = clamp_cursor_index(input, index).saturating_add(1);
    while next < input.len() && !input.is_char_boundary(next) {
        next += 1;
    }
    next.min(input.len())
}

fn vertical_cursor_target(input: &str, cursor: usize, direction: isize) -> Option<usize> {
    let cursor = clamp_cursor_index(input, cursor);
    let line_start = line_start_before(input, cursor);
    let line_end = line_end_after(input, cursor);
    let column = input[line_start..cursor].chars().count();

    match direction {
        -1 => {
            if line_start == 0 {
                return None;
            }
            let target_end = line_start - 1;
            let target_start = line_start_before(input, target_end);
            Some(byte_index_for_line_column(
                input,
                target_start,
                target_end,
                column,
            ))
        }
        1 => {
            let next_start = line_end.checked_add(1)?;
            if next_start > input.len() {
                return None;
            }
            let target_end = line_end_after(input, next_start);
            Some(byte_index_for_line_column(
                input, next_start, target_end, column,
            ))
        }
        _ => None,
    }
}

fn line_start_before(input: &str, index: usize) -> usize {
    input[..index]
        .rfind('\n')
        .map(|offset| offset + '\n'.len_utf8())
        .unwrap_or(0)
}

fn line_end_after(input: &str, index: usize) -> usize {
    input[index..]
        .find('\n')
        .map(|offset| index + offset)
        .unwrap_or(input.len())
}

fn byte_index_for_line_column(input: &str, start: usize, end: usize, column: usize) -> usize {
    input[start..end]
        .char_indices()
        .nth(column)
        .map(|(offset, _)| start + offset)
        .unwrap_or(end)
}

fn previous_word_boundary(input: &str, index: usize) -> usize {
    let index = clamp_cursor_index(input, index);
    let mut prefix = input[..index].char_indices().rev().peekable();
    while matches!(prefix.peek(), Some((_, c)) if c.is_whitespace()) {
        prefix.next();
    }
    let mut word_start = None;
    while let Some(&(byte_idx, c)) = prefix.peek() {
        if c.is_whitespace() {
            break;
        }
        word_start = Some(byte_idx);
        prefix.next();
    }
    word_start.unwrap_or(0)
}

fn next_word_boundary(input: &str, index: usize) -> usize {
    let index = clamp_cursor_index(input, index);
    let mut suffix = input[index..].char_indices().peekable();
    while matches!(suffix.peek(), Some((_, c)) if !c.is_whitespace()) {
        suffix.next();
    }
    while matches!(suffix.peek(), Some((_, c)) if c.is_whitespace()) {
        suffix.next();
    }
    match suffix.peek() {
        Some(&(rel, _)) => index + rel,
        None => input.len(),
    }
}

fn shift_byte_index(index: usize, delta: isize) -> usize {
    if delta < 0 {
        index.saturating_sub(delta.unsigned_abs())
    } else {
        index.saturating_add(delta as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::{next_word_boundary, previous_word_boundary};

    #[derive(Clone, Copy)]
    enum Dir {
        Left,
        Right,
    }

    fn step(dir: Dir, before: &str) -> String {
        let idx = before
            .find('|')
            .expect("fixture must mark the cursor with `|`");
        let mut input = String::with_capacity(before.len() - 1);
        input.push_str(&before[..idx]);
        input.push_str(&before[idx + 1..]);
        let next = match dir {
            Dir::Left => previous_word_boundary(&input, idx),
            Dir::Right => next_word_boundary(&input, idx),
        };
        let mut out = input.clone();
        out.insert(next, '|');
        out
    }

    #[test]
    fn word_skip_lands_on_word_starts() {
        let cases: &[(Dir, &str, &str)] = &[
            (Dir::Left, "hello world|", "hello |world"),
            (Dir::Left, "hello |world", "|hello world"),
            (Dir::Right, "|hello world", "hello |world"),
            (Dir::Right, "hello |world", "hello world|"),
            (Dir::Left, "   foo|", "   |foo"),
            (Dir::Left, "|hello", "|hello"),
            (Dir::Left, "   |", "|   "),
            (Dir::Right, "hello|", "hello|"),
            (Dir::Right, "|   ", "   |"),
            (Dir::Right, "hello|   world", "hello   |world"),
            (Dir::Right, "hello   |world", "hello   world|"),
            (Dir::Left, "안녕 하세요|", "안녕 |하세요"),
            (Dir::Left, "안녕 |하세요", "|안녕 하세요"),
            (Dir::Right, "|안녕 하세요", "안녕 |하세요"),
            (Dir::Right, "안녕 |하세요", "안녕 하세요|"),
            (Dir::Right, "|a 🦀 b", "a |🦀 b"),
            (Dir::Right, "a |🦀 b", "a 🦀 |b"),
            (Dir::Left, "a 🦀 b|", "a 🦀 |b"),
            (Dir::Left, "a 🦀 |b", "a |🦀 b"),
            (Dir::Left, "|", "|"),
            (Dir::Right, "|", "|"),
        ];

        for (dir, before, expected) in cases {
            let arrow = match dir {
                Dir::Left => "Ctrl+Left",
                Dir::Right => "Ctrl+Right",
            };
            assert_eq!(
                step(*dir, before),
                *expected,
                "{arrow} on {before:?} should land at {expected:?}",
            );
        }
    }
}
