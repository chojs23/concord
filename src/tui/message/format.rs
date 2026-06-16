use std::time::{SystemTime, UNIX_EPOCH};

mod attachments;
mod embed;
mod reactions;

pub(in crate::tui) use attachments::format_attachment_summary;
use attachments::format_attachment_summary_lines;
pub(in crate::tui) use embed::embed_color;
use embed::format_embed_lines;
pub(in crate::tui) use reactions::format_message_reaction_lines;
pub(crate) use reactions::{
    ReactionLayout, lay_out_reaction_chips_with_custom_emoji_images, reaction_line_spans,
};
#[cfg(test)]
pub(crate) use reactions::{lay_out_reaction_chips, reaction_line_test_spans};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, MessageMarker},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::time as message_time;
use crate::discord::{MessageKind, MessageSnapshotInfo, MessageState, PollInfo, ReplyInfo};
use crate::tui::{
    format::{
        InlineEmojiSlot, RenderedText, TextHighlight, TextHighlightKind, detected_url_ranges,
        truncate_display_width, truncate_text,
    },
    state::{DashboardState, ThreadSummary, discord_color},
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const COMMAND_BLUE: Color = Color::Rgb(88, 101, 242);
const COMMAND_USAGE_PREFIX: &str = "┌ ";
const SELF_REACTION: Color = Color::Yellow;
const INLINE_CODE: Color = Color::Rgb(255, 165, 0);
const THREAD_CARD_INDENT: &str = "  ";
const EDITED_MARKER: &str = " (edited)";
const MARKDOWN_QUOTE_PREFIX: &str = "▎ ";
const MARKDOWN_BULLET_PREFIX: &str = "• ";
pub(in crate::tui) const EMOJI_REACTION_IMAGE_WIDTH: u16 = 2;

#[derive(Clone)]
pub(in crate::tui) struct MessageContentLine {
    pub(in crate::tui) text: String,
    pub(in crate::tui) style: Style,
    mention_highlights: Vec<TextHighlight>,
    styled_prefixes: Vec<StyledPrefix>,
    pub(in crate::tui) image_slots: Vec<MessageContentImageSlot>,
}

#[derive(Clone, Copy)]
struct StyledPrefix {
    start: usize,
    len: usize,
    style: Style,
    patch_base: bool,
}

/// Per-line projection of [`InlineEmojiSlot`]: `col` is where the image
/// lands and `byte_start..byte_start+byte_len` is the visible placeholder the
/// renderer blanks once the image arrives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct MessageContentImageSlot {
    pub(in crate::tui) col: u16,
    pub(in crate::tui) byte_start: usize,
    pub(in crate::tui) byte_len: usize,
    pub(in crate::tui) display_width: u16,
    pub(in crate::tui) url: String,
}

impl MessageContentLine {
    pub(in crate::tui) fn plain(text: String) -> Self {
        Self {
            text,
            style: Style::default(),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
            image_slots: Vec::new(),
        }
    }

    fn styled_text(text: String, style: Style, mention_highlights: Vec<TextHighlight>) -> Self {
        Self {
            text,
            style,
            mention_highlights,
            styled_prefixes: Vec::new(),
            image_slots: Vec::new(),
        }
    }

    fn dim(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(DIM),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
            image_slots: Vec::new(),
        }
    }

    fn accent(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(ACCENT),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
            image_slots: Vec::new(),
        }
    }

    fn with_image_slots(mut self, slots: Vec<MessageContentImageSlot>) -> Self {
        self.image_slots = slots;
        self
    }

    fn styled_range(&mut self, start: usize, len: usize, style: Style) {
        let end = start.saturating_add(len).min(self.text.len());
        if start < end {
            self.styled_prefixes.push(StyledPrefix {
                start,
                len: end.saturating_sub(start),
                style,
                patch_base: false,
            });
        }
    }

    fn append_styled_suffix(&mut self, suffix: &str, style: Style) {
        let start = self.text.len();
        self.text.push_str(suffix);
        self.styled_range(start, suffix.len(), style);
    }

    pub(in crate::tui) fn spans(&self) -> Vec<Span<'static>> {
        let mut boundaries = vec![0, self.text.len()];
        for highlight in &self.mention_highlights {
            push_range_boundaries(
                &mut boundaries,
                highlight.start,
                highlight.end,
                self.text.len(),
            );
        }
        for prefix in &self.styled_prefixes {
            push_range_boundaries(
                &mut boundaries,
                prefix.start,
                prefix.start.saturating_add(prefix.len),
                self.text.len(),
            );
        }

        boundaries.sort_unstable();
        boundaries.dedup();

        boundaries
            .windows(2)
            .filter_map(|window| {
                let start = window[0];
                let end = window[1];
                (start < end).then(|| {
                    Span::styled(
                        self.text[start..end].to_owned(),
                        self.style_for_range(start, end),
                    )
                })
            })
            .collect()
    }

    fn style_for_range(&self, start: usize, end: usize) -> Style {
        let mut style = self.style;
        for prefix in self
            .styled_prefixes
            .iter()
            .filter(|prefix| prefix.contains(start, end))
        {
            if prefix.patch_base {
                style = style.patch(prefix.style);
            } else {
                style = prefix.style;
            }
        }

        if let Some(highlight) = self
            .mention_highlights
            .iter()
            .find(|highlight| highlight.start <= start && end <= highlight.end)
        {
            style = style.patch(mention_highlight_style(highlight.kind));
        }

        style
    }
}

struct LoadedEmojiReplacement {
    start: usize,
    end: usize,
    new_start: usize,
    new_len: usize,
}

struct WrappedTextLine {
    text: String,
    source_start: usize,
    source_end: usize,
    mention_highlights: Vec<TextHighlight>,
    image_slots: Vec<MessageContentImageSlot>,
}

