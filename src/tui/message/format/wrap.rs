//! Plain-text wrap engine. Wraps at display width while distributing mention
//! highlights, styled ranges, and custom-emoji image slots per wrapped line.

use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::tui::text::{InlineEmojiSlot, TextHighlight};

use super::{MessageContentImageSlot, StyledPrefix};

pub(super) struct WrappedTextLine {
    pub(super) text: String,
    pub(super) source_start: usize,
    pub(super) source_end: usize,
    pub(super) mention_highlights: Vec<TextHighlight>,
    pub(super) image_slots: Vec<MessageContentImageSlot>,
}

struct WrapBoundary {
    source_start: usize,
    byte_start: usize,
    width: usize,
    slot_count: usize,
}

pub(in crate::tui) fn wrap_text_lines(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    for line in value.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        for grapheme in line.graphemes(true) {
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width.saturating_add(grapheme_width) > width
            {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }

            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
        }
        lines.push(current);
    }
    lines
}

pub(super) fn wrap_text_line_with_styles(
    value: Vec<(Style, String)>,
    width: usize,
) -> Vec<Vec<(Style, String)>> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0usize;
    for (style, region) in value {
        let mut current_region = String::new();
        for grapheme in region.graphemes(true) {
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width.saturating_add(grapheme_width) > width
            {
                if !current_region.is_empty() {
                    current.push((style, current_region));
                }
                lines.push(current);
                current = Vec::new();
                current_region = String::new();
                current_width = 0;
            }

            current_region.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
        }
        if !current_region.is_empty() {
            current.push((style, current_region));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Wraps `value` to `width`, distributing mention highlights and custom-
/// emoji slots per line. Each slot is treated as an atomic `display_width`
/// unit so the `:name:` fallback cannot straddle a wrap edge.
#[cfg(test)]
fn wrap_text_with_extras(
    value: &str,
    highlights: &[TextHighlight],
    emoji_slots: &[InlineEmojiSlot],
    width: usize,
) -> Vec<(String, Vec<TextHighlight>, Vec<MessageContentImageSlot>)> {
    wrap_text_with_metadata(value, highlights, emoji_slots, width)
        .into_iter()
        .map(|line| (line.text, line.mention_highlights, line.image_slots))
        .collect()
}

fn wrapped_line(
    text: String,
    source_start: usize,
    source_end: usize,
    highlights: &[TextHighlight],
    image_slots: Vec<MessageContentImageSlot>,
) -> WrappedTextLine {
    WrappedTextLine {
        text,
        source_start,
        source_end,
        mention_highlights: highlights_for_range(highlights, source_start, source_end),
        image_slots,
    }
}

pub(super) fn wrap_text_with_metadata(
    value: &str,
    highlights: &[TextHighlight],
    emoji_slots: &[InlineEmojiSlot],
    width: usize,
) -> Vec<WrappedTextLine> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines: Vec<WrappedTextLine> = Vec::new();
    let mut line_start = 0usize;
    for line in value.split('\n') {
        if line.is_empty() {
            lines.push(wrapped_line(
                String::new(),
                line_start,
                line_start,
                highlights,
                Vec::new(),
            ));
            line_start = line_start.saturating_add(1);
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        let mut current_start = line_start;
        let mut current_end = line_start;
        let mut current_slots: Vec<MessageContentImageSlot> = Vec::new();
        let mut word_boundary: Option<WrapBoundary> = None;
        let mut previous_was_whitespace = false;
        for (relative_start, grapheme) in line.grapheme_indices(true) {
            let grapheme_start = line_start.saturating_add(relative_start);
            let grapheme_end = grapheme_start.saturating_add(grapheme.len());
            let grapheme_width = grapheme.width();
            let slot_at_grapheme = emoji_slots
                .iter()
                .find(|slot| slot.byte_start == grapheme_start);
            let grapheme_is_separator =
                slot_at_grapheme.is_none() && grapheme.chars().all(char::is_whitespace);
            if !grapheme_is_separator
                && previous_was_whitespace
                && current.chars().any(|ch| !ch.is_whitespace())
            {
                word_boundary = Some(WrapBoundary {
                    source_start: grapheme_start,
                    byte_start: current.len(),
                    width: current_width,
                    slot_count: current_slots.len(),
                });
            }
            let effective_width = match slot_at_grapheme {
                Some(slot) => slot.display_width as usize,
                None => grapheme_width,
            };
            if current_width > 0
                && effective_width > 0
                && current_width.saturating_add(effective_width) > width
            {
                if grapheme_is_separator {
                    lines.push(wrapped_line(
                        std::mem::take(&mut current),
                        current_start,
                        current_end,
                        highlights,
                        std::mem::take(&mut current_slots),
                    ));
                    current_width = 0;
                    current_start = grapheme_end;
                    current_end = grapheme_end;
                    word_boundary = None;
                    previous_was_whitespace = true;
                    continue;
                } else if let Some(boundary) = word_boundary
                    .take()
                    .filter(|boundary| boundary.byte_start < current.len())
                {
                    let text = current[..boundary.byte_start].to_owned();
                    let mut next = current[boundary.byte_start..].to_owned();
                    let mut next_slots = current_slots.split_off(boundary.slot_count);
                    for slot in &mut next_slots {
                        slot.byte_start = slot.byte_start.saturating_sub(boundary.byte_start);
                        slot.col = slot
                            .col
                            .saturating_sub(u16::try_from(boundary.width).unwrap_or(u16::MAX));
                    }
                    lines.push(wrapped_line(
                        text,
                        current_start,
                        boundary.source_start,
                        highlights,
                        current_slots,
                    ));
                    std::mem::swap(&mut current, &mut next);
                    current_slots = next_slots;
                    current_width = current_width.saturating_sub(boundary.width);
                    current_start = boundary.source_start;
                } else {
                    lines.push(wrapped_line(
                        std::mem::take(&mut current),
                        current_start,
                        current_end,
                        highlights,
                        std::mem::take(&mut current_slots),
                    ));
                    current_width = 0;
                    current_start = grapheme_start;
                    word_boundary = None;
                }
            }

            if let Some(slot) = slot_at_grapheme {
                let line_byte_start = current.len();
                current_slots.push(MessageContentImageSlot {
                    col: u16::try_from(current_width).unwrap_or(u16::MAX),
                    byte_start: line_byte_start,
                    byte_len: slot.byte_len,
                    display_width: slot.display_width,
                    url: slot.url.clone(),
                });
            }

            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
            current_end = grapheme_end;
            previous_was_whitespace = grapheme_is_separator;
        }
        lines.push(wrapped_line(
            current,
            current_start,
            current_end,
            highlights,
            current_slots,
        ));
        line_start = line_start.saturating_add(line.len()).saturating_add(1);
    }
    lines
}

pub(super) fn styled_ranges_for_range(
    styled_ranges: &[StyledPrefix],
    start: usize,
    end: usize,
) -> Vec<StyledPrefix> {
    styled_ranges
        .iter()
        .filter_map(|range| {
            let range_start = range.start.max(start);
            let range_end = range.start.saturating_add(range.len).min(end);
            (range_start < range_end).then(|| StyledPrefix {
                start: range_start.saturating_sub(start),
                len: range_end.saturating_sub(range_start),
                style: range.style,
                patch_base: range.patch_base,
            })
        })
        .collect()
}

pub(super) fn highlights_for_range(
    highlights: &[TextHighlight],
    start: usize,
    end: usize,
) -> Vec<TextHighlight> {
    highlights
        .iter()
        .filter_map(|highlight| {
            let highlight_start = highlight.start.max(start);
            let highlight_end = highlight.end.min(end);
            (highlight_start < highlight_end).then(|| TextHighlight {
                start: highlight_start.saturating_sub(start),
                end: highlight_end.saturating_sub(start),
                kind: highlight.kind,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_distributes_emoji_slots_per_line_with_correct_columns() {
        let text = "ab:e:cd:e:";
        let slots = vec![
            InlineEmojiSlot {
                byte_start: 2,
                byte_len: 3,
                display_width: 3,
                url: "u-first".to_owned(),
            },
            InlineEmojiSlot {
                byte_start: 7,
                byte_len: 3,
                display_width: 3,
                url: "u-second".to_owned(),
            },
        ];

        let lines = wrap_text_with_extras(text, &[], &slots, 7);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, "ab:e:cd");
        assert_eq!(lines[0].2.len(), 1);
        assert_eq!(lines[0].2[0].col, 2);
        assert_eq!(lines[0].2[0].byte_start, 2);
        assert_eq!(lines[0].2[0].byte_len, 3);
        assert_eq!(lines[0].2[0].url, "u-first");
        assert_eq!(lines[1].0, ":e:");
        assert_eq!(lines[1].2.len(), 1);
        assert_eq!(lines[1].2[0].col, 0);
        assert_eq!(lines[1].2[0].byte_start, 0);
        assert_eq!(lines[1].2[0].url, "u-second");
    }

    #[test]
    fn wrap_keeps_emoji_text_fallback_atomic_at_line_edge() {
        let text = "ab:e:";
        let slots = vec![InlineEmojiSlot {
            byte_start: 2,
            byte_len: 3,
            display_width: 3,
            url: "u".to_owned(),
        }];
        let lines = wrap_text_with_extras(text, &[], &slots, 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, "ab");
        assert_eq!(lines[0].2.len(), 0);
        assert_eq!(lines[1].0, ":e:");
        assert_eq!(lines[1].2.len(), 1);
        assert_eq!(lines[1].2[0].col, 0);
        assert_eq!(lines[1].2[0].byte_start, 0);
    }

    #[test]
    fn wrap_prefers_word_boundaries_when_possible() {
        let cases = [
            (
                "this is a line where the last word spills",
                37,
                vec!["this is a line where the last word ", "spills"],
            ),
            ("hello world again", 11, vec!["hello world", "again"]),
            (
                "supercalifragilistic",
                6,
                vec!["superc", "alifra", "gilist", "ic"],
            ),
        ];

        for (text, width, expected) in cases {
            let lines = wrap_text_with_extras(text, &[], &[], width)
                .into_iter()
                .map(|line| line.0)
                .collect::<Vec<_>>();

            assert_eq!(lines, expected);
        }
    }
}
