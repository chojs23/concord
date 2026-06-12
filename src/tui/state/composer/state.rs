use std::ops::Range;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::discord::{
    APPLICATION_COMMAND_CHANNEL_KIND, APPLICATION_COMMAND_MENTIONABLE_KIND,
    APPLICATION_COMMAND_ROLE_KIND, APPLICATION_COMMAND_USER_KIND, ApplicationCommandIdentity,
    ApplicationCommandInfo, ApplicationCommandInvocation, BuiltinSlashCommandParse,
    BuiltinSlashCommandSubmit, GlobalUserProfileUpdate, GuildUserProfileUpdate,
    MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload, UserProfileUpdate,
    application_command_content_is_complete, application_command_option_scope,
    parse_builtin_slash_command, parsed_application_command_option_names,
};

use super::super::{
    ActiveModalPopupKind, CommandPickerEntry, DashboardState, EmojiPickerEntry, FocusPane,
    MentionPickerEntry,
};
use super::completions::{
    ComposerEmojiImageCompletion, EmojiCompletion, MentionCompletion,
    build_builtin_command_candidates, build_channel_mention_candidates, build_command_candidates,
    build_command_choice_candidates, build_command_option_candidates, build_emoji_candidates,
    build_mention_candidates, expand_composer_completions, expand_emoji_shortcodes,
    is_command_query_char, is_emoji_query_char, is_mention_query_char, move_picker_selection,
    should_start_completion_query,
};
use crate::discord::AppCommand;
use crate::tui::text_cursor::{
    clamp_cursor_index, next_char_boundary, next_word_boundary, previous_char_boundary,
    previous_word_boundary,
};

#[derive(Debug, Default)]
pub(in crate::tui::state) struct ComposerUiState {
    pub(in crate::tui::state) composer_input: String,
    pub(in crate::tui::state) composer_cursor_byte_index: usize,
    pub(in crate::tui::state) pending_composer_attachments: Vec<MessageAttachmentUpload>,
    pub(in crate::tui::state) composer_active: bool,
    pub(in crate::tui::state) reply_target_message_id: Option<Id<MessageMarker>>,
    pub(in crate::tui::state) edit_target_message: Option<(Id<ChannelMarker>, Id<MessageMarker>)>,
    /// Set when the user is in the middle of an `@mention` autocomplete. The
    /// stored string is the characters typed *after* the `@` and is used to
    /// filter the candidate list. `None` means the picker is closed.
    pub(in crate::tui::state) composer_mention_query: Option<String>,
    pub(in crate::tui::state) composer_mention_start: Option<usize>,
    pub(in crate::tui::state) composer_mention_selected: usize,
    /// Set when the user is typing a Unicode emoji shortcode after `:`. The
    /// picker opens after two shortcode characters, mirroring Discord's
    /// threshold while avoiding noisy popups for ordinary punctuation.
    pub(in crate::tui::state) composer_emoji_query: Option<String>,
    pub(in crate::tui::state) composer_emoji_start: Option<usize>,
    pub(in crate::tui::state) composer_emoji_selected: usize,
    pub(in crate::tui::state) composer_emoji_candidates: Vec<EmojiPickerEntry>,
    pub(in crate::tui::state) composer_command_query: Option<String>,
    pub(in crate::tui::state) composer_command_start: Option<usize>,
    pub(in crate::tui::state) composer_command_selected: usize,
    pub(in crate::tui::state) composer_command_candidates: Vec<CommandPickerEntry>,
    pub(in crate::tui::state) composer_selected_command_identity:
        Option<ApplicationCommandIdentity>,
    /// Records `@displayname` substrings that the picker inserted, so the
    /// composer can rewrite them to Discord's `<@USER_ID>` wire format on
    /// submit even though the visible text is still the friendly form.
    pub(in crate::tui::state) composer_mention_completions: Vec<MentionCompletion>,
    /// Recorded custom emoji ranges inserted by the picker. The editor keeps
    /// the readable `:name:` text while submit rewrites these ranges to
    /// Discord's `<:name:id>` or `<a:name:id>` wire format.
    pub(in crate::tui::state) composer_emoji_completions: Vec<EmojiCompletion>,
}

impl ComposerUiState {
    fn composer_cursor_byte_index(&self) -> usize {
        let mut index = self
            .composer_cursor_byte_index
            .min(self.composer_input.len());
        while index > 0 && !self.composer_input.is_char_boundary(index) {
            index -= 1;
        }
        index
    }
}

impl DashboardState {
    pub fn is_composing(&self) -> bool {
        self.composer.composer_active
    }

