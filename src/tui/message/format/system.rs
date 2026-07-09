//! Renderers for non-plain message kinds: system notices, slash-command
//! usage, thread created/starter lines, forwarded snapshots, and the
//! relative message age label.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::style::{Modifier, Style};

use crate::discord::ids::{Id, marker::MessageMarker};
use crate::discord::{MessageKind, MessageSnapshotInfo, MessageState};
use crate::tui::message::time as message_time;
use crate::tui::state::{DashboardState, discord_color};
use crate::tui::text::{truncate_display_width, truncate_text};
use crate::tui::theme;
use crate::tui::ui::forum::forum_post_card_lines;

use super::polls::format_poll_result_lines;
use super::{
    MessageContentLine, display_text_with_stickers, format_attachment_summary_lines,
    format_embed_lines, format_reply_line, prefix_message_content_line_without_underline,
    wrap_rendered_text_lines_with_loaded_custom_emoji_urls,
};

const COMMAND_USAGE_PREFIX: &str = "┌ ";

pub(super) fn format_message_kind_line(message_kind: MessageKind) -> Option<MessageContentLine> {
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

pub(super) fn format_system_message_lines(
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

pub(super) fn format_chat_input_command_line(
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
            .fg(discord_color(user_color, theme::current().text))
            .add_modifier(Modifier::DIM),
    );
    line.styled_range(
        command_start,
        command.len(),
        Style::default()
            .fg(theme::current().blurple)
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
    let thread_name = state
        .thread_summary_for_message(message)
        .map(|summary| summary.name)
        .or_else(|| {
            message
                .content
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "thread".to_owned());
    let mut lines = vec![format_thread_created_starter_line(
        message,
        state,
        &thread_name,
        width,
    )];

    // Reuse the forum-post card UI for the thread box. The card owns a two-column
    // marker gutter, so cap the body at 72 columns (the historical thread-box
    // maximum) and add the gutter back to keep the same on-screen width.
    let card_width = width.saturating_sub(2).clamp(4, 72).saturating_add(2);
    if let Some(item) = state.thread_card_item_for_message(message) {
        lines.extend(
            forum_post_card_lines(&item, false, card_width, state.show_custom_emoji())
                .into_iter()
                .map(MessageContentLine::from_line),
        );
    }
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
            theme::current().text,
        ))
        .bold();
    let thread_style = Style::default().fg(theme::current().accent).bold();
    let base_style = Style::default().fg(theme::current().text);

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

pub(super) fn format_forwarded_snapshot(
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
            false,
            &[],
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

#[cfg(test)]
mod tests {
    use super::*;

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
