use crate::discord::{CustomEmojiInfo, ReactionEmoji};

use std::collections::HashSet;

use super::EmojiReactionItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UnicodeEmojiReactionItem {
    emoji: &'static str,
    label: &'static str,
}

const EMOJI_REACTION_ITEMS: &[UnicodeEmojiReactionItem] = &[
    UnicodeEmojiReactionItem {
        emoji: "👍",
        label: "Thumbs up",
    },
    UnicodeEmojiReactionItem {
        emoji: "❤️",
        label: "Heart",
    },
    UnicodeEmojiReactionItem {
        emoji: "😂",
        label: "Laugh",
    },
    UnicodeEmojiReactionItem {
        emoji: "🎉",
        label: "Celebrate",
    },
    UnicodeEmojiReactionItem {
        emoji: "😮",
        label: "Surprised",
    },
    UnicodeEmojiReactionItem {
        emoji: "😢",
        label: "Sad",
    },
    UnicodeEmojiReactionItem {
        emoji: "🙏",
        label: "Thanks",
    },
    UnicodeEmojiReactionItem {
        emoji: "👀",
        label: "Looking",
    },
];

pub(super) fn quick_unicode_emoji_reaction_items() -> Vec<EmojiReactionItem> {
    EMOJI_REACTION_ITEMS
        .iter()
        .map(|item| EmojiReactionItem {
            emoji: ReactionEmoji::Unicode(item.emoji.to_owned()),
            label: item.label.to_owned(),
        })
        .collect()
}

pub(super) fn remaining_unicode_emoji_reaction_items() -> Vec<EmojiReactionItem> {
    let quick_emojis: HashSet<&'static str> =
        EMOJI_REACTION_ITEMS.iter().map(|item| item.emoji).collect();

    emojis::iter()
        .filter(|emoji| !quick_emojis.contains(emoji.as_str()))
        .map(|emoji| EmojiReactionItem {
            emoji: ReactionEmoji::Unicode(emoji.as_str().to_owned()),
            label: unicode_emoji_label(emoji),
        })
        .collect()
}

pub(super) fn is_quick_unicode_emoji(value: &str) -> bool {
    EMOJI_REACTION_ITEMS.iter().any(|item| item.emoji == value)
}

pub(super) fn custom_emoji_reaction_item(emoji: &CustomEmojiInfo) -> EmojiReactionItem {
    EmojiReactionItem {
        emoji: ReactionEmoji::Custom {
            id: emoji.id,
            name: Some(emoji.name.clone()),
            animated: emoji.animated,
        },
        label: custom_emoji_label(&emoji.name),
    }
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
