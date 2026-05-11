use crate::discord::{CustomEmojiInfo, ReactionEmoji};

use std::collections::HashSet;

use super::EmojiReactionItem;

const QUICK_UNICODE_EMOJIS: &[&str] = &["👍", "❤️", "😂", "🎉", "😮", "😢", "🙏", "👀"];

pub(super) fn quick_unicode_emoji_reaction_items() -> Vec<EmojiReactionItem> {
    QUICK_UNICODE_EMOJIS
        .iter()
        .map(|emoji| unicode_emoji_reaction_item(emoji))
        .collect()
}

pub(super) fn remaining_unicode_emoji_reaction_items() -> Vec<EmojiReactionItem> {
    let quick_emojis: HashSet<&'static str> = QUICK_UNICODE_EMOJIS.iter().copied().collect();

    emojis::iter()
        .filter(|emoji| !quick_emojis.contains(emoji.as_str()))
        .map(unicode_emoji_reaction_item_from_emoji)
        .collect()
}

pub(super) fn is_quick_unicode_emoji(value: &str) -> bool {
    QUICK_UNICODE_EMOJIS.contains(&value)
}

fn unicode_emoji_reaction_item(value: &str) -> EmojiReactionItem {
    let emoji = emojis::get(value).expect("quick emoji must exist");
    unicode_emoji_reaction_item_from_emoji(emoji)
}

fn unicode_emoji_reaction_item_from_emoji(emoji: &emojis::Emoji) -> EmojiReactionItem {
    EmojiReactionItem {
        emoji: ReactionEmoji::Unicode(emoji.as_str().to_owned()),
        label: unicode_emoji_label(emoji),
    }
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
