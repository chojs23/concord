//! Block and inline Discord-flavored markdown: headings, quotes, bullets,
//! fenced code boxes with syntax highlight, and inline marker styling.

use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::tui::state::DashboardState;
use crate::tui::text::{InlineEmojiSlot, RenderedText, TextHighlight, truncate_display_width};
use crate::tui::theme;

use super::wrap::wrap_text_line_with_styles;
use super::{
    MessageContentLine, StyledPrefix, prefix_message_content_line_with_style, rendered_text_slice,
    rendered_text_with_loaded_custom_emoji_placeholders, rendered_text_without_prefix,
    wrap_rendered_text_lines, wrap_rendered_text_lines_with_styled_ranges,
};

const MARKDOWN_QUOTE_PREFIX: &str = "▎ ";
const MARKDOWN_BULLET_PREFIX: &str = "• ";

struct InlineMarkdownText {
    rendered: RenderedText,
    styled_ranges: Vec<StyledPrefix>,
}

struct SourceSegment {
    source_start: usize,
    source_end: usize,
    output_start: usize,
}

pub(super) fn wrap_markdown_message_lines_with_loaded_custom_emoji_urls(
    state: &DashboardState,
    rendered: RenderedText,
    width: usize,
    style: Style,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    if rendered.text.is_empty() {
        return wrap_rendered_text_lines(rendered, width, style);
    }

    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut in_code_block = false;
    let mut code_block_label: Option<String> = None;
    let mut code_block_fence: Option<RenderedText> = None;
    let mut code_block_lines = Vec::new();
    for line in rendered.text.split('\n') {
        let line_end = line_start.saturating_add(line.len());
        let rendered_line = rendered_text_slice(&rendered, line_start, line_end);
        if in_code_block {
            if let Some(content_end) = markdown_code_fence_closing_content_end(&rendered_line.text)
            {
                if content_end > 0 {
                    code_block_lines.push(rendered_text_slice(&rendered_line, 0, content_end));
                }
                lines.extend(wrap_code_block_lines_and_highlight(
                    state,
                    std::mem::take(&mut code_block_lines),
                    width,
                    code_block_label.take(),
                ));
                in_code_block = false;
                code_block_fence = None;
            } else {
                code_block_lines.push(rendered_line);
            }
        } else if let Some(label) = markdown_code_fence_label(&rendered_line.text) {
            in_code_block = true;
            code_block_label = (!label.is_empty()).then_some(label);
            code_block_fence = Some(rendered_line);
        } else {
            let rendered_line = rendered_text_with_loaded_custom_emoji_placeholders(
                rendered_line,
                loaded_custom_emoji_urls,
            );
            lines.extend(wrap_markdown_message_line(rendered_line, width, style));
        }
        line_start = line_end.saturating_add(1);
    }
    if in_code_block {
        if code_block_lines.is_empty() {
            if let Some(fence) = code_block_fence {
                lines.extend(wrap_markdown_inline_text(fence, width, style));
            }
        } else {
            lines.extend(wrap_code_block_lines_and_highlight(
                state,
                code_block_lines,
                width,
                code_block_label,
            ));
        }
    }
    lines
}

fn wrap_markdown_message_line(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    if rendered.text.is_empty() {
        return vec![MessageContentLine::styled_text(
            String::new(),
            style,
            Vec::new(),
        )];
    }

    if let Some((prefix_len, heading_style)) = markdown_heading(&rendered.text, style) {
        let prefix = rendered.text[..prefix_len].to_owned();
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            heading_style,
            &prefix,
            markdown_marker_style(),
        );
    }

    if let Some(prefix_len) = markdown_quote_prefix_len(&rendered.text) {
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            theme::current().apply(theme::HighlightGroup::MarkdownQuote, style),
            MARKDOWN_QUOTE_PREFIX,
            markdown_marker_style(),
        );
    }

    if let Some(prefix_len) = markdown_bullet_prefix_len(&rendered.text) {
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            style,
            MARKDOWN_BULLET_PREFIX,
            markdown_marker_style(),
        );
    }

    wrap_markdown_inline_text(rendered, width, style)
}

fn wrap_markdown_inline_text(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    let inline = parse_inline_markdown(rendered);
    wrap_rendered_text_lines_with_styled_ranges(
        inline.rendered,
        width,
        style,
        &inline.styled_ranges,
    )
}