    pub(in crate::tui::state) fn start_reply_composer(&mut self) {
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

    pub(in crate::tui::state) fn start_edit_composer(&mut self) {
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

    fn can_send_tts_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) if channel.is_forum() => false,
            Some(channel) => self.discord.cache.can_send_tts_in_channel(channel),
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

    pub fn open_composer_reaction_picker_from_plus_colon(&mut self) -> bool {
        let cursor = self.composer.composer_cursor_byte_index();
        if !composer_plus_colon_trigger_before_cursor(&self.composer.composer_input, cursor) {
            return false;
        }

        self.open_emoji_reaction_picker();
        if !self.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker) {
            return false;
        }

        let plus_start = cursor - '+'.len_utf8();
        self.replace_composer_range(plus_start..cursor, "");
        true
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
            self.cancel_composer();
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
            self.cancel_composer();
            return None;
        }
        if has_attachments && !self.can_attach_in_selected_channel() {
            self.cancel_composer();
            return None;
        }

        if !has_attachments && self.composer.reply_target_message_id.is_none() {
            match self.builtin_slash_command_submit_for_content(&content, channel_id) {
                BuiltinCommandSubmit::Ready(command) => {
                    self.clear_submitted_composer_text();
                    self.composer.reply_target_message_id = None;
                    self.composer.pending_composer_attachments.clear();
                    return Some(command);
                }
                BuiltinCommandSubmit::Incomplete => return None,
                BuiltinCommandSubmit::Error(message) => {
                    self.show_error_toast(message, std::time::Instant::now());
                    return None;
                }
                BuiltinCommandSubmit::NotCommand => {}
            }
            match self.application_command_submit_for_content(&content) {
                ApplicationCommandSubmit::Ready(interaction) => {
                    self.clear_submitted_composer_text();
                    self.composer.reply_target_message_id = None;
                    self.composer.pending_composer_attachments.clear();
                    return Some(AppCommand::RunApplicationCommand {
                        invocation: interaction,
                    });
                }
                ApplicationCommandSubmit::Incomplete => return None,
                ApplicationCommandSubmit::NotCommand => {}
            }
        }

        self.clear_submitted_composer_text();
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

    fn clear_submitted_composer_text(&mut self) {
        self.composer.composer_input.clear();
        self.composer.composer_cursor_byte_index = 0;
        self.reset_mention_picker_state();
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
        if self.active_composer_mention_trigger() == Some('#') {
            build_channel_mention_candidates(query, self.composer_channel_candidates())
        } else {
            build_mention_candidates(
                query,
                self.flattened_members(),
                self.composer_role_candidates(),
            )
        }
    }

    fn active_composer_mention_trigger(&self) -> Option<char> {
        let start = self.composer.composer_mention_start?;
        self.composer.composer_input[start..]
            .chars()
            .next()
            .filter(|value| matches!(value, '@' | '#'))
    }

