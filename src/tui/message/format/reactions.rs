use ratatui::{style::Style, text::Span};
use unicode_width::UnicodeWidthStr;

use crate::discord::{ReactionEmoji, ReactionInfo};
use crate::tui::text::EmojiImageSize;
use crate::tui::theme;

use super::MessageContentLine;

pub(in crate::tui) fn format_message_reaction_lines(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
) -> Vec<MessageContentLine> {
    let layout =
        lay_out_reaction_chips_with_custom_emoji_images(reactions, width, show_custom_emoji);
    let ReactionLayout {
        lines, self_ranges, ..
    } = layout;
    lines
        .into_iter()
        .enumerate()
        .map(|(line_index, text)| {
            styled_reaction_line(
                text,
                &self_ranges,
                line_index,
                theme::current().style(theme::HighlightGroup::Reaction),
            )
        })
        .collect()
}

pub(crate) fn reaction_line_spans(
    text: &str,
    ranges: &[ReactionStyleRange],
    line_index: usize,
    default_style: Style,
) -> Vec<Span<'static>> {
    styled_reaction_line(text.to_owned(), ranges, line_index, default_style).spans()
}

fn styled_reaction_line(
    text: String,
    ranges: &[ReactionStyleRange],
    line_index: usize,
    default_style: Style,
) -> MessageContentLine {
    let mut line = MessageContentLine::styled_text(text, default_style, Vec::new());
    for range in ranges
        .iter()
        .filter(|range| range.line as usize == line_index)
    {
        line.styled_range(
            range.start,
            range.len,
            theme::current().style(theme::HighlightGroup::SelfReaction),
        );
    }
    line
}

#[cfg(test)]
pub(crate) fn reaction_line_test_spans(
    text: &str,
    ranges: &[ReactionStyleRange],
    line_index: usize,
) -> Vec<Span<'static>> {
    reaction_line_spans(
        text,
        ranges,
        line_index,
        theme::current().style(theme::HighlightGroup::Reaction),
    )
}

/// Position of a custom-emoji image overlay relative to the start of a
/// message's reaction strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReactionImageSlot {
    pub(crate) line: u16,
    pub(crate) col: u16,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReactionStyleRange {
    pub(crate) line: u16,
    pub(crate) start: usize,
    pub(crate) len: usize,
}

/// On-screen column span of a single reaction chip, relative to the start of
/// a message's reaction strip. Used to hit-test a mouse click against the
/// reactions rendered under a message, independent of the chip's text/image
/// styling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReactionChipSpan {
    pub(crate) line: u16,
    pub(crate) start_col: u16,
    pub(crate) width: u16,
    pub(crate) emoji: ReactionEmoji,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReactionLayout {
    pub(crate) lines: Vec<String>,
    pub(crate) slots: Vec<ReactionImageSlot>,
    pub(crate) self_ranges: Vec<ReactionStyleRange>,
    pub(crate) chips: Vec<ReactionChipSpan>,
}

/// Finds which reaction emoji (if any) occupies column `col` of reaction line
/// `line`, recomputing the same layout used to render the reaction strip.
pub(in crate::tui) fn reaction_emoji_at(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
    line: u16,
    col: u16,
) -> Option<ReactionEmoji> {
    lay_out_reaction_chips_with_custom_emoji_images(reactions, width, show_custom_emoji)
        .chips
        .into_iter()
        .find(|chip| {
            chip.line == line
                && col >= chip.start_col
                && col < chip.start_col.saturating_add(chip.width)
        })
        .map(|chip| chip.emoji)
}