struct SourceSegment {
    source_start: usize,
    source_end: usize,
    output_start: usize,
}

struct WrapBoundary {
    source_start: usize,
    byte_start: usize,
    width: usize,
    slot_count: usize,
}

struct InlineMarkdownText {
    rendered: RenderedText,
    styled_ranges: Vec<StyledPrefix>,
}

fn remap_loaded_emoji_offset(replacements: &[LoadedEmojiReplacement], position: usize) -> usize {
    let mut delta = 0isize;
    for replacement in replacements {
        if position < replacement.start {
            break;
        }
        if position < replacement.end {
            let inside = position.saturating_sub(replacement.start);
            return replacement
                .new_start
                .saturating_add(inside.min(replacement.new_len));
        }
        delta += replacement.new_len as isize - (replacement.end - replacement.start) as isize;
    }

    if delta < 0 {
        position.saturating_sub(delta.unsigned_abs())
    } else {
        position.saturating_add(delta as usize)
    }
}

impl StyledPrefix {
    fn contains(&self, start: usize, end: usize) -> bool {
        self.start <= start && end <= self.start.saturating_add(self.len)
    }
}

fn push_range_boundaries(boundaries: &mut Vec<usize>, start: usize, end: usize, text_len: usize) {
    let start = start.min(text_len);
    let end = end.min(text_len);
    if start < end {
        boundaries.push(start);
        boundaries.push(end);
    }
}

