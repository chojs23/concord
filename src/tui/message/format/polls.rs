//! Poll card and poll-result renderers.

use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::discord::PollInfo;
use crate::tui::format::{RenderedText, truncate_display_width, truncate_text};

use super::{MessageContentLine, wrap_rendered_text_lines_with_loaded_custom_emoji_urls};

pub(super) fn format_poll_lines(
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

pub(super) fn format_poll_result_lines(
    poll: Option<&PollInfo>,
    width: usize,
) -> Vec<MessageContentLine> {
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
