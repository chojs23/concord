use std::time::{SystemTime, UNIX_EPOCH};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, MessageMarker},
};
use ratatui::{
    style::{Color, Modifier, Style, Stylize},
    text::Span,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{
    format::{
        RenderedText, TextHighlight, TextHighlightKind, truncate_display_width, truncate_text,
    },
    state::{DashboardState, ThreadSummary, discord_color},
};
use crate::discord::{
    AttachmentInfo, EmbedInfo, MessageKind, MessageSnapshotInfo, MessageState, PollInfo,
    ReactionEmoji, ReactionInfo, ReplyInfo,
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
const THREAD_CARD_INDENT: &str = "  ";
pub(super) const EMOJI_REACTION_IMAGE_WIDTH: u16 = 2;

#[derive(Clone)]
pub(super) struct MessageContentLine {
    pub(super) text: String,
    pub(super) style: Style,
    mention_highlights: Vec<TextHighlight>,
    styled_prefixes: Vec<StyledPrefix>,
}

#[derive(Clone, Copy)]
struct StyledPrefix {
    start: usize,
    len: usize,
    style: Style,
}

impl MessageContentLine {
    pub(super) fn plain(text: String) -> Self {
        Self {
            text,
            style: Style::default(),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
        }
    }

    fn styled_text(text: String, style: Style, mention_highlights: Vec<TextHighlight>) -> Self {
        Self {
            text,
            style,
            mention_highlights,
            styled_prefixes: Vec::new(),
        }
    }

    fn dim(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(DIM),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
        }
    }

    fn accent(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(ACCENT),
            mention_highlights: Vec::new(),
            styled_prefixes: Vec::new(),
        }
    }

    fn styled_range(&mut self, start: usize, len: usize, style: Style) {
        let end = start.saturating_add(len).min(self.text.len());
        if start < end {
            self.styled_prefixes.push(StyledPrefix {
                start,
                len: end.saturating_sub(start),
                style,
            });
        }
    }

    pub(super) fn spans(&self) -> Vec<Span<'static>> {
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
        let mut style = self
            .styled_prefixes
            .iter()
            .find(|prefix| prefix.contains(start, end))
            .map(|prefix| prefix.style)
            .unwrap_or(self.style);

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
pub(super) fn format_message_content(message: &MessageState, width: usize) -> String {
    format_message_content_lines(message, &DashboardState::new(), width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn format_message_content_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let attachment_summary =
        (!message.attachments.is_empty()).then(|| format_attachment_summary(&message.attachments));
    let mut lines = Vec::new();

    if let Some(system_lines) = format_system_message_lines(message, state, width) {
        return system_lines;
    }

    let renders_poll_card = message.reply.is_none() && message.poll.is_some();

    if let Some(line) = message
        .reply
        .as_ref()
        .map(|reply| format_reply_line(reply, message.guild_id, state, width))
    {
        lines.push(line);
    } else if let Some(poll) = message.poll.as_ref() {
        let content = message
            .content
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                state.render_user_mentions_with_highlights(
                    message.guild_id,
                    &message.mentions,
                    value,
                )
            });
        lines.extend(format_poll_lines(poll, content, width));
    } else if let Some(line) = format_message_kind_line(message.message_kind) {
        lines.push(line);
    }

    let standalone_content = (!renders_poll_card)
        .then(|| message.content.as_deref().filter(|value| !value.is_empty()))
        .flatten();
    if let Some(value) = standalone_content {
        lines.extend(wrap_rendered_text_lines(
            state.render_user_mentions_with_highlights(message.guild_id, &message.mentions, value),
            width,
            Style::default(),
        ));
    }
    lines.extend(format_embed_lines(
        &message.embeds,
        message.content.as_deref(),
        width,
    ));
    if let Some(attachments) = attachment_summary {
        lines.push(MessageContentLine::accent(truncate_text(
            &attachments,
            width,
        )));
    }
    if let Some(snapshot) = message.forwarded_snapshots.first() {
        lines.extend(format_forwarded_snapshot(snapshot, state, width));
    }
    if !message.reactions.is_empty() {
        lines.extend(format_reaction_lines(&message.reactions, width));
    }

    if lines.is_empty() {
        lines.push(MessageContentLine::plain(if message.content.is_some() {
            "<empty message>".to_owned()
        } else {
            "<message content unavailable>".to_owned()
        }));
    }

    lines
}