fn wrap_markdown_inline_text_preserving_empty(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    let mut lines = wrap_markdown_inline_text(rendered, width, style);
    if lines.is_empty() {
        lines.push(MessageContentLine::styled_text(
            String::new(),
            style,
            Vec::new(),
        ));
    }
    lines
}

fn wrap_code_block_lines_and_highlight(
    state: &DashboardState,
    code_lines: Vec<RenderedText>,
    width: usize,
    label: Option<String>,
) -> Vec<MessageContentLine> {
    let style = markdown_code_style();
    let highlighted_lines = {
        if let Some(language) = label.as_ref().filter(|l| !l.is_empty()) {
            let text_lines = code_lines.into_iter().map(|rt| rt.text).collect::<Vec<_>>();
            state
                .syntax_highlight_cache
                .highlight(&text_lines, language)
        } else {
            code_lines
                .into_iter()
                .map(|rt| vec![(style, rt.text)])
                .collect()
        }
    };

    let inner_width = width.saturating_sub(4).max(1);
    let mut body_lines = Vec::new();
    for regions in highlighted_lines {
        let wrapped = wrap_text_line_with_styles(regions, inner_width);
        if wrapped.is_empty() {
            body_lines.push(vec![(style, String::new())]);
        } else {
            body_lines.extend(wrapped);
        }
    }
    if body_lines.is_empty() {
        body_lines.push(vec![(style, String::new())]);
    }

    let content_width = body_lines
        .iter()
        .map(|line| line.iter().map(|region| region.1.width()).sum())
        .max()
        .unwrap_or(0)
        .max(4)
        .min(inner_width);

    let mut lines = vec![code_box_border_line(
        '╭',
        '╮',
        content_width,
        label.as_deref(),
    )];
    lines.extend(
        body_lines
            .into_iter()
            .map(|line| code_box_body_line(line, content_width)),
    );
    lines.push(code_box_border_line('╰', '╯', content_width, None));
    lines
}

fn code_box_border_line(
    left: char,
    right: char,
    content_width: usize,
    label: Option<&str>,
) -> MessageContentLine {
    let inner_width = content_width.saturating_add(2);
    let inner = label
        .filter(|label| !label.is_empty())
        .map(|label| {
            let label = truncate_display_width(label, inner_width.saturating_sub(3));
            let title = format!("─ {label} ");
            if title.width() >= inner_width {
                title
            } else {
                format!(
                    "{title}{}",
                    "─".repeat(inner_width.saturating_sub(title.width()))
                )
            }
        })
        .unwrap_or_else(|| "─".repeat(inner_width));
    MessageContentLine::styled_text(
        format!("{left}{inner}{right}"),
        code_box_border_style(),
        Vec::new(),
    )
}

fn code_box_body_line(regions: Vec<(Style, String)>, content_width: usize) -> MessageContentLine {
    let mut line =
        MessageContentLine::styled_text("│ ".to_owned(), code_box_border_style(), Vec::new());
    let content_start = line.text.len();
    let mut width = 0usize;
    let mut current_pos = content_start;
    for (style, text) in regions {
        line.text.push_str(&text);
        line.styled_prefixes.push(StyledPrefix {
            start: current_pos,
            len: text.len(),
            style,
            patch_base: false,
        });
        width += text.width();
        current_pos += text.len();
    }
    let padding = content_width.saturating_sub(width);
    line.text.push_str(&" ".repeat(padding));
    line.append_styled_suffix(" │", code_box_border_style());
    line
}

fn wrap_prefixed_markdown_line(
    rendered: RenderedText,
    width: usize,
    style: Style,
    prefix: &str,
    prefix_style: Style,
) -> Vec<MessageContentLine> {
    let body_width = width.saturating_sub(prefix.width()).max(1);
    wrap_markdown_inline_text_preserving_empty(rendered, body_width, style)
        .into_iter()
        .map(|line| prefix_message_content_line_with_style(prefix, prefix_style, line))
        .collect()
}

fn markdown_heading(value: &str, base: Style) -> Option<(usize, Style)> {
    let (prefix_len, group) = if value.starts_with("# ") {
        ("# ".len(), theme::HighlightGroup::MarkdownHeading1)
    } else if value.starts_with("## ") {
        ("## ".len(), theme::HighlightGroup::MarkdownHeading2)
    } else if value.starts_with("### ") {
        ("### ".len(), theme::HighlightGroup::MarkdownHeading3)
    } else {
        return None;
    };
    Some((prefix_len, theme::current().apply(group, base)))
}

