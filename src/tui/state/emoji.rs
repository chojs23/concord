use crate::discord::{CustomEmojiInfo, ReactionEmoji};

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

pub(super) fn unicode_emoji_reaction_items() -> Vec<EmojiReactionItem> {
    EMOJI_REACTION_ITEMS
        .iter()
        .map(|item| EmojiReactionItem {
            emoji: ReactionEmoji::Unicode(item.emoji.to_owned()),
            label: item.label.to_owned(),
        })
        .collect()
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
    let words: Vec<String> = name
        .split('_')
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
        name.to_owned()
    } else {
        words.join(" ")
    }
}
