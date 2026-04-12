use crate::discord::ids::{Id, marker::UserMarker};

use crate::discord::PresenceStatus;

use super::MemberEntry;

/// Maximum number of suggestions the @-mention picker shows at once. Mirrors
/// the upper bound used by other composer popups so the layout math stays
/// predictable.
pub const MAX_MENTION_PICKER_VISIBLE: usize = 8;

/// One entry in the rendered @-mention picker list.
#[derive(Debug, Clone)]
pub struct MentionPickerEntry {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle. Shown as a hint in the picker so the user can
    /// tell which entry matches when they typed against the username instead
    /// of the alias.
    pub username: Option<String>,
    pub status: PresenceStatus,
    pub is_bot: bool,
}

pub(super) fn build_mention_candidates(
    query: &str,
    entries: Vec<MemberEntry<'_>>,
) -> Vec<MentionPickerEntry> {
    let needle = query.to_lowercase();
    let mut scored: Vec<(u8, String, MentionPickerEntry)> = entries
        .into_iter()
        .filter_map(|entry| {
            let display_name = entry.display_name();
            let username = entry.username();
            let lowered_display = display_name.to_lowercase();
            let lowered_username = username.as_deref().map(str::to_lowercase);

            // Lower rank wins. We deliberately stagger the ladder so an alias
            // prefix beats a username prefix and either beats a substring hit
            // on the other field.
            let rank = if needle.is_empty() {
                2
            } else if lowered_display.starts_with(&needle) {
                0
            } else if lowered_username
                .as_deref()
                .is_some_and(|name| name.starts_with(&needle))
            {
                1
            } else if lowered_display.contains(&needle) {
                2
            } else if lowered_username
                .as_deref()
                .is_some_and(|name| name.contains(&needle))
            {
                3
            } else {
                return None;
            };
            Some((
                rank,
                lowered_display,
                MentionPickerEntry {
                    user_id: entry.user_id(),
                    display_name,
                    username,
                    status: entry.status(),
                    is_bot: entry.is_bot(),
                },
            ))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.truncate(MAX_MENTION_PICKER_VISIBLE);
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

pub(super) fn move_mention_selection(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current = selected.min(len - 1) as isize;
    (current + delta).clamp(0, len as isize - 1) as usize
}

pub(super) fn should_start_mention_query(input: &str) -> bool {
    input.chars().last().is_none_or(char::is_whitespace)
}

pub(super) fn is_mention_query_char(value: char) -> bool {
    value.is_alphanumeric() || matches!(value, '_' | '.' | '-')
}

/// A previously confirmed mention recorded by byte range inside the composer
/// input. The composer keeps the human-readable `@displayname` text in the
/// editor so the user can see what they wrote, and rewrites these ranges to
/// `<@USER_ID>` only at submission time.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct MentionCompletion {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) user_id: Id<UserMarker>,
}

/// Rewrites every recorded mention range in `input` to Discord's wire format
/// `<@USER_ID>`. Walks ranges back-to-front so earlier byte positions remain
/// valid as later ones grow or shrink.
pub(super) fn expand_mention_completions(input: &str, completions: &[MentionCompletion]) -> String {
    if completions.is_empty() {
        return input.to_owned();
    }
    let mut ordered: Vec<MentionCompletion> = completions
        .iter()
        .filter(|completion| completion.byte_end <= input.len())
        .copied()
        .collect();
    ordered.sort_by_key(|completion| std::cmp::Reverse(completion.byte_start));
    let mut buffer = input.to_owned();
    for completion in ordered {
        let replacement = format!("<@{}>", completion.user_id.get());
        buffer.replace_range(completion.byte_start..completion.byte_end, &replacement);
    }
    buffer
}