fn markdown_marker_style() -> Style {
    theme::current().style(theme::HighlightGroup::MarkdownMarker)
}

fn markdown_quote_prefix_len(value: &str) -> Option<usize> {
    if value == ">" {
        Some(1)
    } else {
        value.starts_with("> ").then_some(2)
    }
}

fn markdown_bullet_prefix_len(value: &str) -> Option<usize> {
    ["- ", "* "]
        .into_iter()
        .find_map(|prefix| value.starts_with(prefix).then_some(prefix.len()))
}

fn markdown_code_fence_label(value: &str) -> Option<String> {
    value
        .trim_start()
        .strip_prefix("```")
        .map(|label| label.trim().to_owned())
}

fn markdown_code_fence_closing_content_end(value: &str) -> Option<usize> {
    if value.trim() == "```" {
        return Some(0);
    }

    let trimmed_end_len = value.trim_end().len();
    let before_closing_fence = value[..trimmed_end_len].strip_suffix("```")?;
    (!before_closing_fence.trim().is_empty()).then_some(before_closing_fence.len())
}

fn markdown_code_style() -> Style {
    Style::default()
}

fn inline_code_style() -> Style {
    theme::current().style(theme::HighlightGroup::InlineCode)
}

fn code_box_border_style() -> Style {
    theme::current().style(theme::HighlightGroup::CodeBlockBorder)
}

fn parse_inline_markdown(rendered: RenderedText) -> InlineMarkdownText {
    let mut output = String::with_capacity(rendered.text.len());
    let mut source_segments = Vec::new();
    let mut styled_ranges = Vec::new();
    let mut cursor = 0usize;

    while cursor < rendered.text.len() {
        if let Some((marker, style)) = inline_markdown_marker_at(&rendered.text, cursor) {
            let content_start = cursor.saturating_add(marker.len());
            if let Some(content_end) =
                find_inline_markdown_closer(&rendered.text, content_start, marker)
            {
                let output_start = output.len();
                push_source_segment(
                    &mut output,
                    &mut source_segments,
                    &rendered.text,
                    content_start,
                    content_end,
                );
                let len = output.len().saturating_sub(output_start);
                if len > 0 {
                    styled_ranges.push(StyledPrefix {
                        start: output_start,
                        len,
                        style,
                        patch_base: true,
                    });
                }
                cursor = content_end.saturating_add(marker.len());
                continue;
            }
        }

        let next = if let Some((marker, _)) = inline_markdown_marker_at(&rendered.text, cursor) {
            cursor.saturating_add(marker.len())
        } else {
            next_inline_markdown_marker(&rendered.text, cursor).unwrap_or(rendered.text.len())
        };
        push_source_segment(
            &mut output,
            &mut source_segments,
            &rendered.text,
            cursor,
            next,
        );
        cursor = next;
    }

    InlineMarkdownText {
        rendered: RenderedText {
            text: output,
            highlights: remap_highlights_with_segments(&rendered.highlights, &source_segments),
            emoji_slots: remap_emoji_slots_with_segments(&rendered.emoji_slots, &source_segments),
        },
        styled_ranges,
    }
}

fn next_inline_markdown_marker(value: &str, cursor: usize) -> Option<usize> {
    ["`", "***", "**", "*", "__", "_", "~~"]
        .into_iter()
        .filter_map(|marker| next_inline_markdown_marker_for(value, cursor, marker))
        .min()
}

fn next_inline_markdown_marker_for(value: &str, cursor: usize, marker: &str) -> Option<usize> {
    let mut search_start = cursor;
    while let Some(relative) = value[search_start..].find(marker) {
        let index = search_start.saturating_add(relative);
        if marker != "_" || should_open_underscore_marker(value, index) {
            return Some(index);
        }
        search_start = index.saturating_add(marker.len());
    }
    None
}

