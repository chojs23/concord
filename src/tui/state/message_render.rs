use crate::discord::{AttachmentInfo, MessageSnapshotInfo, MessageState, PollInfo, ReactionInfo};

use super::super::format::{RenderedText, TextHighlight, TextHighlightKind};
use super::super::{media, ui};

pub(super) fn message_rendered_height_with_mentions<F, G>(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
    render_text: F,
    render_snapshot_text: G,
) -> usize
where
    F: Fn(&str) -> String,
    G: Fn(&MessageSnapshotInfo, &str) -> String,
{
    let preview_height = message
        .first_inline_preview()
        .map(|preview| {
            media::image_preview_height_for_dimensions(
                preview_width,
                max_preview_height,
                preview.width,
                preview.height,
            )
        })
        .unwrap_or(0);
    message_base_line_count_for_width_with_mentions(
        message,
        content_width,
        render_text,
        render_snapshot_text,
    ) + usize::from(preview_height)
        + ui::MESSAGE_ROW_GAP
}

pub(super) fn message_base_line_count_for_width_with_mentions<F, G>(
    message: &MessageState,
    content_width: usize,
    render_text: F,
    render_snapshot_text: G,
) -> usize
where
    F: Fn(&str) -> String,
    G: Fn(&MessageSnapshotInfo, &str) -> String,
{
    if let Some(system_lines) = system_message_line_count(message) {
        return 1 + system_lines.max(1);
    }

    let renders_poll_card = message.reply.is_none() && message.poll.is_some();
    let primary_content = if renders_poll_card {
        None
    } else {
        message.content.as_deref()
    };
    let primary_lines = message_primary_line_count(
        primary_content,
        &message.attachments,
        content_width,
        &render_text,
    );
    let kind_line = usize::from(
        message.reply.is_none() && message.poll.is_none() && !message.message_kind.is_regular(),
    );
    let reply_line = usize::from(message.reply.is_some());
    let poll_lines = if renders_poll_card {
        message
            .poll
            .as_ref()
            .map(|poll| {
                poll_card_line_count(
                    poll,
                    message.content.as_deref(),
                    content_width,
                    &render_text,
                )
            })
            .unwrap_or(0)
    } else {
        0
    };
    let reaction_lines = reaction_line_count(&message.reactions, content_width);
    let embed_lines =
        ui::embed_line_count(&message.embeds, message.content.as_deref(), content_width);

    if let Some(snapshot) = message.forwarded_snapshots.first() {
        let metadata_line =
            usize::from(snapshot.source_channel_id.is_some() || snapshot.timestamp.is_some());
        return 1
            + (reply_line
                + poll_lines
                + kind_line
                + primary_lines
                + embed_lines
                + forwarded_snapshot_line_count(snapshot, content_width, &render_snapshot_text)
                + metadata_line
                + reaction_lines)
                .max(1);
    }

    1 + (reply_line + poll_lines + kind_line + primary_lines + embed_lines + reaction_lines).max(1)
}

fn reaction_line_count(reactions: &[ReactionInfo], width: usize) -> usize {
    ui::lay_out_reaction_chips(reactions, width).lines.len()
}

fn poll_card_line_count(
    poll: &PollInfo,
    content: Option<&str>,
    content_width: usize,
    render_text: &dyn Fn(&str) -> String,
) -> usize {
    let inner_width = ui::poll_card_inner_width(content_width);
    let content_lines = content
        .filter(|value| !value.is_empty())
        .map(|value| ui::wrapped_text_line_count(&render_text(value), inner_width))
        .unwrap_or(0);

    2 + content_lines + 3 + poll.answers.len()
}

fn system_message_line_count(message: &MessageState) -> Option<usize> {
    match message.message_kind.code() {
        8..=11 => Some(1),
        18 => Some(3),
        21 => Some(2),
        46 => Some(match message.poll.as_ref() {
            Some(poll) if poll.total_votes.is_some() => 4,
            Some(_) => 3,
            None => 2,
        }),
        _ => None,
    }
}

fn message_primary_line_count(
    content: Option<&str>,
    attachments: &[AttachmentInfo],
    content_width: usize,
    render_text: &dyn Fn(&str) -> String,
) -> usize {
    content
        .filter(|value| !value.is_empty())
        .map(|value| ui::wrapped_text_line_count(&render_text(value), content_width))
        .unwrap_or(0)
        + usize::from(!attachments.is_empty())
}

fn forwarded_snapshot_line_count(
    snapshot: &MessageSnapshotInfo,
    content_width: usize,
    render_text: &dyn Fn(&MessageSnapshotInfo, &str) -> String,
) -> usize {
    let forwarded_content_width = content_width.saturating_sub(2).max(1);
    let content_lines = snapshot
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| {
            ui::wrapped_text_line_count(&render_text(snapshot, value), forwarded_content_width)
        })
        .unwrap_or(0);
    let attachment_line = usize::from(!snapshot.attachments.is_empty());
    let embed_lines = ui::embed_line_count(
        &snapshot.embeds,
        snapshot.content.as_deref(),
        forwarded_content_width,
    );

    1 + content_lines
        .saturating_add(attachment_line)
        .saturating_add(embed_lines)
        .max(1)
}

pub(super) fn add_literal_mention_highlights(rendered: &mut RenderedText, mention: &str) {
    let mut cursor = 0usize;
    while let Some(relative_start) = rendered.text[cursor..].find(mention) {
        let start = cursor.saturating_add(relative_start);
        let end = start.saturating_add(mention.len());
        if is_literal_mention_boundary(&rendered.text, start, end) {
            rendered.highlights.push(TextHighlight {
                start,
                end,
                // `@everyone`/`@here` always notify the current user, so they
                // share the self-mention background colour.
                kind: TextHighlightKind::SelfMention,
            });
        }
        cursor = end;
    }
}

pub(super) fn normalize_text_highlights(highlights: &mut Vec<TextHighlight>) {
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    let mut normalized: Vec<TextHighlight> = Vec::new();
    for highlight in highlights.drain(..) {
        let Some(last) = normalized.last_mut() else {
            normalized.push(highlight);
            continue;
        };
        if highlight.start <= last.end {
            last.end = last.end.max(highlight.end);
            // SelfMention always wins over OtherMention so that ranges that
            // happen to overlap (e.g. `@everyone @me` collisions) keep the
            // louder colour.
            if matches!(highlight.kind, TextHighlightKind::SelfMention) {
                last.kind = TextHighlightKind::SelfMention;
            }
        } else {
            normalized.push(highlight);
        }
    }
    *highlights = normalized;
}

fn is_literal_mention_boundary(value: &str, start: usize, end: usize) -> bool {
    let before = value[..start].chars().next_back();
    let after = value[end..].chars().next();
    !before.is_some_and(is_literal_mention_word_char)
        && !after.is_some_and(is_literal_mention_word_char)
}

fn is_literal_mention_word_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}
