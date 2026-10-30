use crate::config::ReactionOptions;
use crate::discord::{CustomEmojiInfo, ReactionEmoji};

use std::collections::HashSet;

use super::{DashboardState, EmojiReactionItem};

pub(super) const DEFAULT_QUICK_UNICODE_EMOJIS: &[&str] =
    &["👍", "❤️", "😂", "🎉", "😮", "😢", "🙏", "👀"];

impl DashboardState {
    /// Toggles `emoji`'s pinned state, persists it, and refreshes any open
    /// picker so the new pinned-first ordering shows immediately. Silently
    /// no-ops when pinning would exceed [`ReactionOptions::MAX_PINNED_EMOJIS`].
    pub fn toggle_pinned_emoji(&mut self, emoji: &ReactionEmoji) {
        let pins = &mut self.options.reaction_options.pinned_emojis;
        if let Some(position) = pins.iter().position(|pinned| pinned.pin_identity_eq(emoji)) {
            pins.remove(position);
        } else {
            if pins.len() >= ReactionOptions::MAX_PINNED_EMOJIS {
                return;
            }
            pins.push(emoji.clone());
        }
        self.options.config_save_pending = true;
        self.refresh_open_emoji_reaction_picker_items();
        self.refresh_composer_emoji_candidates_for_current_query();
    }

    pub fn pinned_emojis(&self) -> &[ReactionEmoji] {
        &self.options.reaction_options.pinned_emojis
    }
}

/// The "quick" tier shown at the top of the reaction picker: the user's pins
/// when any are set, otherwise a curated default set of unicode emoji.
pub(super) fn quick_emoji_reaction_items(
    pins: &[ReactionEmoji],
    eligible_custom_items: &[EmojiReactionItem],
) -> Vec<EmojiReactionItem> {
    if pins.is_empty() {
        DEFAULT_QUICK_UNICODE_EMOJIS
            .iter()
            .filter_map(|emoji| emojis::get(emoji).map(unicode_emoji_reaction_item_from_emoji))
            .collect()
    } else {
        // Saved pins change rank, not availability. Resolve custom IDs through
        // eligible live entries so renamed emoji also use their current data.
        pins.iter()
            .filter_map(|pin| match pin {
                ReactionEmoji::Unicode(value) => {
                    emojis::get(value).map(unicode_emoji_reaction_item_from_emoji)
                }
                ReactionEmoji::Custom { .. } => eligible_custom_items
                    .iter()
                    .find(|item| item.emoji.pin_identity_eq(pin))
                    .cloned(),
            })
            .collect()
    }
}

/// Every unicode emoji not already covered by the quick tier.
pub(super) fn remaining_unicode_emoji_reaction_items(
    pins: &[ReactionEmoji],
) -> Vec<EmojiReactionItem> {
    let quick: HashSet<&str> = if pins.is_empty() {
        DEFAULT_QUICK_UNICODE_EMOJIS.iter().copied().collect()
    } else {
        pins.iter()
            .filter_map(|pin| match pin {
                ReactionEmoji::Unicode(value) => Some(value.as_str()),
                ReactionEmoji::Custom { .. } => None,
            })
            .collect()
    };

    emojis::iter()
        .filter(|emoji| !quick.contains(emoji.as_str()))
        .map(unicode_emoji_reaction_item_from_emoji)
        .collect()
}

/// Whether a unicode emoji is already represented in the quick tier, so
/// callers can skip re-listing it in the remaining/searchable tier.
pub(super) fn is_quick_unicode_emoji(value: &str, pins: &[ReactionEmoji]) -> bool {
    if pins.is_empty() {
        DEFAULT_QUICK_UNICODE_EMOJIS.contains(&value)
    } else {
        pins.iter()
            .any(|pin| matches!(pin, ReactionEmoji::Unicode(pinned) if pinned == value))
    }
}

fn unicode_emoji_reaction_item_from_emoji(emoji: &emojis::Emoji) -> EmojiReactionItem {
    EmojiReactionItem {
        emoji: ReactionEmoji::Unicode(emoji.as_str().to_owned()),
        label: unicode_emoji_label(emoji),
    }
}

pub(super) fn custom_emoji_reaction_item(emoji: &CustomEmojiInfo) -> EmojiReactionItem {
    EmojiReactionItem {
        emoji: custom_emoji_identity(emoji),
        label: custom_emoji_label(&emoji.name),
    }
}

pub(super) fn custom_emoji_identity(emoji: &CustomEmojiInfo) -> ReactionEmoji {
    ReactionEmoji::Custom {
        id: emoji.id,
        name: Some(emoji.name.clone()),
        animated: emoji.animated,
    }
}

pub(super) fn custom_emoji_can_be_used_directly(
    emoji: &CustomEmojiInfo,
    is_foreign: bool,
    has_nitro: bool,
) -> bool {
    (!is_foreign && !emoji.animated) || has_nitro
}

fn custom_emoji_label(name: &str) -> String {
    title_case_words(name.split('_'))
}

fn unicode_emoji_label(emoji: &emojis::Emoji) -> String {
    title_case_words(emoji.name().split_whitespace())
}

fn title_case_words<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let words: Vec<String> = words
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect();

    if words.is_empty() {
        String::new()
    } else {
        words.join(" ")
    }
}