fn inline_markdown_marker_at(value: &str, cursor: usize) -> Option<(&'static str, Style)> {
    let rest = &value[cursor..];
    if rest.starts_with('`') {
        Some(("`", inline_code_style()))
    } else if rest.starts_with("***") {
        Some((
            "***",
            Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC),
        ))
    } else if rest.starts_with("**") {
        Some(("**", Style::default().add_modifier(Modifier::BOLD)))
    } else if rest.starts_with('*') {
        Some(("*", Style::default().add_modifier(Modifier::ITALIC)))
    } else if rest.starts_with("__") {
        Some(("__", Style::default().add_modifier(Modifier::UNDERLINED)))
    } else if rest.starts_with('_') && should_open_underscore_marker(value, cursor) {
        Some(("_", Style::default().add_modifier(Modifier::ITALIC)))
    } else if rest.starts_with("~~") {
        Some(("~~", Style::default().add_modifier(Modifier::CROSSED_OUT)))
    } else {
        None
    }
}

fn find_inline_markdown_closer(value: &str, start: usize, marker: &str) -> Option<usize> {
    if matches!(marker, "*" | "_") {
        let mut search_start = start;
        while let Some(relative) = value[search_start..].find(marker) {
            let index = search_start.saturating_add(relative);
            if marker == "_" && !should_close_underscore_marker(value, index) {
                search_start = index.saturating_add(1);
                continue;
            }
            if marker == "_" || !value[index..].starts_with("**") {
                return (start < index).then_some(index);
            }
            search_start = index.saturating_add(1);
        }
        None
    } else {
        value[start..]
            .find(marker)
            .map(|relative| start.saturating_add(relative))
            .filter(|index| start < *index)
    }
}

fn should_open_underscore_marker(value: &str, index: usize) -> bool {
    let previous = value[..index].chars().next_back();
    let next = value[index + '_'.len_utf8()..].chars().next();
    previous.is_none_or(|value| !is_inline_markdown_word_char(value))
        && next.is_some_and(|value| !value.is_whitespace())
}

fn should_close_underscore_marker(value: &str, index: usize) -> bool {
    let previous = value[..index].chars().next_back();
    let next = value[index + '_'.len_utf8()..].chars().next();
    previous.is_some_and(|value| !value.is_whitespace())
        && next.is_none_or(|value| !is_inline_markdown_word_char(value))
}

fn is_inline_markdown_word_char(value: char) -> bool {
    value.is_alphanumeric()
}

fn push_source_segment(
    output: &mut String,
    source_segments: &mut Vec<SourceSegment>,
    source: &str,
    source_start: usize,
    source_end: usize,
) {
    if source_start >= source_end {
        return;
    }
    let output_start = output.len();
    output.push_str(&source[source_start..source_end]);
    source_segments.push(SourceSegment {
        source_start,
        source_end,
        output_start,
    });
}

fn remap_highlights_with_segments(
    highlights: &[TextHighlight],
    source_segments: &[SourceSegment],
) -> Vec<TextHighlight> {
    let mut remapped = Vec::new();
    for highlight in highlights {
        for segment in source_segments {
            let start = highlight.start.max(segment.source_start);
            let end = highlight.end.min(segment.source_end);
            if start < end {
                remapped.push(TextHighlight {
                    start: segment
                        .output_start
                        .saturating_add(start.saturating_sub(segment.source_start)),
                    end: segment
                        .output_start
                        .saturating_add(end.saturating_sub(segment.source_start)),
                    kind: highlight.kind,
                });
            }
        }
    }
    merge_adjacent_highlights(remapped)
}

fn merge_adjacent_highlights(mut highlights: Vec<TextHighlight>) -> Vec<TextHighlight> {
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    let mut merged: Vec<TextHighlight> = Vec::new();
    for highlight in highlights {
        if let Some(last) = merged.last_mut()
            && last.kind == highlight.kind
            && last.end == highlight.start
        {
            last.end = highlight.end;
            continue;
        }
        merged.push(highlight);
    }
    merged
}

fn remap_emoji_slots_with_segments(
    emoji_slots: &[InlineEmojiSlot],
    source_segments: &[SourceSegment],
) -> Vec<InlineEmojiSlot> {
    emoji_slots
        .iter()
        .filter_map(|slot| {
            let slot_end = slot.byte_start.saturating_add(slot.byte_len);
            source_segments.iter().find_map(|segment| {
                (segment.source_start <= slot.byte_start && slot_end <= segment.source_end).then(
                    || InlineEmojiSlot {
                        byte_start: segment
                            .output_start
                            .saturating_add(slot.byte_start.saturating_sub(segment.source_start)),
                        byte_len: slot.byte_len,
                        display_width: slot.display_width,
                        url: slot.url.clone(),
                    },
                )
            })
        })
        .collect()
}
