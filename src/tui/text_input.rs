use std::ops::Range;

use super::text_cursor::{
    clamp_cursor_index, next_char_boundary, next_word_boundary, previous_char_boundary,
    previous_word_boundary, vertical_cursor_target,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct TextInputState {
    value: String,
    cursor_byte_index: usize,
}

impl TextInputState {
    pub(in crate::tui) fn value(&self) -> &str {
        &self.value
    }

    pub(in crate::tui) fn cursor_byte_index(&self) -> usize {
        clamp_cursor_index(&self.value, self.cursor_byte_index)
    }

    pub(in crate::tui) fn set_value(&mut self, value: String) {
        self.cursor_byte_index = value.len();
        self.value = value;
    }

    pub(in crate::tui) fn clear(&mut self) {
        self.value.clear();
        self.cursor_byte_index = 0;
    }

    pub(in crate::tui) fn insert_char(&mut self, value: char) {
        let cursor = self.cursor_byte_index();
        self.value.insert(cursor, value);
        self.cursor_byte_index = cursor + value.len_utf8();
    }

    pub(in crate::tui) fn insert_str(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        let cursor = self.cursor_byte_index();
        self.value.insert_str(cursor, value);
        self.cursor_byte_index = cursor + value.len();
    }

    pub(in crate::tui) fn replace_range(&mut self, range: Range<usize>, replacement: &str) -> bool {
        if range.start > range.end
            || range.end > self.value.len()
            || !self.value.is_char_boundary(range.start)
            || !self.value.is_char_boundary(range.end)
        {
            return false;
        }
        self.value.replace_range(range.clone(), replacement);
        self.cursor_byte_index = range.start + replacement.len();
        true
    }

    pub(in crate::tui) fn delete_previous_grapheme(&mut self) -> bool {
        let end = self.cursor_byte_index();
        if end == 0 {
            return false;
        }
        let start = previous_char_boundary(&self.value, end);
        self.replace_range(start..end, "")
    }

    pub(in crate::tui) fn delete_previous_word(&mut self) -> bool {
        let end = self.cursor_byte_index();
        if end == 0 {
            return false;
        }
        let start = previous_word_boundary(&self.value, end);
        self.replace_range(start..end, "")
    }

    pub(in crate::tui) fn move_left(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = previous_char_boundary(&self.value, cursor);
    }

    pub(in crate::tui) fn move_right(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = next_char_boundary(&self.value, cursor);
    }

    pub(in crate::tui) fn move_word_left(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = previous_word_boundary(&self.value, cursor);
    }

    pub(in crate::tui) fn move_word_right(&mut self) {
        let cursor = self.cursor_byte_index();
        self.cursor_byte_index = next_word_boundary(&self.value, cursor);
    }

    pub(in crate::tui) fn move_up(&mut self) {
        if let Some(target) = vertical_cursor_target(&self.value, self.cursor_byte_index(), -1) {
            self.cursor_byte_index = target;
        }
    }

    pub(in crate::tui) fn move_down(&mut self) {
        if let Some(target) = vertical_cursor_target(&self.value, self.cursor_byte_index(), 1) {
            self.cursor_byte_index = target;
        }
    }

    pub(in crate::tui) fn move_home(&mut self) {
        self.cursor_byte_index = 0;
    }

    pub(in crate::tui) fn move_end(&mut self) {
        self.cursor_byte_index = self.value.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_edits_at_utf8_cursor_boundaries() {
        let mut input = TextInputState::default();
        input.insert_str("가🇰🇷나");
        input.move_left();
        input.delete_previous_grapheme();

        assert_eq!(input.value(), "가나");
        assert_eq!(input.cursor_byte_index(), "가".len());
    }

    #[test]
    fn vertical_movement_moves_across_lines() {
        let mut input = TextInputState::default();
        input.set_value("hello\nworld".to_owned());

        // Cursor starts at the end of "world" (column 5).
        input.move_up();
        assert_eq!(input.cursor_byte_index(), "hello".len());
        input.move_down();
        assert_eq!(input.cursor_byte_index(), "hello\nworld".len());

        // No line above the first one, so up is a no-op there.
        input.move_up();
        input.move_up();
        assert_eq!(input.cursor_byte_index(), "hello".len());
    }

    #[test]
    fn replace_rejects_invalid_boundaries() {
        let mut input = TextInputState::default();
        input.insert_str("가나");

        assert!(!input.replace_range(1..2, "x"));
        assert_eq!(input.value(), "가나");
    }
}
