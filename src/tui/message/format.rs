//! Message content assembly. Turns a [`MessageState`] into styled
//! [`MessageContentLine`]s, delegating markdown, wrapping, and the
//! per-feature renderers to the submodules below.

mod attachments;
mod embed;
mod markdown;
mod polls;
mod reactions;
mod system;
mod wrap;

pub(in crate::tui) use attachments::format_attachment_summary;
use attachments::format_attachment_summary_lines;
pub(in crate::tui) use embed::embed_color;
use embed::format_embed_lines;
use markdown::wrap_markdown_message_lines_with_loaded_custom_emoji_urls;
use polls::format_poll_lines;
#[cfg(test)]
pub(in crate::tui) use polls::poll_box_border;
#[cfg(test)]
pub(crate) use polls::poll_card_inner_width;
pub(in crate::tui) use reactions::format_message_reaction_lines;
pub(crate) use reactions::{
    ReactionLayout, lay_out_reaction_chips_with_custom_emoji_images, reaction_line_spans,
};
#[cfg(test)]
pub(crate) use reactions::{lay_out_reaction_chips, reaction_line_test_spans};
pub(in crate::tui) use system::format_message_relative_age;
use system::{
    format_chat_input_command_line, format_forwarded_snapshot, format_message_kind_line,
    format_system_message_lines,
};
pub(in crate::tui) use wrap::wrap_text_lines;
use wrap::{highlights_for_range, styled_ranges_for_range, wrap_text_with_metadata};

use crate::discord::ids::{Id, marker::GuildMarker};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::discord::{MessageState, ReplyInfo};
use crate::tui::{
    state::DashboardState,
    text::{
        InlineEmojiSlot, RenderedText, TextHighlight, TextHighlightKind, detected_url_ranges,
        truncate_text,
    },
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const SELF_REACTION: Color = Color::Yellow;
const EDITED_MARKER: &str = " (edited)";
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
        Self::styled_text(text, Style::default(), Vec::new())
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
        Self::styled_text(text, Style::default().fg(DIM), Vec::new())
    }

    fn accent(text: String) -> Self {
        Self::styled_text(text, Style::default().fg(ACCENT), Vec::new())
    }

    /// Wrap a pre-styled [`Line`] as a [`MessageContentLine`], concatenating the
    /// span text and preserving each span's style as a byte-range prefix so
    /// [`Self::spans`] reproduces the original styling.
    pub(in crate::tui) fn from_line(line: Line<'static>) -> Self {
        let mut content = Self::plain(String::new());
        for span in line.spans {
            let style = span.style;
            content.append_styled_suffix(&span.content, style);
        }
        content
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

#[cfg(test)]
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
        let content =
            display_text_with_stickers(message.content.as_deref(), &message.sticker_names).map(
                |value| {
                    state.render_user_mentions_with_highlights(
                        message.guild_id,
                        &message.mentions,
                        message.mention_everyone,
                        &message.mention_roles,
                        &value,
                    )
                },
            );
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
        let rendered = state.render_user_mentions_with_highlights(
            message.guild_id,
            &message.mentions,
            message.mention_everyone,
            &message.mention_roles,
            &value,
        );
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

fn format_reply_line(
    reply: &ReplyInfo,
    guild_id: Option<Id<GuildMarker>>,
    state: &DashboardState,
    width: usize,
) -> MessageContentLine {
    let content = display_text_with_stickers(reply.content.as_deref(), &reply.sticker_names)
        .unwrap_or_else(|| "<empty message>".to_owned());
    let content =
        state.render_user_mentions_with_highlights(guild_id, &reply.mentions, false, &[], &content);
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
}
