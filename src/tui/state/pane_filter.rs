use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug)]
pub(super) struct PaneFilterState {
    pub(super) query: String,
    pub(super) query_cursor_byte_index: usize,
}

impl PaneFilterState {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            query_cursor_byte_index: 0,
        }
    }

    pub(super) fn query(&self) -> &str {
        &self.query
    }

    pub(super) fn cursor_byte_index(&self) -> usize {
        clamp_cursor_index(&self.query, self.query_cursor_byte_index)
    }

    pub(super) fn push_char(&mut self, value: char) {
        let cursor = self.cursor_byte_index();
        self.query.insert(cursor, value);
        self.query_cursor_byte_index = cursor + value.len_utf8();
    }

    pub(super) fn pop_char(&mut self) {
        let cursor = self.cursor_byte_index();
        if cursor == 0 {
            return;
        }
        let start = previous_char_boundary(&self.query, cursor);
        self.query.replace_range(start..cursor, "");
        self.query_cursor_byte_index = start;
    }

    pub(super) fn cursor_left(&mut self) {
        let cursor = self.cursor_byte_index();
        self.query_cursor_byte_index = previous_char_boundary(&self.query, cursor);
    }

    pub(super) fn cursor_right(&mut self) {
        let cursor = self.cursor_byte_index();
        self.query_cursor_byte_index = next_char_boundary(&self.query, cursor);
    }
}

fn clamp_cursor_index(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn previous_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[..index]
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[index..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(value.len())
}
