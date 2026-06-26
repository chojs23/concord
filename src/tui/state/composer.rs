mod completions;
mod state;

pub use completions::{
    CommandPickerEntry, EmojiPickerEntry, MAX_MENTION_PICKER_VISIBLE, MentionPickerEntry,
    MentionPickerTarget,
};
// Pure emoji helpers shared with the emoji completion controller and the forum
// post submit path.
pub(in crate::tui::state) use completions::{
    expand_emoji_shortcodes, is_emoji_query_char, should_start_completion_query,
};
pub use state::DmComposerLock;
pub(super) use state::ComposerUiState;