fn format_embed_lines(
    embeds: &[EmbedInfo],
    message_content: Option<&str>,
    width: usize,
) -> Vec<MessageContentLine> {
    embeds
        .iter()
        .flat_map(|embed| format_embed(embed, message_content, width))
        .collect()
}

fn format_embed(
    embed: &EmbedInfo,
    message_content: Option<&str>,
    width: usize,
) -> Vec<MessageContentLine> {
    const PREFIX: &str = "  ▎ ";
    let inner_width = width.saturating_sub(PREFIX.width()).max(1);
    let mut lines = Vec::new();

    push_embed_text(
        &mut lines,
        embed.provider_name.as_deref(),
        inner_width,
        embed_provider_style(),
    );
    push_embed_text(
        &mut lines,
        embed.author_name.as_deref(),
        inner_width,
        embed_author_style(),
    );
    push_embed_text(
        &mut lines,
        embed.title.as_deref(),
        inner_width,
        embed_title_style(),
    );
    for field in &embed.fields {
        push_embed_text(
            &mut lines,
            Some(field.name.as_str()),
            inner_width,
            embed_field_name_style(),
        );
        push_embed_text(
            &mut lines,
            Some(field.value.as_str()),
            inner_width,
            Style::default(),
        );
    }
    push_embed_text(
        &mut lines,
        embed.footer_text.as_deref(),
        inner_width,
        embed_footer_style(),
    );
    for url in [&embed.url]
        .into_iter()
        .filter_map(|url| url.as_deref())
        .filter(|url| !message_content.is_some_and(|content| content.contains(url)))
    {
        push_embed_text(&mut lines, Some(url), inner_width, embed_url_style());
    }

    lines
        .into_iter()
        .map(|line| prefix_message_content_line_with_style(PREFIX, embed_line_style(embed), line))
        .collect()
}

fn push_embed_text(
    lines: &mut Vec<MessageContentLine>,
    value: Option<&str>,
    width: usize,
    style: Style,
) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    lines.extend(
        wrap_text_lines(value, width)
            .into_iter()
            .map(|line| MessageContentLine::styled_text(line, style, Vec::new())),
    );
}

fn embed_provider_style() -> Style {
    Style::default().fg(DIM).add_modifier(Modifier::ITALIC)
}

fn embed_author_style() -> Style {
    Style::default().add_modifier(Modifier::ITALIC)
}

fn embed_title_style() -> Style {
    Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::BOLD)
}

fn embed_field_name_style() -> Style {
    Style::default()
        .add_modifier(Modifier::BOLD)
        .add_modifier(Modifier::UNDERLINED)
}

fn embed_footer_style() -> Style {
    Style::default().fg(DIM).add_modifier(Modifier::ITALIC)
}

fn embed_url_style() -> Style {
    Style::default()
        .fg(Color::Blue)
        .add_modifier(Modifier::UNDERLINED)
}

fn embed_line_style(embed: &EmbedInfo) -> Style {
    Style::default().fg(embed_line_color(embed))
}

fn embed_line_color(embed: &EmbedInfo) -> Color {
    embed.color.map(embed_color).unwrap_or(Color::Red)
}

