use crate::discord::ids::{
    Id,
    marker::{EmojiMarker, UserMarker},
};

use crate::discord::{CustomEmojiInfo, PresenceStatus};

use super::MemberEntry;

/// Maximum number of suggestions composer pickers show at once. Candidate
/// builders still return every match. Rendering scrolls this many rows.
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

/// One entry in the rendered emoji shortcode picker list.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmojiPickerEntry {
    pub emoji: String,
    pub shortcode: String,
    pub name: String,
    pub wire_format: Option<String>,
    pub available: bool,
    pub custom_image_url: Option<String>,
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
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

pub(super) fn move_picker_selection(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current = selected.min(len - 1) as isize;
    (current + delta).clamp(0, len as isize - 1) as usize
}

pub(super) fn build_emoji_candidates(
    query: &str,
    custom_emojis: &[CustomEmojiInfo],
    can_use_animated_custom_emojis: bool,
) -> Vec<EmojiPickerEntry> {
    let needle = query.to_ascii_lowercase();
    if needle.chars().count() < 2 {
        return Vec::new();
    }

    let mut scored: Vec<(u8, String, EmojiPickerEntry)> = custom_emojis
        .iter()
        .filter(|emoji| emoji.name.to_ascii_lowercase().starts_with(&needle))
        .map(|emoji| {
            let shortcode = emoji.name.clone();
            let marker = if emoji.animated { "◇" } else { "◆" };
            let label = if emoji.animated {
                "animated custom emoji"
            } else {
                "custom emoji"
            };
            (
                0,
                shortcode.to_ascii_lowercase(),
                EmojiPickerEntry {
                    emoji: marker.to_owned(),
                    shortcode: shortcode.clone(),
                    name: label.to_owned(),
                    wire_format: Some(custom_emoji_markup(&shortcode, emoji.id, emoji.animated)),
                    available: emoji.available
                        && (!emoji.animated || can_use_animated_custom_emojis),
                    custom_image_url: Some(custom_emoji_image_url(emoji.id, emoji.animated)),
                },
            )
        })
        .collect();

    scored.extend(emojis::iter().flat_map(|emoji| {
        emoji
            .shortcodes()
            .filter(|shortcode| shortcode.starts_with(&needle))
            .map(|shortcode| {
                (
                    1,
                    shortcode.to_owned(),
                    EmojiPickerEntry {
                        emoji: emoji.as_str().to_owned(),
                        shortcode: shortcode.to_owned(),
                        name: emoji.name().to_owned(),
                        wire_format: None,
                        available: true,
                        custom_image_url: None,
                    },
                )
            })
    }));
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

fn custom_emoji_markup(name: &str, id: Id<EmojiMarker>, animated: bool) -> String {
    if animated {
        format!("<a:{name}:{}>", id.get())
    } else {
        format!("<:{name}:{}>", id.get())
    }
}

fn custom_emoji_image_url(id: Id<EmojiMarker>, animated: bool) -> String {
    let extension = if animated { "gif" } else { "png" };
    format!("https://cdn.discordapp.com/emojis/{}.{extension}", id.get())
}

pub(super) fn should_start_completion_query(input: &str) -> bool {
    input.chars().last().is_none_or(char::is_whitespace)
}

pub(super) fn is_mention_query_char(value: char) -> bool {
    value.is_alphanumeric() || matches!(value, '_' | '.' | '-')
}

pub(super) fn is_emoji_query_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '+')
}

pub(super) fn expand_emoji_shortcodes(input: &str) -> String {
    let mut rest = input;
    let mut output = String::with_capacity(input.len());

    while let Some((start, name_start, name_end, end)) = rest.find(':').and_then(|start| {
        let name_start = start + ':'.len_utf8();
        rest[name_start..].find(':').map(|relative_end| {
            (
                start,
                name_start,
                name_start + relative_end,
                name_start + relative_end + ':'.len_utf8(),
            )
        })
    }) {
        if starts_custom_emoji_markup(rest, start) {
            output.push_str(&rest[..name_start]);
            rest = &rest[name_start..];
            continue;
        }

        let shortcode = &rest[name_start..name_end];
        if shortcode.is_empty() {
            let colon_run_end = rest[start..]
                .char_indices()
                .find_map(|(offset, value)| (value != ':').then_some(start + offset))
                .unwrap_or(rest.len());
            let keep_to = if colon_run_end < rest.len() {
                colon_run_end - ':'.len_utf8()
            } else {
                colon_run_end
            };
            output.push_str(&rest[..keep_to]);
            rest = &rest[keep_to..];
            continue;
        }
        if let Some(emoji) = emojis::get_by_shortcode(shortcode) {
            output.push_str(&rest[..start]);
            output.push_str(emoji.as_str());
            rest = &rest[end..];
        } else {
            output.push_str(&rest[..name_end]);
            rest = &rest[name_end..];
        }
    }

    output.push_str(rest);
    output
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmojiCompletion {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) replacement: String,
    pub(super) custom_image_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(in crate::tui) struct ComposerEmojiImageCompletion {
    pub(in crate::tui) byte_start: usize,
    pub(in crate::tui) byte_end: usize,
    pub(in crate::tui) url: String,
}

fn starts_custom_emoji_markup(input: &str, colon_start: usize) -> bool {
    input[..colon_start].ends_with('<') || input[..colon_start].ends_with("<a")
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

/// Rewrites recorded mention and custom emoji ranges in one back-to-front pass.
/// Both completion kinds store byte ranges against the visible composer text, so
/// applying them together prevents earlier replacements from shifting later
/// ranges before they are used.
pub(super) fn expand_composer_completions(
    input: &str,
    mention_completions: &[MentionCompletion],
    emoji_completions: &[EmojiCompletion],
) -> String {
    if mention_completions.is_empty() && emoji_completions.is_empty() {
        return input.to_owned();
    }

    let mut replacements: Vec<CompletionReplacement> = mention_completions
        .iter()
        .filter(|completion| completion.byte_end <= input.len())
        .map(|completion| CompletionReplacement {
            byte_start: completion.byte_start,
            byte_end: completion.byte_end,
            replacement: format!("<@{}>", completion.user_id.get()),
        })
        .collect();

    replacements.extend(
        emoji_completions
            .iter()
            .filter(|completion| completion.byte_end <= input.len())
            .map(|completion| CompletionReplacement {
                byte_start: completion.byte_start,
                byte_end: completion.byte_end,
                replacement: completion.replacement.clone(),
            }),
    );

    replacements.sort_by_key(|replacement| std::cmp::Reverse(replacement.byte_start));
    let mut buffer = input.to_owned();
    for replacement in replacements {
        if !buffer.is_char_boundary(replacement.byte_start)
            || !buffer.is_char_boundary(replacement.byte_end)
        {
            continue;
        }
        buffer.replace_range(
            replacement.byte_start..replacement.byte_end,
            &replacement.replacement,
        );
    }
    buffer
}

struct CompletionReplacement {
    byte_start: usize,
    byte_end: usize,
    replacement: String,
}