    fn selected_composer_guild_id(&self) -> Option<Id<crate::discord::ids::marker::GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
            .or_else(|| self.selected_guild_id())
    }

    fn composer_role_candidates(&self) -> Vec<&crate::discord::RoleState> {
        self.selected_composer_guild_id()
            .map(|guild_id| self.discord.cache.roles_for_guild(guild_id))
            .unwrap_or_default()
    }

    fn composer_channel_candidates(&self) -> Vec<&crate::discord::ChannelState> {
        self.discord
            .cache
            .viewable_channels_for_guild(self.selected_composer_guild_id())
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

    pub(in crate::tui) fn composer_command_selected_candidate_is_top_level(&self) -> bool {
        self.composer
            .composer_command_candidates
            .get(self.composer.composer_command_selected)
            .is_some_and(|entry| entry.top_level)
    }

    pub(in crate::tui) fn composer_command_can_submit(&self) -> bool {
        let expanded = expand_composer_completions(
            &self.composer.composer_input,
            &self.composer.composer_mention_completions,
            &self.composer.composer_emoji_completions,
        );
        let expanded = expand_emoji_shortcodes(&expanded);
        matches!(
            self.application_command_submit_for_content(expanded.trim()),
            ApplicationCommandSubmit::Ready(_)
        )
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

        let replacement = format!("{} ", entry.visible_text());
        self.replace_composer_range(mention_start..cursor, &replacement);
        let end = mention_start + entry.visible_text().len();

        self.composer
            .composer_mention_completions
            .push(MentionCompletion {
                byte_start: mention_start,
                byte_end: end,
                target: entry.target,
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
        if entry.top_level {
            self.composer.composer_selected_command_identity = entry.command_identity;
        }
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
        self.composer.composer_selected_command_identity = None;
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

    pub(in crate::tui::state) fn refresh_composer_emoji_candidates_for_current_query(&mut self) {
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

    pub(in crate::tui::state) fn refresh_active_mention_query(&mut self) {
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
            let trigger = &self.composer.composer_input[mention_start..query_start];
            if matches!(trigger, "@" | "#")
                && self.should_start_composer_mention_query(mention_start, trigger)
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

    fn should_start_composer_mention_query(&self, mention_start: usize, trigger: &str) -> bool {
        should_start_completion_query(&self.composer.composer_input[..mention_start])
            || self.command_option_accepts_mention_trigger(mention_start, trigger)
    }

    fn command_option_accepts_mention_trigger(&self, mention_start: usize, trigger: &str) -> bool {
        let before_marker = &self.composer.composer_input[..mention_start];
        let token_start = before_marker
            .rfind(char::is_whitespace)
            .map(|index| index + before_marker[index..].chars().next().unwrap().len_utf8())
            .unwrap_or(0);
        let token = &before_marker[token_start..];
        let Some((option_name, _)) = token.split_once(':') else {
            return false;
        };
        let Some(command) = self.application_command_for_input() else {
            return false;
        };
        let Some(option_scope) = application_command_option_scope(command, before_marker) else {
            return false;
        };
        let Some(option) = option_scope
            .iter()
            .find(|option| option.name == option_name)
        else {
            return false;
        };
        match trigger {
            "@" => matches!(
                option.kind,
                APPLICATION_COMMAND_USER_KIND
                    | APPLICATION_COMMAND_ROLE_KIND
                    | APPLICATION_COMMAND_MENTIONABLE_KIND
            ),
            "#" => option.kind == APPLICATION_COMMAND_CHANNEL_KIND,
            _ => false,
        }
    }

    fn queue_application_commands_for_selected_channel(&mut self) {
        let guild_id = self
            .selected_channel_state()
            .and_then(|channel| channel.guild_id);
        if self.discord.application_commands.contains_key(&guild_id) {
            return;
        }
        self.queue_application_command_load(guild_id);
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
                let mut candidates = build_builtin_command_candidates(query);
                candidates.extend(build_command_candidates(query, commands));
                return Some((0, candidates));
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
            let candidates = self.command_option_value_candidates(value_query, option);
            if !candidates.is_empty() {
                return Some((token_start + option_name.len() + ':'.len_utf8(), candidates));
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

    fn command_option_value_candidates(
        &self,
        value_query: &str,
        option: &crate::discord::ApplicationCommandOptionInfo,
    ) -> Vec<CommandPickerEntry> {
        let query = value_query.trim_start_matches(['@', '#']);
        let mention_candidates = match option.kind {
            APPLICATION_COMMAND_USER_KIND => {
                build_mention_candidates(query, self.flattened_members(), Vec::new())
            }
            APPLICATION_COMMAND_ROLE_KIND => {
                build_mention_candidates(query, Vec::new(), self.composer_role_candidates())
            }
            APPLICATION_COMMAND_CHANNEL_KIND => {
                build_channel_mention_candidates(query, self.composer_channel_candidates())
            }
            APPLICATION_COMMAND_MENTIONABLE_KIND => build_mention_candidates(
                query,
                self.flattened_members(),
                self.composer_role_candidates(),
            ),
            _ => return Vec::new(),
        };

        mention_candidates
            .into_iter()
            .map(|entry| CommandPickerEntry {
                label: entry.visible_text(),
                detail: match entry.target {
                    super::completions::MentionPickerTarget::User(_) => entry
                        .username
                        .map(|username| format!("user @{username}"))
                        .unwrap_or_else(|| "user".to_owned()),
                    super::completions::MentionPickerTarget::Role(_) => "role".to_owned(),
                    super::completions::MentionPickerTarget::Channel(_) => "channel".to_owned(),
                },
                replacement: format!("{} ", entry.target.wire_format()),
                top_level: false,
                command_identity: None,
            })
            .collect()
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
        if let Some(identity) = self.composer.composer_selected_command_identity
            && let Some(command) = self
                .application_commands_for_selected_channel()
                .iter()
                .find(|command| command.identity() == identity && command.name == name)
        {
            return Some(command);
        }
        self.application_commands_for_selected_channel()
            .iter()
            .find(|command| command.name == name)
    }

    fn builtin_slash_command_submit_for_content(
        &self,
        content: &str,
        channel_id: Id<ChannelMarker>,
    ) -> BuiltinCommandSubmit {
        match parse_builtin_slash_command(content) {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                content,
                tts,
            }) => {
                if tts {
                    if !self.can_send_tts_in_selected_channel() {
                        return BuiltinCommandSubmit::Error(
                            "Cannot send text-to-speech messages in this channel".to_owned(),
                        );
                    }
                    BuiltinCommandSubmit::Ready(AppCommand::SendTtsMessage {
                        channel_id,
                        content,
                    })
                } else {
                    BuiltinCommandSubmit::Ready(AppCommand::SendMessage {
                        channel_id,
                        content,
                        reply_to: None,
                        attachments: Vec::new(),
                    })
                }
            }
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Nickname { nickname }) => {
                let Some(user_id) = self.current_user_id() else {
                    return BuiltinCommandSubmit::Error(
                        "Cannot change nickname before the current user is loaded".to_owned(),
                    );
                };
                let Some(guild_id) = self
                    .selected_channel_state()
                    .and_then(|channel| channel.guild_id)
                else {
                    return BuiltinCommandSubmit::Error(
                        "/nick can only be used in a server channel".to_owned(),
                    );
                };

                BuiltinCommandSubmit::Ready(AppCommand::UpdateUserProfile {
                    update: UserProfileUpdate {
                        user_id,
                        guild_id: Some(guild_id),
                        global: GlobalUserProfileUpdate::default(),
                        guild: Some(GuildUserProfileUpdate {
                            guild_id,
                            nickname: Some(nickname),
                            pronouns: None,
                        }),
                    },
                })
            }
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Unsupported { message }) => {
                BuiltinCommandSubmit::Error(message)
            }
            BuiltinSlashCommandParse::Incomplete => BuiltinCommandSubmit::Incomplete,
            BuiltinSlashCommandParse::NotBuiltin => BuiltinCommandSubmit::NotCommand,
        }
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
        let commands = self
            .discord
            .application_commands
            .get(&guild_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let selected_identity = self.composer.composer_selected_command_identity;
        let Some(command) = selected_identity
            .and_then(|identity| {
                commands
                    .iter()
                    .find(|command| command.identity() == identity && command.name == command_name)
            })
            .or_else(|| commands.iter().find(|command| command.name == command_name))
            .cloned()
        else {
            return ApplicationCommandSubmit::NotCommand;
        };
        if !application_command_content_is_complete(content, &command) {
            return ApplicationCommandSubmit::Incomplete;
        }
        ApplicationCommandSubmit::Ready(ApplicationCommandInvocation {
            guild_id,
            channel_id,
            command_identity: Some(command.identity()),
            command_name: command.name,
            content: content.to_owned(),
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
        let selected_guild_channle = self.selected_channel_guild_id();

        let foreign_emojis = self
            .discord
            .cache
            .all_custom_emojis()
            .filter(|(id, _)| {
                selected_guild_channle.is_none_or(|guild_channel| **id != guild_channel)
            })
            .flat_map(|(_, emojis)| emojis);

        let guild_emojis = self
            .selected_channel_guild_id()
            .map(|guild_id| self.discord.cache.custom_emojis_for_guild(guild_id))
            .unwrap_or_default()
            .iter();

        build_emoji_candidates(
            query,
            foreign_emojis,
            guild_emojis,
            self.current_user_has_nitro(),
            self.options.composer_options.emojis_as_links,
        )
    }
}

enum ApplicationCommandSubmit {
    Ready(ApplicationCommandInvocation),
    Incomplete,
    NotCommand,
}

enum BuiltinCommandSubmit {
    Ready(AppCommand),
    Incomplete,
    Error(String),
    NotCommand,
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

fn shift_byte_index(index: usize, delta: isize) -> usize {
    if delta < 0 {
        index.saturating_sub(delta.unsigned_abs())
    } else {
        index.saturating_add(delta as usize)
    }
}

fn composer_plus_colon_trigger_before_cursor(input: &str, cursor: usize) -> bool {
    if cursor == 0 || cursor > input.len() || !input.is_char_boundary(cursor) {
        return false;
    }
    let plus_start = previous_char_boundary(input, cursor);
    if &input[plus_start..cursor] != "+" {
        return false;
    }
    input[..plus_start]
        .chars()
        .last()
        .is_none_or(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::{
        composer_plus_colon_trigger_before_cursor, next_word_boundary, previous_word_boundary,
    };

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

    #[test]
    fn plus_colon_trigger_requires_boundary_plus_before_cursor() {
        let cases = [
            ("+", true),
            ("draft +", true),
            ("draft+", false),
            ("", false),
            ("draft", false),
        ];

        for (input, expected) in cases {
            assert_eq!(
                composer_plus_colon_trigger_before_cursor(input, input.len()),
                expected,
                "{input:?} should return {expected}",
            );
        }
    }
}
