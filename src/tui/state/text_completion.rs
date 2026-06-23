use crate::tui::text_cursor::previous_char_boundary;

use super::EmojiPickerEntry;
use super::composer::{is_emoji_query_char, should_start_completion_query};
use super::scroll::clamp_list_scroll;

const EMOJI_PICKER_VISIBLE: usize = 8;

/// Standalone `:shortcode` emoji autocomplete that any text input can own. The
/// caller feeds it the buffer value and cursor on each edit and supplies ranked
/// candidates, so the controller stays free of cache or guild knowledge.
#[derive(Debug, Default)]
pub(in crate::tui::state) struct EmojiCompletionState {
    active: Option<ActiveEmojiCompletion>,
}

#[derive(Debug)]
struct ActiveEmojiCompletion {
    /// Byte offset of the leading `:` of the query inside the buffer.
    start: usize,
    query: String,
    candidates: Vec<EmojiPickerEntry>,
    selected: usize,
    scroll: usize,
}

impl EmojiCompletionState {
    pub(in crate::tui::state) fn close(&mut self) {
        self.active = None;
    }

    pub(in crate::tui::state) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(in crate::tui::state) fn query(&self) -> Option<&str> {
        self.active.as_ref().map(|active| active.query.as_str())
    }

    pub(in crate::tui::state) fn selected(&self) -> usize {
        self.active
            .as_ref()
            .map(|active| active.selected)
            .unwrap_or(0)
    }

    pub(in crate::tui::state) fn candidates(&self) -> &[EmojiPickerEntry] {
        self.active
            .as_ref()
            .map(|active| active.candidates.as_slice())
            .unwrap_or(&[])
    }

    /// Byte offset of the leading `:`, for callers that replace the range in
    /// the buffer themselves.
    pub(in crate::tui::state) fn start(&self) -> Option<usize> {
        self.active.as_ref().map(|active| active.start)
    }

    pub(in crate::tui::state) fn selected_entry(&self) -> Option<&EmojiPickerEntry> {
        self.active
            .as_ref()
            .and_then(|active| active.candidates.get(active.selected))
    }

    /// First visible row for a window of `visible_count` rows, keeping the
    /// highlighted candidate on screen.
    pub(in crate::tui::state) fn window_start(&self, visible_count: usize) -> usize {
        match &self.active {
            Some(active) if !active.candidates.is_empty() => clamp_list_scroll(
                active.selected.min(active.candidates.len() - 1),
                active.scroll,
                visible_count.max(1),
                active.candidates.len(),
            ),
            _ => 0,
        }
    }

    pub(in crate::tui::state) fn move_selection(&mut self, delta: isize) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let len = active.candidates.len();
        if len == 0 {
            return;
        }
        let current = active.selected.min(len - 1) as isize;
        active.selected = (current + delta).clamp(0, len as isize - 1) as usize;
        active.scroll =
            clamp_list_scroll(active.selected, active.scroll, EMOJI_PICKER_VISIBLE, len);
    }

    /// Detect a `:shortcode` query ending at `cursor`. Returns the byte offset
    /// of the `:` and the query text (excluding the colon). Mirrors the main
    /// composer's trigger rules: a leading `:` preceded by start-of-input or
    /// whitespace, and at least two query characters typed.
    ///
    /// Kept separate from [`Self::set`] so callers can run candidate building
    /// (which borrows app state for guild emoji) between the two without holding
    /// a mutable borrow of the controller.
    pub(in crate::tui::state) fn detect(value: &str, cursor: usize) -> Option<(usize, String)> {
        let cursor = cursor.min(value.len());
        let mut query_start = cursor;
        while query_start > 0 {
            let previous = previous_char_boundary(value, query_start);
            let character = value[previous..query_start].chars().next()?;
            if !is_emoji_query_char(character) {
                break;
            }
            query_start = previous;
        }
        if query_start == 0 {
            return None;
        }
        let colon_start = previous_char_boundary(value, query_start);
        if &value[colon_start..query_start] != ":" {
            return None;
        }
        let query = &value[query_start..cursor];
        if query.chars().count() < 2 || !should_start_completion_query(&value[..colon_start]) {
            return None;
        }
        Some((colon_start, query.to_owned()))
    }

    /// Store the freshly built picker for a `detected` query (the result of
    /// [`Self::detect`]) and its `candidates`. The picker closes when there is
    /// no query or no candidate matched.
    pub(in crate::tui::state) fn set(
        &mut self,
        detected: Option<(usize, String)>,
        candidates: Vec<EmojiPickerEntry>,
    ) {
        let Some((start, query)) = detected else {
            self.active = None;
            return;
        };
        if candidates.is_empty() {
            self.active = None;
            return;
        }
        // Keep the highlighted row across keystrokes while the same picker stays
        // open. Start at the top when it first opens.
        let (selected, scroll) = match &self.active {
            Some(active) => {
                let selected = active.selected.min(candidates.len() - 1);
                let scroll = clamp_list_scroll(
                    selected,
                    active.scroll,
                    EMOJI_PICKER_VISIBLE,
                    candidates.len(),
                );
                (selected, scroll)
            }
            None => (0, 0),
        };
        self.active = Some(ActiveEmojiCompletion {
            start,
            query,
            candidates,
            selected,
            scroll,
        });
    }
}