pub(super) fn embed_color(color: u32) -> Color {
    Color::Rgb(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}

fn format_reaction_lines(reactions: &[ReactionInfo], width: usize) -> Vec<MessageContentLine> {
    lay_out_reaction_chips(reactions, width)
        .lines
        .into_iter()
        .map(MessageContentLine::accent)
        .collect()
}

/// Position of a custom-emoji image overlay relative to the start of a
/// message's reaction strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReactionImageSlot {
    pub(crate) line: u16,
    pub(crate) col: u16,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReactionLayout {
    pub(crate) lines: Vec<String>,
    pub(crate) slots: Vec<ReactionImageSlot>,
}

/// Builds a single chip's text plus the chip-internal column offset where its
/// image overlay should land (if any). Custom-emoji chips reserve a fixed
/// `EMOJI_REACTION_IMAGE_WIDTH` of spaces in place of the textual `:name:`
/// label so that loading the image later does not reflow the row.
fn build_reaction_chip(reaction: &ReactionInfo) -> (String, Option<usize>, Option<String>) {
    let count = reaction.count;
    match &reaction.emoji {
        ReactionEmoji::Unicode(emoji) => {
            let chip = if reaction.me {
                format!("[● {emoji} {count}]")
            } else {
                format!("[{emoji} {count}]")
            };
            (chip, None, None)
        }
        ReactionEmoji::Custom { .. } => {
            let url = reaction.emoji.custom_image_url();
            let placeholder = " ".repeat(EMOJI_REACTION_IMAGE_WIDTH as usize);
            let prefix = if reaction.me { "[● " } else { "[" };
            let chip = format!("{prefix}{placeholder} {count}]");
            let image_offset = prefix.width();
            (chip, Some(image_offset), url)
        }
    }
}

/// Lays out reaction chips for a message, wrapping at chip boundaries so a
/// chip is never split across rows. Returns both the rendered text rows and
/// the absolute (line, col) position of every custom-emoji image overlay,
/// relative to the first reaction row.
pub(crate) fn lay_out_reaction_chips(reactions: &[ReactionInfo], width: usize) -> ReactionLayout {
    let width = width.max(1);
    let chips: Vec<(String, Option<usize>, Option<String>)> = reactions
        .iter()
        .filter(|reaction| reaction.count > 0)
        .map(build_reaction_chip)
        .collect();
    if chips.is_empty() {
        return ReactionLayout::default();
    }

    let mut lines: Vec<String> = Vec::new();
    let mut slots: Vec<ReactionImageSlot> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for (chip_text, image_offset, url) in chips {
        let chip_width = chip_text.width();
        let separator_width = if current_width == 0 { 0 } else { 2 };
        let projected = current_width + separator_width + chip_width;
        let needs_wrap = current_width > 0 && projected > width;
        if needs_wrap {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }

        let chip_start_col = if current_width == 0 {
            0usize
        } else {
            current.push_str("  ");
            current_width += 2;
            current_width
        };
        current.push_str(&chip_text);
        current_width += chip_width;

        if let (Some(offset), Some(url)) = (image_offset, url) {
            slots.push(ReactionImageSlot {
                line: u16::try_from(lines.len()).unwrap_or(u16::MAX),
                col: u16::try_from(chip_start_col + offset).unwrap_or(u16::MAX),
                url,
            });
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    ReactionLayout { lines, slots }
}

fn wrap_rendered_text_lines(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    wrap_text_with_highlights(&rendered.text, &rendered.highlights, width)
        .into_iter()
        .map(|(text, mention_highlights)| {
            MessageContentLine::styled_text(text, style, mention_highlights)
        })
        .collect()
}

fn rendered_text_line(rendered: RenderedText, style: Style) -> MessageContentLine {
    MessageContentLine::styled_text(rendered.text, style, rendered.highlights)
}

fn prepend_rendered_text(prefix: String, mut rendered: RenderedText) -> RenderedText {
    let shift = prefix.len();
    for highlight in &mut rendered.highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
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
    RenderedText { text, highlights }
}

fn prefix_message_content_line(prefix: &str, mut line: MessageContentLine) -> MessageContentLine {
    let shift = prefix.len();
    for highlight in &mut line.mention_highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    for styled_prefix in &mut line.styled_prefixes {
        styled_prefix.start = styled_prefix.start.saturating_add(shift);
    }
    line.text.insert_str(0, prefix);
    line
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
    });
    line
}

pub(super) fn wrap_text_lines(value: &str, width: usize) -> Vec<String> {
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

fn wrap_text_with_highlights(
    value: &str,
    highlights: &[TextHighlight],
    width: usize,
) -> Vec<(String, Vec<TextHighlight>)> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    for line in value.split('\n') {
        if line.is_empty() {
            lines.push((String::new(), Vec::new()));
            line_start = line_start.saturating_add(1);
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        let mut current_start = line_start;
        let mut current_end = line_start;
        for (relative_start, grapheme) in line.grapheme_indices(true) {
            let grapheme_start = line_start.saturating_add(relative_start);
            let grapheme_end = grapheme_start.saturating_add(grapheme.len());
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width.saturating_add(grapheme_width) > width
            {
                let text = std::mem::take(&mut current);
                lines.push((
                    text,
                    highlights_for_range(highlights, current_start, current_end),
                ));
                current_width = 0;
                current_start = grapheme_start;
            }

            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
            current_end = grapheme_end;
        }
        lines.push((
            current,
            highlights_for_range(highlights, current_start, current_end),
        ));
        line_start = line_start.saturating_add(line.len()).saturating_add(1);
    }
    lines
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
            wrap_rendered_text_lines(content, inner_width, Style::default())
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

pub(super) fn poll_box_border(left: char, right: char, width: usize) -> String {
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
    let content = reply
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or("<empty message>");
    let content = state.render_user_mentions_with_highlights(guild_id, &reply.mentions, content);
    let content = prepend_rendered_text(format!("╭─ {} : ", reply.author), content);
    rendered_text_line(
        truncate_rendered_text(content, width),
        Style::default().fg(DIM),
    )
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

pub(super) fn format_message_relative_age(message_id: Id<MessageMarker>) -> String {
    let created = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
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
) -> Vec<MessageContentLine> {
    let attachment_summary = (!snapshot.attachments.is_empty())
        .then(|| format_attachment_summary(&snapshot.attachments));
    let mut lines = vec![MessageContentLine::plain("↱ Forwarded".to_owned())];
    if let Some(content) = snapshot
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let content_width = width.saturating_sub(2).max(1);
        let content = state.render_user_mentions_with_highlights(
            state.forwarded_snapshot_mention_guild_id(snapshot),
            &snapshot.mentions,
            content,
        );
        lines.extend(
            wrap_rendered_text_lines(content, content_width, Style::default())
                .into_iter()
                .map(|line| prefix_message_content_line_without_underline("│ ", line)),
        );
    }
    if let Some(attachments) = attachment_summary {
        lines.push(MessageContentLine::accent(truncate_text(
            &format!("│ {attachments}"),
            width,
        )));
    }
    lines.extend(
        format_embed_lines(
            &snapshot.embeds,
            snapshot.content.as_deref(),
            width.saturating_sub(2).max(1),
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

pub(super) fn format_attachment_summary(attachments: &[AttachmentInfo]) -> String {
    attachments
        .iter()
        .map(format_attachment)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_attachment(attachment: &AttachmentInfo) -> String {
    let kind = if attachment.is_image() {
        "image"
    } else if attachment.is_video() {
        "video"
    } else {
        "file"
    };
    let dimensions = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!(" {width}x{height}"),
        _ => String::new(),
    };

    format!("[{kind}: {}]{}", attachment.filename, dimensions)
}

pub(super) fn mention_highlight_style(kind: TextHighlightKind) -> Style {
    match kind {
        // The current user got pinged — Discord paints this gold/yellow.
        TextHighlightKind::SelfMention => Style::default()
            .bg(Color::Rgb(92, 76, 35))
            .fg(Color::Yellow),
        // Someone else was pinged — render with Discord's softer blue tint so
        // the user can see the chip without the "you" alarm colour.
        TextHighlightKind::OtherMention => Style::default()
            .bg(Color::Rgb(40, 50, 92))
            .fg(Color::Rgb(193, 206, 247)),
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
            }],
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