#[cfg(test)]
pub(in crate::tui) fn format_message_content(message: &MessageState, width: usize) -> String {
    format_message_content_lines(message, &DashboardState::new(), width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(in crate::tui) fn format_message_content_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let (mut lines, reaction_lines) = format_message_content_sections(message, state, width);
    lines.extend(reaction_lines);
    lines
}

pub(in crate::tui) fn format_message_content_lines_with_loaded_custom_emoji_urls(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    let (mut lines, reaction_lines) = format_message_content_sections_with_loaded_custom_emoji_urls(
        message,
        state,
        width,
        loaded_custom_emoji_urls,
    );
    lines.extend(reaction_lines);
    lines
}

pub(in crate::tui) fn format_message_content_sections(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> (Vec<MessageContentLine>, Vec<MessageContentLine>) {
    format_message_content_sections_with_loaded_custom_emoji_urls(message, state, width, &[])
}

pub(in crate::tui) fn format_message_content_sections_with_loaded_custom_emoji_urls(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> (Vec<MessageContentLine>, Vec<MessageContentLine>) {
    let attachment_summary_lines = if message.attachments.is_empty() {
        Vec::new()
    } else {
        format_attachment_summary_lines(&message.attachments)
    };
    let mut lines = Vec::new();

    if let Some(system_lines) = format_system_message_lines(message, state, width) {
        return (system_lines, Vec::new());
    }

    let renders_poll_card = message.reply.is_none() && message.poll.is_some();
    let chat_input_command_line = format_chat_input_command_line(message, state, width);
    if let Some(line) = chat_input_command_line.clone() {
        lines.push(line);
    }

    if let Some(line) = message
        .reply
        .as_ref()
        .map(|reply| format_reply_line(reply, message.guild_id, state, width))
    {
        lines.push(line);
    } else if let Some(poll) = message.poll.as_ref() {
        let content = display_text_with_stickers(
            message.content.as_deref(),
            &message.sticker_names,
        )
        .map(|value| {
            state.render_user_mentions_with_highlights(message.guild_id, &message.mentions, &value)
        });
        lines.extend(format_poll_lines(
            poll,
            content,
            width,
            loaded_custom_emoji_urls,
        ));
    } else if chat_input_command_line.is_none()
        && let Some(line) = format_message_kind_line(message.message_kind)
    {
        lines.push(line);
    }

    let standalone_content = (!renders_poll_card)
        .then(|| display_text_with_stickers(message.content.as_deref(), &message.sticker_names))
        .flatten();
    if let Some(value) = standalone_content {
        let rendered =
            state.render_user_mentions_with_highlights(message.guild_id, &message.mentions, &value);
        lines.extend(wrap_markdown_message_lines_with_loaded_custom_emoji_urls(
            state,
            rendered,
            width,
            Style::default(),
            loaded_custom_emoji_urls,
        ));
    }
    lines.extend(format_embed_lines(
        &message.embeds,
        message.content.as_deref(),
        state.show_custom_emoji(),
        width,
        loaded_custom_emoji_urls,
    ));
    for attachment in attachment_summary_lines {
        lines.push(MessageContentLine::accent(truncate_text(
            &attachment,
            width,
        )));
    }
    if let Some(snapshot) = message.forwarded_snapshots.first() {
        lines.extend(format_forwarded_snapshot(
            snapshot,
            state,
            width,
            loaded_custom_emoji_urls,
        ));
    }
    if lines.is_empty() {
        lines.push(MessageContentLine::plain(if message.content.is_some() {
            "<empty message>".to_owned()
        } else {
            "<message content unavailable>".to_owned()
        }));
    }

    if message.edited_timestamp.is_some() {
        append_edited_marker(&mut lines, width);
    }

    let reaction_lines =
        format_message_reaction_lines(&message.reactions, width, state.show_custom_emoji());
    (lines, reaction_lines)
}

fn append_edited_marker(lines: &mut Vec<MessageContentLine>, width: usize) {
    let marker_style = Style::default().fg(DIM).add_modifier(Modifier::ITALIC);
    let marker_width = EDITED_MARKER.width();
    if let Some(line) = lines.last_mut()
        && line.text.width().saturating_add(marker_width) <= width
    {
        line.append_styled_suffix(EDITED_MARKER, marker_style);
        return;
    }
    lines.push(MessageContentLine::styled_text(
        EDITED_MARKER.trim().to_owned(),
        marker_style,
        Vec::new(),
    ));
}

fn wrap_rendered_text_lines_with_loaded_custom_emoji_urls(
    rendered: RenderedText,
    width: usize,
    style: Style,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    let rendered =
        rendered_text_with_loaded_custom_emoji_placeholders(rendered, loaded_custom_emoji_urls);
    wrap_rendered_text_lines(rendered, width, style)
}

fn wrap_rendered_text_lines(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    wrap_rendered_text_lines_with_styled_ranges(rendered, width, style, &[])
}

fn wrap_rendered_text_lines_with_styled_ranges(
    rendered: RenderedText,
    width: usize,
    style: Style,
    styled_ranges: &[StyledPrefix],
) -> Vec<MessageContentLine> {
    let rendered = rendered_text_with_url_highlights(rendered);
    wrap_text_with_metadata(
        &rendered.text,
        &rendered.highlights,
        &rendered.emoji_slots,
        width,
    )
    .into_iter()
    .map(|wrapped| {
        let mut line =
            MessageContentLine::styled_text(wrapped.text, style, wrapped.mention_highlights)
                .with_image_slots(wrapped.image_slots);
        for range in
            styled_ranges_for_range(styled_ranges, wrapped.source_start, wrapped.source_end)
        {
            line.styled_prefixes.push(range);
        }
        line
    })
    .collect()
}

fn wrap_markdown_message_lines_with_loaded_custom_emoji_urls(
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

    if let Some((prefix_len, heading_style)) = markdown_heading(&rendered.text) {
        let prefix = rendered.text[..prefix_len].to_owned();
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            heading_style,
            &prefix,
            Style::default().fg(DIM),
        );
    }

    if let Some(prefix_len) = markdown_quote_prefix_len(&rendered.text) {
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            style.fg(DIM),
            MARKDOWN_QUOTE_PREFIX,
            Style::default().fg(DIM),
        );
    }

    if let Some(prefix_len) = markdown_bullet_prefix_len(&rendered.text) {
        let content = rendered_text_without_prefix(rendered, prefix_len);
        return wrap_prefixed_markdown_line(
            content,
            width,
            style,
            MARKDOWN_BULLET_PREFIX,
            Style::default().fg(DIM),
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
    MessageContentLine::dim(format!("{left}{inner}{right}"))
}

fn code_box_body_line(regions: Vec<(Style, String)>, content_width: usize) -> MessageContentLine {
    let mut line =
        MessageContentLine::styled_text("│ ".to_owned(), Style::default().fg(DIM), Vec::new());
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
    line.append_styled_suffix(" │", Style::default().fg(DIM));
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

fn markdown_heading(value: &str) -> Option<(usize, Style)> {
    if value.starts_with("# ") {
        Some((
            "# ".len(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
    } else if value.starts_with("## ") {
        Some((
            "## ".len(),
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ))
    } else if value.starts_with("### ") {
        Some(("### ".len(), Style::default().add_modifier(Modifier::BOLD)))
    } else {
        None
    }
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
    Style::default().fg(Color::White)
}

fn inline_code_style() -> Style {
    Style::default().fg(INLINE_CODE)
}

fn rendered_text_without_prefix(rendered: RenderedText, prefix_len: usize) -> RenderedText {
    rendered_text_slice(&rendered, prefix_len, rendered.text.len())
}

fn rendered_text_slice(rendered: &RenderedText, start: usize, end: usize) -> RenderedText {
    let start = start.min(rendered.text.len());
    let end = end.min(rendered.text.len());
    let text = rendered.text[start..end].to_owned();
    let highlights = highlights_for_range(&rendered.highlights, start, end);
    let emoji_slots = rendered
        .emoji_slots
        .iter()
        .filter_map(|slot| {
            let slot_end = slot.byte_start.saturating_add(slot.byte_len);
            (start <= slot.byte_start && slot_end <= end).then(|| InlineEmojiSlot {
                byte_start: slot.byte_start.saturating_sub(start),
                byte_len: slot.byte_len,
                display_width: slot.display_width,
                url: slot.url.clone(),
            })
        })
        .collect();

    RenderedText {
        text,
        highlights,
        emoji_slots,
    }
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
    ["`", "***", "**", "*", "_"]
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
    } else if rest.starts_with('_') && should_open_underscore_marker(value, cursor) {
        Some(("_", Style::default().add_modifier(Modifier::ITALIC)))
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
        if let Some(last) = merged.last_mut() {
            if last.kind == highlight.kind && last.end == highlight.start {
                last.end = highlight.end;
                continue;
            }
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

fn rendered_text_with_url_highlights(mut rendered: RenderedText) -> RenderedText {
    rendered.highlights.extend(url_highlights(&rendered.text));
    rendered
}

fn url_highlights(value: &str) -> Vec<TextHighlight> {
    detected_url_ranges(value)
        .into_iter()
        .map(|(start, end)| TextHighlight {
            start,
            end,
            kind: TextHighlightKind::Url,
        })
        .collect()
}

fn rendered_text_with_loaded_custom_emoji_placeholders(
    rendered: RenderedText,
    loaded_custom_emoji_urls: &[String],
) -> RenderedText {
    if loaded_custom_emoji_urls.is_empty() || rendered.emoji_slots.is_empty() {
        return rendered;
    }

    let RenderedText {
        text,
        highlights,
        emoji_slots,
    } = rendered;
    let mut slots: Vec<usize> = (0..emoji_slots.len()).collect();
    slots.sort_by_key(|index| emoji_slots[*index].byte_start);

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut replacements = Vec::new();
    let mut slot_updates = vec![None; emoji_slots.len()];

    for index in slots {
        let slot = &emoji_slots[index];
        let start = slot.byte_start;
        let end = slot.byte_start.saturating_add(slot.byte_len);
        if start < cursor
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            continue;
        }

        output.push_str(&text[cursor..start]);
        let new_start = output.len();
        if loaded_custom_emoji_urls.iter().any(|url| url == &slot.url) {
            let placeholder = " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH));
            output.push_str(&placeholder);
            replacements.push(LoadedEmojiReplacement {
                start,
                end,
                new_start,
                new_len: placeholder.len(),
            });
            slot_updates[index] = Some(InlineEmojiSlot {
                byte_start: new_start,
                byte_len: placeholder.len(),
                display_width: EMOJI_REACTION_IMAGE_WIDTH,
                url: slot.url.clone(),
            });
        } else {
            output.push_str(&text[start..end]);
            slot_updates[index] = Some(InlineEmojiSlot {
                byte_start: new_start,
                byte_len: slot.byte_len,
                display_width: slot.display_width,
                url: slot.url.clone(),
            });
        }
        cursor = end;
    }

    if replacements.is_empty() {
        return RenderedText {
            text,
            highlights,
            emoji_slots,
        };
    }

    output.push_str(&text[cursor..]);
    let highlights = highlights
        .into_iter()
        .map(|highlight| TextHighlight {
            start: remap_loaded_emoji_offset(&replacements, highlight.start),
            end: remap_loaded_emoji_offset(&replacements, highlight.end),
            kind: highlight.kind,
        })
        .collect();
    let emoji_slots = emoji_slots
        .into_iter()
        .enumerate()
        .map(|(index, slot)| {
            slot_updates[index]
                .clone()
                .unwrap_or_else(|| InlineEmojiSlot {
                    byte_start: remap_loaded_emoji_offset(&replacements, slot.byte_start),
                    byte_len: slot.byte_len,
                    display_width: slot.display_width,
                    url: slot.url,
                })
        })
        .collect();

    RenderedText {
        text: output,
        highlights,
        emoji_slots,
    }
}

fn rendered_text_line(rendered: RenderedText, style: Style) -> MessageContentLine {
    let image_slots = emoji_slots_to_image_slots(&rendered.text, &rendered.emoji_slots);
    MessageContentLine::styled_text(rendered.text, style, rendered.highlights)
        .with_image_slots(image_slots)
}

fn prepend_rendered_text(prefix: String, mut rendered: RenderedText) -> RenderedText {
    let shift = prefix.len();
    for highlight in &mut rendered.highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    for slot in &mut rendered.emoji_slots {
        slot.byte_start = slot.byte_start.saturating_add(shift);
    }
    rendered.text.insert_str(0, &prefix);
    rendered
}

fn truncate_rendered_text(rendered: RenderedText, limit: usize) -> RenderedText {
    let mut chars = rendered.text.char_indices();
    let cutoff = match chars.nth(limit) {
        Some((index, _)) => index,
        None => return rendered,
    };
    let mut text = rendered.text[..cutoff].to_owned();
    text.push_str("...");
    let highlights = rendered
        .highlights
        .into_iter()
        .filter(|highlight| highlight.start < cutoff)
        .map(|highlight| TextHighlight {
            start: highlight.start,
            end: highlight.end.min(cutoff),
            kind: highlight.kind,
        })
        .collect();
    let emoji_slots = rendered
        .emoji_slots
        .into_iter()
        .filter(|slot| slot.byte_start.saturating_add(slot.byte_len) <= cutoff)
        .collect();
    RenderedText {
        text,
        highlights,
        emoji_slots,
    }
}

fn prefix_message_content_line(prefix: &str, mut line: MessageContentLine) -> MessageContentLine {
    let byte_shift = prefix.len();
    let col_shift = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
    for highlight in &mut line.mention_highlights {
        highlight.start = highlight.start.saturating_add(byte_shift);
        highlight.end = highlight.end.saturating_add(byte_shift);
    }
    for styled_prefix in &mut line.styled_prefixes {
        styled_prefix.start = styled_prefix.start.saturating_add(byte_shift);
    }
    for slot in &mut line.image_slots {
        slot.col = slot.col.saturating_add(col_shift);
        slot.byte_start = slot.byte_start.saturating_add(byte_shift);
    }
    line.text.insert_str(0, prefix);
    line
}

/// Single-line variant of slot distribution for places where wrapping is skipped.
fn emoji_slots_to_image_slots(
    text: &str,
    emoji_slots: &[InlineEmojiSlot],
) -> Vec<MessageContentImageSlot> {
    if emoji_slots.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(emoji_slots.len());
    for slot in emoji_slots {
        let prefix = text.get(..slot.byte_start).unwrap_or("");
        let col = u16::try_from(prefix.width()).unwrap_or(u16::MAX);
        output.push(MessageContentImageSlot {
            col,
            byte_start: slot.byte_start,
            byte_len: slot.byte_len,
            display_width: slot.display_width,
            url: slot.url.clone(),
        });
    }
    output
}

fn prefix_message_content_line_without_underline(
    prefix: &str,
    line: MessageContentLine,
) -> MessageContentLine {
    let style = line.style.remove_modifier(Modifier::UNDERLINED);
    prefix_message_content_line_with_style(prefix, style, line)
}

fn prefix_message_content_line_with_style(
    prefix: &str,
    style: Style,
    mut line: MessageContentLine,
) -> MessageContentLine {
    line = prefix_message_content_line(prefix, line);
    line.styled_prefixes.push(StyledPrefix {
        start: 0,
        len: prefix.len(),
        style,
        patch_base: false,
    });
    line
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

fn wrap_text_line_with_styles(
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

fn wrap_text_with_metadata(
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
            lines.push(WrappedTextLine {
                text: String::new(),
                source_start: line_start,
                source_end: line_start,
                mention_highlights: Vec::new(),
                image_slots: Vec::new(),
            });
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
            // First grapheme of a slot reserves the full `:name:` width.
            let effective_width = match slot_at_grapheme {
                Some(slot) => slot.display_width as usize,
                None => grapheme_width,
            };
            if current_width > 0
                && effective_width > 0
                && current_width.saturating_add(effective_width) > width
            {
                if grapheme_is_separator {
                    let text = std::mem::take(&mut current);
                    let line_slots = std::mem::take(&mut current_slots);
                    lines.push(WrappedTextLine {
                        text,
                        source_start: current_start,
                        source_end: current_end,
                        mention_highlights: highlights_for_range(
                            highlights,
                            current_start,
                            current_end,
                        ),
                        image_slots: line_slots,
                    });
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
                    lines.push(WrappedTextLine {
                        text,
                        source_start: current_start,
                        source_end: boundary.source_start,
                        mention_highlights: highlights_for_range(
                            highlights,
                            current_start,
                            boundary.source_start,
                        ),
                        image_slots: current_slots,
                    });
                    std::mem::swap(&mut current, &mut next);
                    current_slots = next_slots;
                    current_width = current_width.saturating_sub(boundary.width);
                    current_start = boundary.source_start;
                } else {
                    let text = std::mem::take(&mut current);
                    let line_slots = std::mem::take(&mut current_slots);
                    lines.push(WrappedTextLine {
                        text,
                        source_start: current_start,
                        source_end: current_end,
                        mention_highlights: highlights_for_range(
                            highlights,
                            current_start,
                            current_end,
                        ),
                        image_slots: line_slots,
                    });
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
        lines.push(WrappedTextLine {
            text: current,
            source_start: current_start,
            source_end: current_end,
            mention_highlights: highlights_for_range(highlights, current_start, current_end),
            image_slots: current_slots,
        });
        line_start = line_start.saturating_add(line.len()).saturating_add(1);
    }
    lines
}

fn styled_ranges_for_range(
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

fn highlights_for_range(
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

fn format_poll_lines(
    poll: &PollInfo,
    content: Option<RenderedText>,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    let inner_width = poll_card_inner_width(width);
    let helper = if poll.allow_multiselect {
        "Select one or more answers"
    } else {
        "Select one answer"
    };
    let mut lines = vec![MessageContentLine::accent(poll_box_border('╭', '╮', width))];
    lines.push(poll_box_line(
        MessageContentLine::plain(truncate_display_width(&poll.question, inner_width)),
        inner_width,
    ));
    if let Some(content) = content {
        lines.extend(
            wrap_rendered_text_lines_with_loaded_custom_emoji_urls(
                content,
                inner_width,
                Style::default(),
                loaded_custom_emoji_urls,
            )
            .into_iter()
            .map(|line| poll_box_line(line, inner_width)),
        );
    }
    lines.push(poll_box_line(
        MessageContentLine::dim(truncate_display_width(helper, inner_width)),
        inner_width,
    ));
    let counted_votes = poll
        .answers
        .iter()
        .filter_map(|answer| answer.vote_count)
        .sum::<u64>();
    let total_votes = poll.total_votes.unwrap_or(counted_votes);
    lines.extend(poll.answers.iter().enumerate().map(|(index, answer)| {
        poll_box_line(
            MessageContentLine::plain(truncate_display_width(
                &format_poll_answer(index, answer, total_votes),
                inner_width,
            )),
            inner_width,
        )
    }));
    lines.push(poll_box_line(
        MessageContentLine::dim(truncate_display_width(
            &format_poll_footer(poll, total_votes),
            inner_width,
        )),
        inner_width,
    ));
    lines.push(MessageContentLine::accent(poll_box_border('╰', '╯', width)));
    lines
}

pub(crate) fn poll_card_inner_width(width: usize) -> usize {
    poll_box_width(width).saturating_sub(4).max(1)
}

fn poll_box_width(width: usize) -> usize {
    width.clamp(4, 72)
}

pub(in crate::tui) fn poll_box_border(left: char, right: char, width: usize) -> String {
    let width = poll_box_width(width);
    format!("{left}{}{right}", "─".repeat(width.saturating_sub(2)))
}

fn poll_box_line(mut line: MessageContentLine, inner_width: usize) -> MessageContentLine {
    let prefix = "│ ";
    let suffix = " │";
    let padding = inner_width.saturating_sub(line.text.width());
    let shift = prefix.len();
    for highlight in &mut line.mention_highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    line.text = format!("{prefix}{}{}{suffix}", line.text, " ".repeat(padding));
    line
}

fn format_poll_result_lines(poll: Option<&PollInfo>, width: usize) -> Vec<MessageContentLine> {
    let Some(poll) = poll else {
        return vec![
            MessageContentLine::accent(truncate_text("Poll results", width)),
            MessageContentLine::dim(truncate_text("Result details unavailable", width)),
        ];
    };
    let mut lines = vec![
        MessageContentLine::accent(truncate_text("Poll results", width)),
        MessageContentLine::plain(truncate_text(&poll.question, width)),
    ];
    if let Some(winner) = poll.answers.first() {
        let votes = winner
            .vote_count
            .map(|count| format!(" with {count} votes"))
            .unwrap_or_default();
        lines.push(MessageContentLine::plain(truncate_text(
            &format!("Winner: {}{votes}", winner.text),
            width,
        )));
    } else {
        lines.push(MessageContentLine::dim(truncate_text(
            "No winning answer recorded",
            width,
        )));
    }
    let counted_votes = poll
        .answers
        .iter()
        .filter_map(|answer| answer.vote_count)
        .sum::<u64>();
    let total_votes = poll
        .total_votes
        .or_else(|| (counted_votes > 0).then_some(counted_votes));
    if let Some(total_votes) = total_votes {
        let vote_label = if total_votes == 1 { "vote" } else { "votes" };
        lines.push(MessageContentLine::dim(truncate_text(
            &format!("{total_votes} total {vote_label} · Final results"),
            width,
        )));
    }
    lines
}

fn format_poll_answer(
    index: usize,
    answer: &crate::discord::PollAnswerInfo,
    total_votes: u64,
) -> String {
    let marker = if answer.me_voted { "◉" } else { "◯" };
    let results = answer.vote_count.map(|count| {
        let percent = count
            .saturating_mul(100)
            .checked_div(total_votes)
            .unwrap_or(0);
        format!("  {count} votes  {percent}%")
    });
    match results {
        Some(results) => format!("  {marker} {}. {}{results}", index + 1, answer.text),
        None => format!("  {marker} {}. {}", index + 1, answer.text),
    }
}

fn format_poll_footer(poll: &PollInfo, total_votes: u64) -> String {
    let vote_label = if total_votes == 1 { "vote" } else { "votes" };
    match poll.results_finalized {
        Some(true) => format!("{total_votes} {vote_label} · Final results"),
        Some(false) => format!("{total_votes} {vote_label} · Results may still change"),
        None => "Results not available yet".to_owned(),
    }
}

fn format_reply_line(
    reply: &ReplyInfo,
    guild_id: Option<Id<GuildMarker>>,
    state: &DashboardState,
    width: usize,
) -> MessageContentLine {
    let content = display_text_with_stickers(reply.content.as_deref(), &reply.sticker_names)
        .unwrap_or_else(|| "<empty message>".to_owned());
    let content = state.render_user_mentions_with_highlights(guild_id, &reply.mentions, &content);
    let content = prepend_rendered_text(format!("╭─ {} : ", reply.author), content);
    rendered_text_line(
        truncate_rendered_text(content, width),
        Style::default().fg(DIM),
    )
}

fn display_text_with_stickers(content: Option<&str>, sticker_names: &[String]) -> Option<String> {
    let content = content.filter(|value| !value.is_empty());
    let stickers = sticker_display_text(sticker_names);
    match (content, stickers) {
        (Some(content), Some(stickers)) => Some(format!("{content}\n{stickers}")),
        (Some(content), None) => Some(content.to_owned()),
        (None, Some(stickers)) => Some(stickers),
        (None, None) => None,
    }
}

fn sticker_display_text(sticker_names: &[String]) -> Option<String> {
    (!sticker_names.is_empty()).then(|| {
        sticker_names
            .iter()
            .map(|name| format!("[Sticker: {name}]"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn format_message_kind_line(message_kind: MessageKind) -> Option<MessageContentLine> {
    if message_kind.is_regular() {
        return None;
    }

    let label = match message_kind.code() {
        7 => "joined the server",
        19 => "↳ Reply",
        _ => message_kind
            .known_label()
            .unwrap_or("<unsupported message type>"),
    };

    Some(MessageContentLine::dim(label.to_owned()))
}

fn format_system_message_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Option<Vec<MessageContentLine>> {
    match message.message_kind.code() {
        8 => Some(vec![MessageContentLine::accent(truncate_text(
            &format!("{} boosted the server", message.author),
            width,
        ))]),
        9..=11 => {
            let tier = message.message_kind.code() - 8;
            Some(vec![MessageContentLine::accent(truncate_text(
                &format!("{} boosted the server to Level {tier}", message.author),
                width,
            ))])
        }
        18 => Some(format_thread_created_lines(message, state, width)),
        21 => Some(format_thread_starter_lines(message, state, width)),
        46 => Some(format_poll_result_lines(message.poll.as_ref(), width)),
        _ => None,
    }
}

fn format_chat_input_command_line(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Option<MessageContentLine> {
    if message.message_kind.code() != 20 {
        return None;
    }
    let interaction = message.interaction.as_ref()?;
    let command = interaction
        .command_name
        .as_deref()
        .map(format_command_name)
        .unwrap_or_else(|| "a command".to_owned());
    let user_start = COMMAND_USAGE_PREFIX.len();
    let command_start = user_start + interaction.user.len() + " used ".len();
    let text = truncate_text(
        &format!("{COMMAND_USAGE_PREFIX}{} used {command}", interaction.user),
        width,
    );
    let mut line = MessageContentLine::dim(text);
    let user_color = interaction
        .user_id
        .and_then(|user_id| state.message_user_role_color(message, user_id));
    line.styled_range(
        user_start,
        interaction.user.len(),
        Style::default()
            .fg(discord_color(user_color, Color::White))
            .add_modifier(Modifier::DIM),
    );
    line.styled_range(
        command_start,
        command.len(),
        Style::default()
            .fg(COMMAND_BLUE)
            .add_modifier(Modifier::DIM),
    );
    Some(line)
}

fn format_command_name(command_name: &str) -> String {
    let command_name = command_name.trim();
    if command_name.starts_with('/') {
        command_name.to_owned()
    } else {
        format!("/{command_name}")
    }
}

fn format_thread_created_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let summary = state.thread_summary_for_message(message);
    let thread_name = summary
        .as_ref()
        .map(|summary| summary.name.as_str())
        .or_else(|| message.content.as_deref().filter(|value| !value.is_empty()))
        .unwrap_or("thread");
    let mut lines = vec![format_thread_created_starter_line(
        message,
        state,
        thread_name,
        width,
    )];
    lines.extend(format_thread_card_lines(
        thread_name,
        summary.as_ref(),
        message.id,
        width,
    ));
    lines
}

fn format_thread_created_starter_line(
    message: &MessageState,
    state: &DashboardState,
    thread_name: &str,
    width: usize,
) -> MessageContentLine {
    let author_style = Style::default()
        .fg(discord_color(
            state.message_author_role_color(message),
            Color::White,
        ))
        .bold();
    let thread_style = Style::default().fg(ACCENT).bold();
    let base_style = Style::default().fg(Color::White);

    let author = message.author.as_str();
    let (starter, thread_start) = if thread_name == "thread" {
        (format!("{author} started a thread."), None)
    } else {
        let before_thread = format!("{author} started ");
        let thread_start = before_thread.len();
        (
            format!("{before_thread}{thread_name} thread."),
            Some(thread_start),
        )
    };
    let mut line = MessageContentLine::plain(truncate_display_width(&starter, width));
    line.style = base_style;
    line.styled_range(0, author.len(), author_style);
    if let Some(thread_start) = thread_start {
        line.styled_range(thread_start, thread_name.len(), thread_style);
    }
    line
}

fn format_thread_card_lines(
    thread_name: &str,
    summary: Option<&ThreadSummary>,
    message_id: Id<MessageMarker>,
    width: usize,
) -> Vec<MessageContentLine> {
    let card_width = thread_card_width(width);
    let inner_width = thread_card_inner_width(width);
    vec![
        MessageContentLine::accent(thread_card_border('╭', '╮', width)),
        thread_card_line(
            format_thread_card_title_line(thread_name, summary, inner_width),
            inner_width,
        ),
        thread_card_line(
            format_thread_latest_line(summary, message_id, inner_width),
            inner_width,
        ),
        MessageContentLine::accent(format!(
            "{THREAD_CARD_INDENT}╰{}╯",
            "─".repeat(card_width.saturating_sub(2))
        )),
    ]
}

fn format_thread_card_title_line(
    thread_name: &str,
    summary: Option<&ThreadSummary>,
    width: usize,
) -> MessageContentLine {
    let Some(count_label) = summary.and_then(thread_message_count_label) else {
        return MessageContentLine::accent(truncate_display_width(thread_name, width));
    };

    let count_width = count_label.width();
    if count_width.saturating_add(2) >= width {
        return MessageContentLine::accent(truncate_display_width(thread_name, width));
    }

    let name_width = width.saturating_sub(count_width).saturating_sub(2);
    let name = truncate_display_width(thread_name, name_width);
    let padding = width
        .saturating_sub(name.width())
        .saturating_sub(count_width);
    MessageContentLine::accent(format!("{name}{}{count_label}", " ".repeat(padding)))
}

fn thread_message_count_label(summary: &ThreadSummary) -> Option<String> {
    summary
        .message_count
        .or(summary.total_message_sent)
        .map(|count| {
            let label = if count == 1 { "message" } else { "messages" };
            format!("{count} {label}")
        })
}

fn thread_card_width(width: usize) -> usize {
    width
        .saturating_sub(THREAD_CARD_INDENT.width())
        .clamp(4, 72)
}

fn thread_card_inner_width(width: usize) -> usize {
    thread_card_width(width).saturating_sub(4).max(1)
}

fn thread_card_border(left: char, right: char, width: usize) -> String {
    let card_width = thread_card_width(width);
    format!(
        "{THREAD_CARD_INDENT}{left}{}{right}",
        "─".repeat(card_width.saturating_sub(2))
    )
}

fn thread_card_line(mut line: MessageContentLine, inner_width: usize) -> MessageContentLine {
    let prefix = format!("{THREAD_CARD_INDENT}│ ");
    let suffix = " │";
    let padding = inner_width.saturating_sub(line.text.width());
    line.text = format!("{prefix}{}{}{suffix}", line.text, " ".repeat(padding));
    line
}

fn format_thread_latest_line(
    summary: Option<&ThreadSummary>,
    message_id: Id<MessageMarker>,
    width: usize,
) -> MessageContentLine {
    let mut metadata = Vec::new();
    if let Some(summary) = summary {
        let mut statuses = Vec::new();
        let latest_message_id = summary.latest_message_id.unwrap_or(message_id);
        let age = format_message_relative_age(latest_message_id);
        if summary.archived == Some(true) {
            statuses.push("archived".to_owned());
        }
        if summary.locked == Some(true) {
            statuses.push("locked".to_owned());
        }
        let suffix = if statuses.is_empty() {
            age
        } else {
            format!("{age} · {}", statuses.join(" · "))
        };
        if let Some(preview) = summary.latest_message_preview.as_ref() {
            return MessageContentLine::dim(format_latest_message_preview(
                &preview.author,
                &preview.content,
                &suffix,
                width,
            ));
        }
        metadata.push(suffix);
    } else {
        metadata.push(format_message_relative_age(message_id));
        metadata.push("Thread details unavailable".to_owned());
    }

    MessageContentLine::dim(truncate_display_width(&metadata.join(" · "), width))
}

fn format_latest_message_preview(
    author: &str,
    content: &str,
    suffix: &str,
    width: usize,
) -> String {
    let prefix = format!("{author} ");
    let suffix = format!(" {suffix}");
    if prefix.width().saturating_add(suffix.width()) >= width {
        return truncate_display_width(&format!("{author} {content}{suffix}"), width);
    }

    let content_width = width
        .saturating_sub(prefix.width())
        .saturating_sub(suffix.width());
    let content = truncate_display_width(content, content_width.max(1));
    format!("{prefix}{content}{suffix}")
}

pub(in crate::tui) fn format_message_relative_age(message_id: Id<MessageMarker>) -> String {
    let created = message_time::message_unix_millis(message_id);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(created);
    let seconds = now.saturating_sub(created) / 1000;
    format_relative_seconds(seconds)
}

fn format_relative_seconds(seconds: u64) -> String {
    if seconds < 60 {
        return "just now".to_owned();
    }

    let minutes = seconds / 60;
    if minutes < 60 {
        return format_relative_unit(minutes, "minute");
    }

    let hours = minutes / 60;
    if hours < 24 {
        return format_relative_unit(hours, "hour");
    }

    let days = hours / 24;
    if days < 30 {
        return format_relative_unit(days, "day");
    }

    let months = days / 30;
    if months < 12 {
        return format_relative_unit(months, "month");
    }

    format_relative_unit((days / 365).max(1), "year")
}

fn format_relative_unit(value: u64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{suffix} ago")
}

fn format_thread_starter_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let mut lines = vec![MessageContentLine::accent(truncate_text(
        "Thread starter message",
        width,
    ))];
    if let Some(reply) = message.reply.as_ref() {
        lines.push(format_reply_line(reply, message.guild_id, state, width));
    } else {
        lines.push(MessageContentLine::dim(truncate_text(
            "Started from an unavailable message",
            width,
        )));
    }
    lines
}

fn format_forwarded_snapshot(
    snapshot: &MessageSnapshotInfo,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    let attachment_summary_lines = if snapshot.attachments.is_empty() {
        Vec::new()
    } else {
        format_attachment_summary_lines(&snapshot.attachments)
    };
    let mut lines = vec![MessageContentLine::plain("↱ Forwarded".to_owned())];
    if let Some(content) =
        display_text_with_stickers(snapshot.content.as_deref(), &snapshot.sticker_names)
    {
        let content_width = width.saturating_sub(2).max(1);
        let content = state.render_user_mentions_with_highlights(
            state.forwarded_snapshot_mention_guild_id(snapshot),
            &snapshot.mentions,
            &content,
        );
        lines.extend(
            wrap_rendered_text_lines_with_loaded_custom_emoji_urls(
                content,
                content_width,
                Style::default(),
                loaded_custom_emoji_urls,
            )
            .into_iter()
            .map(|line| prefix_message_content_line_without_underline("│ ", line)),
        );
    }
    for attachment in attachment_summary_lines {
        lines.push(MessageContentLine::accent(truncate_text(
            &format!("│ {attachment}"),
            width,
        )));
    }
    lines.extend(
        format_embed_lines(
            &snapshot.embeds,
            snapshot.content.as_deref(),
            state.show_custom_emoji(),
            width.saturating_sub(2).max(1),
            loaded_custom_emoji_urls,
        )
        .into_iter()
        .map(|line| prefix_message_content_line_without_underline("│ ", line)),
    );
    if lines.len() == 1 {
        lines.push(MessageContentLine::plain("│ <empty message>".to_owned()));
    }
    let mut metadata = Vec::new();
    if let Some(channel_id) = snapshot.source_channel_id {
        metadata.push(state.channel_label(channel_id));
    }
    if let Some(timestamp) = snapshot.timestamp.as_deref() {
        metadata.push(format_forwarded_time(timestamp));
    }
    if !metadata.is_empty() {
        lines.push(MessageContentLine::dim(truncate_text(
            &format!("│ {}", metadata.join(" · ")),
            width,
        )));
    }

    lines
}

fn format_forwarded_time(timestamp: &str) -> String {
    timestamp
        .split_once('T')
        .and_then(|(_, time)| time.get(0..5))
        .unwrap_or(timestamp)
        .to_owned()
}

pub(in crate::tui) fn mention_highlight_style(kind: TextHighlightKind) -> Style {
    match kind {
        // The current user got pinged, so match Discord's gold highlight.
        TextHighlightKind::SelfMention => Style::default()
            .bg(Color::Rgb(92, 76, 35))
            .fg(Color::Yellow),
        // Someone else was pinged, so use Discord's softer blue tint.
        TextHighlightKind::OtherMention => Style::default()
            .bg(Color::Rgb(40, 50, 92))
            .fg(Color::Rgb(193, 206, 247)),
        TextHighlightKind::Url => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_content_line_spans_combine_prefix_and_mention_styles() {
        let mention_start = ">> hello ".len();
        let line = MessageContentLine {
            text: ">> hello @alice".to_owned(),
            style: Style::default().add_modifier(Modifier::UNDERLINED),
            mention_highlights: vec![TextHighlight {
                start: mention_start,
                end: mention_start + "@alice".len(),
                kind: TextHighlightKind::SelfMention,
            }],
            styled_prefixes: vec![StyledPrefix {
                start: 0,
                len: ">> ".len(),
                style: Style::default().fg(Color::Red),
                patch_base: false,
            }],
            image_slots: Vec::new(),
        };

        let spans = line.spans();

        assert_eq!(spans[0].content.as_ref(), ">> ");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(!spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(spans[1].content.as_ref(), "hello ");
        assert!(spans[1].style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(spans[2].content.as_ref(), "@alice");
        assert!(spans[2].style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(
            spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn wrap_distributes_emoji_slots_per_line_with_correct_columns() {
        // Two `:e:` placeholders (each 3 cells wide) at byte offsets 2 and 7.
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
        // Width 4 cannot fit "ab" + 3-cell ":e:" on one line, so the emoji wraps.
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

    #[test]
    fn relative_age_labels_use_expected_boundaries() {
        assert_eq!(format_relative_seconds(0), "just now");
        assert_eq!(format_relative_seconds(59), "just now");
        assert_eq!(format_relative_seconds(60), "1 minute ago");
        assert_eq!(format_relative_seconds(2 * 60), "2 minutes ago");
        assert_eq!(format_relative_seconds(59 * 60), "59 minutes ago");
        assert_eq!(format_relative_seconds(60 * 60), "1 hour ago");
        assert_eq!(format_relative_seconds(24 * 60 * 60), "1 day ago");
        assert_eq!(format_relative_seconds(30 * 24 * 60 * 60), "1 month ago");
        assert_eq!(format_relative_seconds(365 * 24 * 60 * 60), "1 year ago");
    }
}
