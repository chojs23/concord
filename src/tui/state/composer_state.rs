use std::ops::Range;

use crate::discord::{AppCommand, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload};

use super::composer::{
    ComposerEmojiImageCompletion, EmojiCompletion, MentionCompletion, build_emoji_candidates,
    build_mention_candidates, expand_composer_completions, expand_emoji_shortcodes,
    is_emoji_query_char, is_mention_query_char, move_picker_selection,
    should_start_completion_query,
};
use super::{DashboardState, EmojiPickerEntry, FocusPane, MentionPickerEntry};

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
        self.composer_cursor_byte_index = 0;
        self.pending_composer_attachments.clear();
        self.reply_target_message_id = Some(message_id);
        self.edit_target_message = None;
        self.reset_mention_picker_state();
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub(super) fn start_edit_composer(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if Some(message.author_id) != self.current_user_id || !message.message_kind.is_regular() {
            return;
        }
        let Some(content) = message.content.clone() else {
            return;
        };
        let channel_id = message.channel_id;
        let message_id = message.id;
        self.composer_input = content;
        self.composer_cursor_byte_index = self.composer_input.len();
        self.pending_composer_attachments.clear();
        self.reply_target_message_id = None;
        self.edit_target_message = Some((channel_id, message_id));
        self.reset_mention_picker_state();
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
    }

    pub fn composer_cursor_byte_index(&self) -> usize {
        clamp_cursor_index(&self.composer_input, self.composer_cursor_byte_index)
    }

    pub fn pending_composer_attachments(&self) -> &[MessageAttachmentUpload] {
        &self.pending_composer_attachments
    }

    pub fn add_pending_composer_attachments(&mut self, attachments: Vec<MessageAttachmentUpload>) {
        if attachments.is_empty() || !self.composer_accepts_attachments() {
            return;
        }
        let available =
            MAX_UPLOAD_ATTACHMENT_COUNT.saturating_sub(self.pending_composer_attachments.len());
        self.pending_composer_attachments
            .extend(attachments.into_iter().take(available));
    }

    pub fn pop_pending_composer_attachment(&mut self) {
        self.pending_composer_attachments.pop();
    }

    pub fn composer_accepts_attachments(&self) -> bool {
        self.edit_target_message.is_none() && self.can_attach_in_selected_channel()
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
    /// Paste-based attachment input uses this to decide whether file paths
    /// become pending uploads or plain composer text.
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
        self.edit_target_message = None;
        self.composer_active = true;
        self.move_composer_cursor_end();
        self.focus = FocusPane::Messages;
    }

    pub fn cancel_composer(&mut self) {
        self.composer_active = false;
        self.composer_input.clear();
        self.composer_cursor_byte_index = 0;
        self.pending_composer_attachments.clear();
        self.reply_target_message_id = None;
        self.edit_target_message = None;
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
        let cursor = self.composer_cursor_byte_index();
        self.replace_composer_range(cursor..cursor, value);
    }

    pub fn pop_composer_char(&mut self) {
        let end = self.composer_cursor_byte_index();
        if end == 0 {
            return;
        }
        let start = previous_char_boundary(&self.composer_input, end);
        self.replace_composer_range(start..end, "");
    }

    pub fn delete_composer_char(&mut self) {
        let start = self.composer_cursor_byte_index();
        if start >= self.composer_input.len() {
            return;
        }
        let end = next_char_boundary(&self.composer_input, start);
        self.replace_composer_range(start..end, "");
    }

    pub fn move_composer_cursor_left(&mut self) {
        let cursor = self.composer_cursor_byte_index();
        self.composer_cursor_byte_index = previous_char_boundary(&self.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_right(&mut self) {
        let cursor = self.composer_cursor_byte_index();
        self.composer_cursor_byte_index = next_char_boundary(&self.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_word_left(&mut self) {
        let cursor = self.composer_cursor_byte_index();
        self.composer_cursor_byte_index = previous_word_boundary(&self.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_word_right(&mut self) {
        let cursor = self.composer_cursor_byte_index();
        self.composer_cursor_byte_index = next_word_boundary(&self.composer_input, cursor);
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_home(&mut self) {
        self.composer_cursor_byte_index = 0;
        self.refresh_active_mention_query();
    }

    pub fn move_composer_cursor_end(&mut self) {
        self.composer_cursor_byte_index = self.composer_input.len();
        self.refresh_active_mention_query();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let expanded = expand_composer_completions(
            &self.composer_input,
            &self.composer_mention_completions,
            &self.composer_emoji_completions,
        );
        let expanded = expand_emoji_shortcodes(&expanded);
        let content = expanded.trim().to_owned();
        let has_attachments = !self.pending_composer_attachments.is_empty();
        if content.is_empty() && !has_attachments {
            return None;
        }
        if let Some((channel_id, message_id)) = self.edit_target_message.take() {
            if content.is_empty() {
                self.edit_target_message = Some((channel_id, message_id));
                return None;
            }
            self.composer_input.clear();
            self.composer_cursor_byte_index = 0;
            self.pending_composer_attachments.clear();
            self.composer_active = false;
            self.reply_target_message_id = None;
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
            self.composer_input.clear();
            self.composer_cursor_byte_index = 0;
            self.pending_composer_attachments.clear();
            self.composer_active = false;
            self.reply_target_message_id = None;
            self.edit_target_message = None;
            self.reset_mention_picker_state();
            return None;
        }
        if has_attachments && !self.can_attach_in_selected_channel() {
            self.composer_input.clear();
            self.composer_cursor_byte_index = 0;
            self.pending_composer_attachments.clear();
            self.composer_active = false;
            self.reply_target_message_id = None;
            self.edit_target_message = None;
            self.reset_mention_picker_state();
            return None;
        }

        self.composer_input.clear();
        self.composer_cursor_byte_index = 0;
        self.reset_mention_picker_state();
        let reply_to = self.reply_target_message_id.take();
        let attachments = std::mem::take(&mut self.pending_composer_attachments);
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
        self.composer_mention_query.as_deref()
    }

    pub fn composer_mention_selected(&self) -> usize {
        self.composer_mention_selected
    }

    /// Builds the full suggestion list for the picker, ordered by best match
    /// across the member's display name AND username: prefix matches beat
    /// substring matches, alias matches beat username matches at the same rank,
    /// and ties are broken alphabetically by display name.
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
            move_picker_selection(self.composer_mention_selected, len, delta);
    }

    pub fn composer_emoji_query(&self) -> Option<&str> {
        self.composer_emoji_query.as_deref()
    }

    pub fn composer_emoji_selected(&self) -> usize {
        self.composer_emoji_selected
    }

    pub fn composer_emoji_candidates(&self) -> Vec<EmojiPickerEntry> {
        self.composer_emoji_candidates.clone()
    }

    pub fn move_composer_emoji_selection(&mut self, delta: isize) {
        if self.composer_emoji_query.is_none() {
            return;
        }
        let len = self.composer_emoji_candidates.len();
        self.composer_emoji_selected =
            move_picker_selection(self.composer_emoji_selected, len, delta);
    }

    /// Confirms the currently highlighted mention. Replaces the trailing
    /// `@query` with `@displayname ` (so the user sees what they wrote) and
    /// records the byte range so `submit_composer` can rewrite it to
    /// `<@USER_ID>` later. Returns `false` when the picker has no candidate
    /// to apply.
    pub fn confirm_composer_mention(&mut self) -> bool {
        let Some(_query) = self.composer_mention_query.clone() else {
            return false;
        };
        let Some(mention_start) = self.composer_mention_start else {
            return false;
        };
        let candidates = self.composer_mention_candidates();
        let Some(entry) = candidates.get(self.composer_mention_selected) else {
            return false;
        };
        let entry = entry.clone();

        let cursor = self.composer_cursor_byte_index();
        if mention_start > cursor {
            return false;
        }

        let replacement = format!("@{} ", entry.display_name);
        self.replace_composer_range(mention_start..cursor, &replacement);
        let end = mention_start + '@'.len_utf8() + entry.display_name.len();

        self.composer_mention_completions.push(MentionCompletion {
            byte_start: mention_start,
            byte_end: end,
            user_id: entry.user_id,
        });
        self.close_composer_mention_query();
        true
    }

    /// Confirms the highlighted emoji. Unicode emoji are inserted directly;
    /// available custom emoji keep their readable `:name:` form and record a
    /// byte range so submit can send Discord's wire markup. Unavailable custom
    /// emoji stay visible in the picker as a hint, but cannot be confirmed.
    pub fn confirm_composer_emoji(&mut self) -> bool {
        let Some(_query) = self.composer_emoji_query.clone() else {
            return false;
        };
        let Some(emoji_start) = self.composer_emoji_start else {
            return false;
        };
        let Some(entry) = self
            .composer_emoji_candidates
            .get(self.composer_emoji_selected)
        else {
            return false;
        };
        let entry = entry.clone();
        if !entry.available {
            return false;
        }

        let cursor = self.composer_cursor_byte_index();
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
            self.composer_emoji_completions.push(EmojiCompletion {
                byte_start: emoji_start,
                byte_end: end,
                replacement: wire_format,
                custom_image_url: entry.custom_image_url,
            });
        }
        self.close_composer_emoji_query();
        true
    }

    pub(in crate::tui) fn composer_emoji_image_completions(
        &self,
    ) -> Vec<ComposerEmojiImageCompletion> {
        self.composer_emoji_completions
            .iter()
            .filter(|completion| completion.byte_end <= self.composer_input.len())
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

    fn reset_mention_picker_state(&mut self) {
        self.close_composer_mention_query();
        self.close_composer_emoji_query();
        self.composer_mention_completions.clear();
        self.composer_emoji_completions.clear();
    }

    fn close_composer_mention_query(&mut self) {
        self.composer_mention_query = None;
        self.composer_mention_start = None;
        self.composer_mention_selected = 0;
    }

    fn close_composer_emoji_query(&mut self) {
        self.composer_emoji_query = None;
        self.composer_emoji_start = None;
        self.composer_emoji_selected = 0;
        self.composer_emoji_candidates.clear();
    }

    pub(super) fn refresh_composer_emoji_candidates_for_current_query(&mut self) {
        let Some(query) = self.composer_emoji_query.clone() else {
            self.composer_emoji_candidates.clear();
            return;
        };

        let candidates = self.emoji_candidates_for_query(&query);
        if candidates.is_empty() {
            self.close_composer_emoji_query();
            return;
        }

        self.composer_emoji_selected = self.composer_emoji_selected.min(candidates.len() - 1);
        self.composer_emoji_candidates = candidates;
    }

    fn replace_composer_range(&mut self, range: Range<usize>, replacement: &str) {
        if range.start > range.end
            || range.end > self.composer_input.len()
            || !self.composer_input.is_char_boundary(range.start)
            || !self.composer_input.is_char_boundary(range.end)
        {
            return;
        }
        self.adjust_mention_completions_for_replace(range.clone(), replacement.len());
        self.adjust_emoji_completions_for_replace(range.clone(), replacement.len());
        self.composer_input
            .replace_range(range.clone(), replacement);
        self.composer_cursor_byte_index = range.start + replacement.len();
        self.refresh_active_mention_query();
    }

    fn refresh_active_mention_query(&mut self) {
        let cursor = self.composer_cursor_byte_index();
        let mut query_start = cursor;

        while query_start > 0 {
            let previous = previous_char_boundary(&self.composer_input, query_start);
            let value = self.composer_input[previous..query_start]
                .chars()
                .next()
                .expect("character boundary slice contains one character");
            if !is_mention_query_char(value) {
                break;
            }
            query_start = previous;
        }

        if query_start > 0 {
            let mention_start = previous_char_boundary(&self.composer_input, query_start);
            if &self.composer_input[mention_start..query_start] == "@"
                && should_start_completion_query(&self.composer_input[..mention_start])
            {
                self.composer_mention_query =
                    Some(self.composer_input[query_start..cursor].to_owned());
                self.composer_mention_start = Some(mention_start);
                self.composer_mention_selected = 0;
                self.close_composer_emoji_query();
                return;
            }
        }

        let mut query_start = cursor;

        while query_start > 0 {
            let previous = previous_char_boundary(&self.composer_input, query_start);
            let value = self.composer_input[previous..query_start]
                .chars()
                .next()
                .expect("character boundary slice contains one character");
            if !is_emoji_query_char(value) {
                break;
            }
            query_start = previous;
        }

        if query_start > 0 {
            let emoji_start = previous_char_boundary(&self.composer_input, query_start);
            let query = &self.composer_input[query_start..cursor];
            if &self.composer_input[emoji_start..query_start] == ":"
                && query.chars().count() >= 2
                && should_start_completion_query(&self.composer_input[..emoji_start])
            {
                let candidates = self.emoji_candidates_for_query(query);
                if candidates.is_empty() {
                    self.close_composer_mention_query();
                    self.close_composer_emoji_query();
                    return;
                }
                self.composer_emoji_query = Some(query.to_owned());
                self.composer_emoji_start = Some(emoji_start);
                self.composer_emoji_selected = 0;
                self.composer_emoji_candidates = candidates;
                self.close_composer_mention_query();
                return;
            }
        }

        self.close_composer_mention_query();
        self.close_composer_emoji_query();
    }

    fn adjust_mention_completions_for_replace(
        &mut self,
        replaced: Range<usize>,
        replacement_len: usize,
    ) {
        let replaced_len = replaced.end - replaced.start;
        let delta = replacement_len as isize - replaced_len as isize;
        let mut completions = Vec::with_capacity(self.composer_mention_completions.len());

        for mut completion in self.composer_mention_completions.drain(..) {
            if completion.byte_end <= replaced.start {
                completions.push(completion);
            } else if completion.byte_start >= replaced.end {
                completion.byte_start = shift_byte_index(completion.byte_start, delta);
                completion.byte_end = shift_byte_index(completion.byte_end, delta);
                completions.push(completion);
            }
        }

        self.composer_mention_completions = completions;
    }

    fn adjust_emoji_completions_for_replace(
        &mut self,
        replaced: Range<usize>,
        replacement_len: usize,
    ) {
        let replaced_len = replaced.end - replaced.start;
        let delta = replacement_len as isize - replaced_len as isize;
        let mut completions = Vec::with_capacity(self.composer_emoji_completions.len());

        for mut completion in self.composer_emoji_completions.drain(..) {
            if completion.byte_end <= replaced.start {
                completions.push(completion);
            } else if completion.byte_start >= replaced.end {
                completion.byte_start = shift_byte_index(completion.byte_start, delta);
                completion.byte_end = shift_byte_index(completion.byte_end, delta);
                completions.push(completion);
            }
        }

        self.composer_emoji_completions = completions;
    }

    fn emoji_candidates_for_query(&self, query: &str) -> Vec<EmojiPickerEntry> {
        let custom_emojis = self
            .selected_channel_guild_id()
            .map(|guild_id| self.discord.custom_emojis_for_guild(guild_id))
            .unwrap_or_default();
        build_emoji_candidates(
            query,
            custom_emojis,
            self.current_user_can_use_animated_custom_emojis
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
