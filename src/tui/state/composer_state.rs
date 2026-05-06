use crate::discord::AppCommand;

use super::composer::{
    MentionCompletion, build_mention_candidates, expand_mention_completions, is_mention_query_char,
    move_mention_selection, should_start_mention_query,
};
use super::{DashboardState, FocusPane, MentionPickerEntry};

impl DashboardState {
    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub(super) fn start_reply_composer(&mut self) {
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

    /// Whether the user can post messages in the currently selected channel.
    /// Returns `true` when no channel is selected so callers don't have to
    /// special-case the empty state.
    pub fn can_send_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) if channel.is_forum() => false,
            Some(channel) => self.discord.can_send_in_channel(channel),
            None => true,
        }
    }

    /// Whether the user can attach files in the currently selected channel.
    /// Wired up so a future attachment picker can disable itself; the
    /// composer doesn't expose attachment input today.
    pub fn can_attach_in_selected_channel(&self) -> bool {
        match self.selected_channel_state() {
            Some(channel) if channel.is_forum() => false,
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
        self.reset_mention_picker_state();
        let reply_to = self.reply_target_message_id.take();
        // Stay in insert mode so the user can send several messages in a
        // row without re-pressing `i`. The composer closes only when the
        // user explicitly bails with Esc or the channel revokes
        // SEND_MESSAGES (handled above).
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
}