/// Builds a single chip's text plus the chip-internal column offset where its
/// image overlay should land (if any). Custom-emoji chips reserve a fixed
/// compact image width of spaces in place of the textual `:name:`
/// label so that loading the image later does not reflow the row.
fn build_reaction_chip(
    reaction: &ReactionInfo,
    show_custom_emoji: bool,
) -> (String, Option<usize>, Option<String>, bool) {
    let count = reaction.count;
    match &reaction.emoji {
        ReactionEmoji::Unicode(emoji) => {
            let chip = format!("[{emoji} {count}]");
            (chip, None, None, reaction.me)
        }
        ReactionEmoji::Custom { .. } if !show_custom_emoji => {
            let label = reaction.emoji.status_label();
            (format!("[{label} {count}]"), None, None, reaction.me)
        }
        ReactionEmoji::Custom { .. } => {
            let url = reaction.emoji.custom_image_url();
            let placeholder = " ".repeat(usize::from(EmojiImageSize::Compact.width()));
            let prefix = "[";
            let chip = format!("{prefix}{placeholder} {count}]");
            let image_offset = prefix.width();
            (chip, Some(image_offset), url, reaction.me)
        }
    }
}

/// Lays out reaction chips for a message, wrapping at chip boundaries so a
/// chip is never split across rows. Returns both the rendered text rows and
/// the absolute (line, col) position of every custom-emoji image overlay,
/// relative to the first reaction row.
#[cfg(test)]
pub(crate) fn lay_out_reaction_chips(reactions: &[ReactionInfo], width: usize) -> ReactionLayout {
    lay_out_reaction_chips_with_custom_emoji_images(reactions, width, true)
}

/// `(chip text, custom-emoji image column offset, custom-emoji image url,
/// is the current user's own reaction, the chip's emoji identity)`.
type ReactionChipSpec = (String, Option<usize>, Option<String>, bool, ReactionEmoji);

pub(crate) fn lay_out_reaction_chips_with_custom_emoji_images(
    reactions: &[ReactionInfo],
    width: usize,
    show_custom_emoji: bool,
) -> ReactionLayout {
    let width = width.max(1);
    let chip_specs: Vec<ReactionChipSpec> = reactions
        .iter()
        .filter(|reaction| reaction.count > 0)
        .map(|reaction| {
            let (chip_text, image_offset, url, is_self) =
                build_reaction_chip(reaction, show_custom_emoji);
            (
                chip_text,
                image_offset,
                url,
                is_self,
                reaction.emoji.clone(),
            )
        })
        .collect();
    if chip_specs.is_empty() {
        return ReactionLayout::default();
    }

    let mut lines: Vec<String> = Vec::new();
    let mut slots: Vec<ReactionImageSlot> = Vec::new();
    let mut self_ranges: Vec<ReactionStyleRange> = Vec::new();
    let mut chips: Vec<ReactionChipSpan> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for (chip_text, image_offset, url, is_self, emoji) in chip_specs {
        let chip_width = chip_text.width();
        let separator_width = if current_width == 0 { 0 } else { 2 };
        let projected = current_width + separator_width + chip_width;
        let needs_wrap = current_width > 0 && projected > width;
        if needs_wrap {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }

        let (chip_start_col, chip_start_byte) = if current_width == 0 {
            (0usize, current.len())
        } else {
            current.push_str("  ");
            current_width += 2;
            (current_width, current.len())
        };
        current.push_str(&chip_text);
        current_width += chip_width;
        if is_self {
            self_ranges.push(ReactionStyleRange {
                line: u16::try_from(lines.len()).unwrap_or(u16::MAX),
                start: chip_start_byte,
                len: chip_text.len(),
            });
        }

        if let (Some(offset), Some(url)) = (image_offset, url) {
            slots.push(ReactionImageSlot {
                line: u16::try_from(lines.len()).unwrap_or(u16::MAX),
                col: u16::try_from(chip_start_col + offset).unwrap_or(u16::MAX),
                url,
            });
        }

        chips.push(ReactionChipSpan {
            line: u16::try_from(lines.len()).unwrap_or(u16::MAX),
            start_col: u16::try_from(chip_start_col).unwrap_or(u16::MAX),
            width: u16::try_from(chip_width).unwrap_or(u16::MAX),
            emoji,
        });
    }

    if !current.is_empty() {
        lines.push(current);
    }

    ReactionLayout {
        lines,
        slots,
        self_ranges,
        chips,
    }
}
