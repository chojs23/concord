use super::forum;
use super::panes::{render_composer, render_composer_emoji_picker, render_composer_mention_picker};
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InlinePreviewSpacer {
    height: u16,
    accent_color: Option<u32>,
    overflow_count: usize,
}

struct MessageItemLinesInput<'a> {
    author: String,
    author_style: Style,
    sent_time: String,
    content: Vec<MessageContentLine>,
    reactions: Vec<MessageContentLine>,
    content_width: usize,
    preview_spacers: &'a [InlinePreviewSpacer],
    line_offset: usize,
}

#[derive(Clone, Copy)]
enum SelectedMessageLineKind {
    Top,
    Body,
}

pub(super) fn render_messages(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
    avatar_images: Vec<AvatarImage>,
    emoji_images: &[EmojiImage<'_>],
) {
    let block = panel_block_owned(
        state.message_pane_title(),
        state.focus() == FocusPane::Messages,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let message_areas = message_areas(inner, state);
    let content_width = message_content_width(message_areas.list);

    render_unread_banner(frame, message_areas.unread_banner, state);

    if state.selected_channel_is_forum() {
        let posts = state.visible_forum_post_items();
        let selected = state.focused_forum_post_selection();
        let is_loading = state.selected_forum_posts_loading();
        let forum_viewport_len =
            forum::forum_post_scrollbar_visible_count(message_areas.list.height);
        let forum_total_rows = state.message_total_rendered_rows(content_width, 0, 0);
        let forum_scrollbar_visible =
            vertical_scrollbar_visible(message_areas.list, forum_viewport_len, forum_total_rows);
        let forum_card_width =
            selected_message_card_width(message_areas.list.width as usize, forum_scrollbar_visible);
        frame.render_widget(
            Paragraph::new(forum::forum_post_viewport_lines_with_custom_emoji_images(
                &posts,
                selected,
                forum_card_width,
                is_loading,
                state.show_custom_emoji(),
            )),
            message_areas.list,
        );
        if state.show_custom_emoji() {
            forum::render_forum_post_reaction_emojis(
                frame,
                message_areas.list,
                &posts,
                forum_card_width,
                emoji_images,
            );
        }
        render_vertical_scrollbar(
            frame,
            message_areas.list,
            state.message_scroll_row_position(content_width, 0, 0),
            forum_viewport_len,
            forum_total_rows,
        );
        render_typing_footer(frame, message_areas.typing, state);
        render_composer(frame, message_areas.composer, state, emoji_images);
        render_composer_mention_picker(frame, message_areas, state);
        render_composer_emoji_picker(frame, message_areas, state, emoji_images);
        return;
    }

    let messages = state.visible_messages();
    let selected = state.focused_message_selection();

    let preview_width = if state.show_images() {
        inline_image_preview_width(message_areas.list)
    } else {
        0
    };
    let max_preview_height = if state.show_images() {
        inline_image_preview_height(message_areas.list, true)
    } else {
        0
    };
    let message_total_rows =
        state.message_total_rendered_rows(content_width, preview_width, max_preview_height);
    let message_scrollbar_visible = vertical_scrollbar_visible(
        message_areas.list,
        message_areas.list.height as usize,
        message_total_rows,
    );
    let selected_card_width =
        selected_message_card_width(message_areas.list.width as usize, message_scrollbar_visible);
    let lines = message_viewport_lines(
        &messages,
        selected,
        state,
        MessageViewportLayout {
            content_width,
            list_width: message_areas.list.width as usize,
            selected_card_width,
            preview_width,
            max_preview_height,
        },
        emoji_images,
    );

    frame.render_widget(Paragraph::new(lines), message_areas.list);
    let selected_avatar_body_top = selected.and_then(|selected| {
        message_body_top_row(
            &messages,
            state,
            selected,
            content_width,
            preview_width,
            max_preview_height,
        )
    });
    for avatar in avatar_images {
        if let Some(area) = message_avatar_area(
            message_areas.list,
            avatar.row,
            avatar.visible_height,
            selected_avatar_x_offset(selected_avatar_body_top, avatar.row),
        ) {
            frame.render_widget(RatatuiImage::new(&avatar.protocol), area);
        }
    }
    render_inline_reaction_emojis(
        frame,
        message_areas.list,
        &messages,
        state,
        content_width,
        selected,
        emoji_images,
    );
    render_inline_message_body_emojis(
        frame,
        message_areas.list,
        &messages,
        state,
        content_width,
        selected,
        emoji_images,
    );
    for image_preview in image_previews.into_iter() {
        let preview_rows_before_cell = inline_preview_rows_before_message(
            &messages,
            image_preview.message_index,
            preview_width,
            max_preview_height,
        )
        .saturating_add(image_preview.preview_y_offset_rows);
        let row = inline_image_preview_row(
            &messages,
            state,
            image_preview.message_index,
            content_width,
            state.message_line_scroll(),
            preview_rows_before_cell,
        );
        if let Some(preview_area) = inline_image_preview_area(
            message_areas.list,
            row,
            image_preview.preview_x_offset_columns.saturating_add(
                selected_message_content_x_offset(selected == Some(image_preview.message_index)),
            ),
            image_preview.preview_width,
            image_preview.preview_height,
            image_preview.accent_color,
        ) {
            render_image_preview(frame, preview_area, image_preview.state);
            render_image_preview_overflow_marker(
                frame,
                preview_area,
                image_preview.preview_overflow_count,
            );
        }
    }
    render_vertical_scrollbar(
        frame,
        message_areas.list,
        state.message_scroll_row_position(content_width, preview_width, max_preview_height),
        message_areas.list.height as usize,
        message_total_rows,
    );
    render_new_messages_notice(frame, message_areas.list, state);
    render_typing_footer(frame, message_areas.typing, state);
    render_composer(frame, message_areas.composer, state, emoji_images);
    render_composer_mention_picker(frame, message_areas, state);
    render_composer_emoji_picker(frame, message_areas, state, emoji_images);
}

fn render_new_messages_notice(frame: &mut Frame, list: Rect, state: &DashboardState) {
    let count = state.new_messages_count();
    if count == 0 || list.height == 0 || list.width == 0 {
        return;
    }

    let label = new_messages_notice_label(count);
    let width = u16::try_from(label.as_str().width())
        .unwrap_or(u16::MAX)
        .min(list.width);
    if width == 0 {
        return;
    }
    let area = Rect {
        x: list.x.saturating_add(list.width.saturating_sub(width) / 2),
        y: list.y.saturating_add(list.height.saturating_sub(1)),
        width,
        height: 1,
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(new_messages_notice_line(count, area.width as usize)),
        area,
    );
}

fn render_typing_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if area.height == 0 {
        return;
    }
    // The text might already be `None` if the only typer was the local user
    // and `message_areas` reserved the strip on a stale read. Render the
    // footer if and only if we still have a label to show.
    let Some(text) = state.typing_footer_for_selected_channel() else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM)))),
        area,
    );
}

fn render_unread_banner(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    // The banner row is reserved by `message_areas` based on the same
    // `unread_banner()` predicate. A missing payload here is a stale layout,
    // so avoid painting an empty Discord-blue strip.
    let Some(banner) = state.unread_banner() else {
        return;
    };

    const BG: Color = Color::Rgb(88, 101, 242);
    const FG: Color = Color::White;
    let style = Style::default().fg(FG).bg(BG);

    let since_label = format_unread_banner_since(banner.since_message_id);
    let left = match since_label {
        Some(time) => format!(" {} unread messages since {}", banner.unread_count, time),
        None => format!(" {} unread messages", banner.unread_count),
    };
    let right = "channel action (a) to mark as read ";

    frame.render_widget(
        Paragraph::new(unread_banner_line(left, right, area.width as usize, style)).style(style),
        area,
    );
}

fn unread_banner_line(left: String, right: &str, width: usize, style: Style) -> Line<'static> {
    let right_width = right.width();
    let left_width = left.as_str().width();
    if width == 0 {
        return Line::from(Span::styled("", style));
    }
    if right_width >= width {
        return Line::from(Span::styled(
            truncate_display_width(right, width),
            style.add_modifier(Modifier::BOLD),
        ));
    }
    let max_left_width = width.saturating_sub(right_width);
    let left = if left_width > max_left_width {
        truncate_display_width(&left, max_left_width)
    } else {
        left
    };
    let used = left.as_str().width().saturating_add(right_width);
    let padding = width.saturating_sub(used);
    Line::from(vec![
        Span::styled(left, style),
        Span::styled(" ".repeat(padding), style),
        Span::styled(right.to_owned(), style.add_modifier(Modifier::BOLD)),
    ])
}

fn format_unread_banner_since(message_id: Id<MessageMarker>) -> Option<String> {
    let unix_millis = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let dt = DateTime::from_timestamp_millis(unix_millis)?.with_timezone(&Local);
    Some(dt.format("%Y-%m-%d %H:%M").to_string())
}

fn render_inline_reaction_emojis(
    frame: &mut Frame,
    list: Rect,
    messages: &[&MessageState],
    state: &DashboardState,
    content_width: usize,
    selected: Option<usize>,
    emoji_images: &[EmojiImage<'_>],
) {
    if emoji_images.is_empty() || list.height == 0 || list.width <= MESSAGE_AVATAR_OFFSET {
        return;
    }

    let list_top = list.y as isize;
    let list_bottom = list_top + list.height as isize;
    let list_left = list.x as isize;
    let list_right = list_left + list.width as isize;
    let avatar_offset = MESSAGE_AVATAR_OFFSET as isize;

    let mut rendered_rows: isize = 0;

    for (index, message) in messages.iter().enumerate() {
        if rendered_rows >= list.height as isize {
            break;
        }
        let line_offset = if index == 0 {
            state.message_line_scroll() as isize
        } else {
            0
        };
        let global_index = state.message_scroll().saturating_add(index);
        let separator_lines = state.message_extra_top_lines(global_index) as isize;
        let body_rows = state.message_body_line_count_for_width(message, content_width) as isize;
        let total_base_rows =
            state.message_base_line_count_for_width(message, content_width) as isize;
        let block_rows = total_base_rows + separator_lines;
        let preview_height = total_inline_preview_height_for_message(
            message,
            if state.show_images() {
                inline_image_preview_width(list)
            } else {
                0
            },
            if state.show_images() {
                inline_image_preview_height(list, true)
            } else {
                0
            },
        ) as isize;

        let layout = lay_out_reaction_chips_with_custom_emoji_images(
            &message.reactions,
            content_width,
            state.show_custom_emoji(),
        );
        if !layout.slots.is_empty() {
            // Reactions are rendered after the body and inline preview spacer.
            // The body starts after the optional date separator, so the
            // reaction strip begins at:
            //     body_top + body_rows + preview_height
            let message_top = rendered_rows - line_offset;
            let body_top = message_top + separator_lines;
            let reaction_strip_top = body_top + body_rows + preview_height;

            for slot in layout.slots {
                let row_in_list = reaction_strip_top + slot.line as isize;
                if row_in_list < 0 || row_in_list >= list.height as isize {
                    continue;
                }
                let Some(image) = emoji_images.iter().find(|img| img.url == slot.url) else {
                    continue;
                };
                let absolute_row = list_top + row_in_list;
                let absolute_col = list_left
                    + avatar_offset
                    + selected_message_content_x_offset(selected == Some(index)) as isize
                    + slot.col as isize;
                if absolute_col >= list_right {
                    continue;
                }
                let max_width = (list_right - absolute_col).max(0) as u16;
                let image_width = EMOJI_REACTION_IMAGE_WIDTH.min(max_width);
                if image_width == 0 {
                    continue;
                }
                let image_area = Rect {
                    x: absolute_col as u16,
                    y: absolute_row as u16,
                    width: image_width,
                    height: 1,
                };
                if image_area.y >= list_bottom as u16 {
                    continue;
                }
                frame.render_widget(RatatuiImage::new(image.protocol), image_area);
            }
        }

        rendered_rows = rendered_rows
            .saturating_add((block_rows + preview_height + MESSAGE_ROW_GAP as isize) - line_offset);
    }
}

fn render_inline_message_body_emojis(
    frame: &mut Frame,
    list: Rect,
    messages: &[&MessageState],
    state: &DashboardState,
    content_width: usize,
    selected: Option<usize>,
    emoji_images: &[EmojiImage<'_>],
) {
    if emoji_images.is_empty() || list.height == 0 || list.width <= MESSAGE_AVATAR_OFFSET {
        return;
    }

    let list_top = list.y as isize;
    let list_bottom = list_top + list.height as isize;
    let list_left = list.x as isize;
    let list_right = list_left + list.width as isize;
    let avatar_offset = MESSAGE_AVATAR_OFFSET as isize;

    let mut rendered_rows: isize = 0;

    for (index, message) in messages.iter().enumerate() {
        if rendered_rows >= list.height as isize {
            break;
        }
        let line_offset = if index == 0 {
            state.message_line_scroll() as isize
        } else {
            0
        };
        let global_index = state.message_scroll().saturating_add(index);
        let separator_lines = state.message_extra_top_lines(global_index) as isize;
        let message_top = rendered_rows - line_offset;
        let body_top = message_top + separator_lines;

        let loaded_custom_emoji_urls = loaded_custom_emoji_urls(emoji_images);
        let body_lines = format_message_content_lines_with_loaded_custom_emoji_urls(
            message,
            state,
            content_width.max(8),
            &loaded_custom_emoji_urls,
        );
        let body_base_rows = 1 + body_lines.len() as isize;
        for (line_idx, line) in body_lines.iter().enumerate() {
            if line.image_slots.is_empty() {
                continue;
            }
            let row_in_list = body_top + 1 + line_idx as isize;
            if row_in_list < 0 || row_in_list >= list.height as isize {
                continue;
            }
            let absolute_row = list_top + row_in_list;
            if absolute_row >= list_bottom {
                continue;
            }
            for slot in &line.image_slots {
                let absolute_col = list_left
                    + avatar_offset
                    + selected_message_content_x_offset(selected == Some(index)) as isize
                    + slot.col as isize;
                if absolute_col >= list_right {
                    continue;
                }
                let Some(image) = emoji_images.iter().find(|img| img.url == slot.url) else {
                    continue;
                };
                let max_width = (list_right - absolute_col).max(0) as u16;
                let image_width = EMOJI_REACTION_IMAGE_WIDTH.min(max_width);
                if image_width == 0 {
                    continue;
                }
                let image_area = Rect {
                    x: absolute_col as u16,
                    y: absolute_row as u16,
                    width: image_width,
                    height: 1,
                };
                frame.render_widget(RatatuiImage::new(image.protocol), image_area);
            }
        }

        let preview_height = total_inline_preview_height_for_message(
            message,
            if state.show_images() {
                inline_image_preview_width(list)
            } else {
                0
            },
            if state.show_images() {
                inline_image_preview_height(list, true)
            } else {
                0
            },
        ) as isize;
        let block_rows = body_base_rows + separator_lines;
        rendered_rows = rendered_rows
            .saturating_add((block_rows + preview_height + MESSAGE_ROW_GAP as isize) - line_offset);
    }
}

pub(super) fn render_image_preview(
    frame: &mut Frame,
    area: Rect,
    image_preview: ImagePreviewState<'_>,
) {
    match image_preview {
        ImagePreviewState::Loading { filename } => frame.render_widget(
            Paragraph::new(format!("loading {filename}..."))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ImagePreviewState::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ImagePreviewState::Ready { protocol, .. } => {
            let widget = StatefulImage::new().resize(Resize::Fit(None));
            frame.render_stateful_widget(widget, area, protocol);
        }
    }
}

fn render_image_preview_overflow_marker(frame: &mut Frame, area: Rect, overflow_count: usize) {
    if overflow_count == 0 || area.width < 3 || area.height == 0 {
        return;
    }

    let marker = format!("+{overflow_count}");
    let width = u16::try_from(marker.width())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let marker_area = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(width)),
        y: area.y.saturating_add(area.height.saturating_sub(1)),
        width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(marker)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White).bg(Color::Black).bold()),
        marker_area,
    );
}

pub(super) fn message_viewport_lines(
    messages: &[&MessageState],
    selected: Option<usize>,
    state: &DashboardState,
    layout: MessageViewportLayout,
    emoji_images: &[EmojiImage<'_>],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let author = message.author.clone();
        let author_style = message_author_style(state.message_author_role_color(message));
        let preview_spacers = inline_preview_spacers_for_message(
            message,
            layout.preview_width,
            layout.max_preview_height,
        );

        let global_index = state.message_scroll().saturating_add(index);
        let mut top_lines = Vec::new();
        if state.message_starts_new_day_at(global_index) {
            top_lines.push(date_separator_line(message.id, layout.list_width));
        }
        if state.should_draw_unread_divider_at(global_index) {
            top_lines.push(unread_divider_line(layout.list_width));
        }
        let separator_lines = top_lines.len();
        let line_offset = usize::from(index == 0) * state.message_line_scroll();
        let body_skip = line_offset.saturating_sub(separator_lines);
        let item_content_width = if selected == Some(index) {
            selected_message_content_width(layout.selected_card_width)
        } else {
            layout.content_width
        };

        for line in top_lines.into_iter().skip(line_offset) {
            lines.push(line);
        }

        let loaded_custom_emoji_urls = loaded_custom_emoji_urls(emoji_images);
        let (content, reactions) = format_message_content_sections_with_loaded_custom_emoji_urls(
            message,
            state,
            item_content_width.max(8),
            &loaded_custom_emoji_urls,
        );

        let item_lines = message_item_lines_with_previews(MessageItemLinesInput {
            author,
            author_style,
            sent_time: format_message_sent_time(message.id),
            content,
            reactions,
            content_width: item_content_width,
            preview_spacers: &preview_spacers,
            line_offset: body_skip,
        });
        if selected == Some(index) {
            lines.extend(selected_message_lines(
                item_lines,
                selected_message_card_inner_width(layout.selected_card_width),
                body_skip == 0,
            ));
        } else {
            lines.extend(item_lines);
        }
    }
    lines
}

#[cfg(test)]
pub(super) fn message_viewport_layout(
    content_width: usize,
    list_width: usize,
    selected_card_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> MessageViewportLayout {
    MessageViewportLayout {
        content_width,
        list_width,
        selected_card_width,
        preview_width,
        max_preview_height,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn message_item_lines(
    author: String,
    author_style: Style,
    sent_time: String,
    content: Vec<MessageContentLine>,
    content_width: usize,
    preview_height: u16,
    preview_accent_color: Option<u32>,
    line_offset: usize,
) -> Vec<Line<'static>> {
    let preview_spacers = (preview_height > 0)
        .then_some(InlinePreviewSpacer {
            height: preview_height,
            accent_color: preview_accent_color,
            overflow_count: 0,
        })
        .into_iter()
        .collect::<Vec<_>>();
    message_item_lines_with_previews(MessageItemLinesInput {
        author,
        author_style,
        sent_time,
        content,
        reactions: Vec::new(),
        content_width,
        preview_spacers: &preview_spacers,
        line_offset,
    })
}

fn message_item_lines_with_previews(input: MessageItemLinesInput<'_>) -> Vec<Line<'static>> {
    let MessageItemLinesInput {
        author,
        author_style,
        sent_time,
        content,
        reactions,
        content_width,
        preview_spacers,
        line_offset,
    } = input;
    let sent_time_width = sent_time.as_str().width();
    let author_width = content_width
        .saturating_sub(sent_time_width)
        .saturating_sub(1)
        .max(1);
    let author = truncate_display_width(&author, author_width);
    let mut lines = vec![Line::from(vec![
        message_avatar_span(),
        Span::styled(author, author_style),
        Span::raw(" "),
        Span::styled(sent_time, Style::default().fg(DIM)),
    ])];
    lines.extend(content.into_iter().map(|line| {
        let mut spans = vec![message_avatar_spacer_span()];
        spans.extend(line.spans());
        Line::from(spans)
    }));
    for spacer in preview_spacers {
        lines.extend(image_preview_spacer_lines(spacer));
    }
    lines.extend(reactions.into_iter().map(|line| {
        let mut spans = vec![message_avatar_spacer_span()];
        spans.extend(line.spans());
        Line::from(spans)
    }));
    lines.push(Line::from(""));
    lines.into_iter().skip(line_offset).collect()
}

pub(super) fn message_author_style(role_color: Option<u32>) -> Style {
    Style::default()
        .fg(discord_color(role_color, Color::White))
        .bold()
}

pub(super) fn message_avatar_area(
    list: Rect,
    row: isize,
    visible_height: u16,
    x_offset: u16,
) -> Option<Rect> {
    if visible_height == 0 {
        return None;
    }

    let top = list.y as isize + row.max(0);
    let bottom = top.saturating_add(visible_height as isize);
    let list_bottom = list.y.saturating_add(list.height) as isize;
    if top >= list_bottom || bottom <= list.y as isize {
        return None;
    }

    Some(Rect {
        x: list.x.saturating_add(x_offset),
        y: u16::try_from(top).ok()?,
        width: MESSAGE_AVATAR_PLACEHOLDER.width() as u16,
        height: visible_height,
    })
}

fn message_avatar_span() -> Span<'static> {
    Span::styled(
        format!("{MESSAGE_AVATAR_PLACEHOLDER} "),
        Style::default().fg(DIM),
    )
}

fn message_avatar_spacer_span() -> Span<'static> {
    Span::raw(" ".repeat(MESSAGE_AVATAR_OFFSET as usize))
}

fn selected_message_lines(
    lines: Vec<Line<'static>>,
    inner_width: usize,
    top_visible: bool,
) -> Vec<Line<'static>> {
    let last_index = lines.len().saturating_sub(1);
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == last_index {
                selected_message_bottom_line(inner_width)
            } else if index == 0 && top_visible {
                selected_message_content_line(line, inner_width, SelectedMessageLineKind::Top)
            } else {
                selected_message_content_line(line, inner_width, SelectedMessageLineKind::Body)
            }
        })
        .collect()
}

fn selected_message_content_line(
    line: Line<'static>,
    inner_width: usize,
    kind: SelectedMessageLineKind,
) -> Line<'static> {
    if matches!(kind, SelectedMessageLineKind::Top) {
        return selected_message_top_line(line, inner_width);
    }

    let inner_width = inner_width.max(1);
    let used_width = line
        .spans
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>();
    let padding = inner_width.saturating_sub(used_width);
    let border_style = selected_message_border_style();
    let mut spans = vec![Span::styled("│ ", border_style)];
    spans.extend(line.spans);
    spans.push(Span::raw(" ".repeat(padding)));
    spans.push(Span::styled(" │", border_style));
    Line::from(spans)
}

fn selected_message_top_line(line: Line<'static>, inner_width: usize) -> Line<'static> {
    const TOP_HEADER_LEFT_FILL: usize = 1;

    let border_style = selected_message_border_style();
    let header_width = line
        .spans
        .iter()
        .map(|span| span.content.width())
        .sum::<usize>();
    let right_gap = usize::from(
        TOP_HEADER_LEFT_FILL
            .saturating_add(header_width)
            .saturating_add(1)
            <= inner_width.saturating_add(2),
    );
    let right_fill_width = inner_width
        .saturating_add(2)
        .saturating_sub(TOP_HEADER_LEFT_FILL)
        .saturating_sub(header_width)
        .saturating_sub(right_gap);

    let mut spans = vec![Span::styled(
        format!("╭{}", "─".repeat(TOP_HEADER_LEFT_FILL)),
        border_style,
    )];
    spans.extend(line.spans);
    if right_gap > 0 {
        spans.push(Span::styled(" ", border_style));
    }
    spans.push(Span::styled(
        format!("{}╮", "─".repeat(right_fill_width)),
        border_style,
    ));
    Line::from(spans)
}

fn selected_message_bottom_line(inner_width: usize) -> Line<'static> {
    let card_width = inner_width.saturating_add(4).max(4);
    Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
        selected_message_border_style(),
    ))
}

fn selected_message_border_style() -> Style {
    Style::default()
        .fg(SELECTED_MESSAGE_BORDER)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn selected_message_content_x_offset(selected: bool) -> u16 {
    if selected {
        SELECTED_MESSAGE_CONTENT_OFFSET
    } else {
        0
    }
}

fn loaded_custom_emoji_urls(emoji_images: &[EmojiImage<'_>]) -> Vec<String> {
    emoji_images.iter().map(|image| image.url.clone()).collect()
}

pub(super) fn selected_avatar_x_offset(selected_body_top: Option<isize>, avatar_row: isize) -> u16 {
    if selected_body_top.is_some_and(|row| avatar_row == row.max(0)) {
        SELECTED_MESSAGE_CONTENT_OFFSET
    } else {
        0
    }
}

pub(super) fn selected_message_card_width(list_width: usize, scrollbar_visible: bool) -> usize {
    list_width
        .saturating_sub(usize::from(scrollbar_visible))
        .max(4)
}

fn selected_message_content_width(card_width: usize) -> usize {
    selected_message_card_inner_width(card_width)
        .saturating_sub(MESSAGE_AVATAR_OFFSET as usize)
        .max(8)
}

fn selected_message_card_inner_width(list_width: usize) -> usize {
    list_width.saturating_sub(4).max(1)
}

fn message_body_top_row(
    messages: &[&MessageState],
    state: &DashboardState,
    local_index: usize,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> Option<isize> {
    let mut rendered_rows = 0usize;
    for (index, message) in messages.iter().enumerate() {
        let line_offset = usize::from(index == 0) * state.message_line_scroll();
        let global_index = state.message_scroll().saturating_add(index);
        let separator_lines = state.message_extra_top_lines(global_index);
        let body_top = rendered_rows as isize - line_offset as isize + separator_lines as isize;
        if index == local_index {
            return Some(body_top);
        }

        let body_base_rows = state.message_base_line_count_for_width(message, content_width);
        let preview_height =
            total_inline_preview_height_for_message(message, preview_width, max_preview_height);
        rendered_rows = rendered_rows.saturating_add(
            body_base_rows
                .saturating_add(separator_lines)
                .saturating_add(preview_height)
                .saturating_add(MESSAGE_ROW_GAP)
                .saturating_sub(line_offset),
        );
    }
    None
}

pub(super) fn format_message_sent_time(message_id: Id<MessageMarker>) -> String {
    let unix_millis = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
    format_unix_millis_local_time(unix_millis).unwrap_or_else(|| "--:--".to_owned())
}

fn format_unix_millis_local_time(unix_millis: u64) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&Local).format("%H:%M").to_string())
}

fn message_local_date(message_id: Id<MessageMarker>) -> NaiveDate {
    let unix_millis = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
    i64::try_from(unix_millis)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map(|dt| dt.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2015, 1, 1).expect("static date is valid"))
}

pub(crate) fn message_starts_new_day(
    current: Id<MessageMarker>,
    previous: Option<Id<MessageMarker>>,
) -> bool {
    match previous {
        None => true,
        Some(prev) => message_local_date(current) != message_local_date(prev),
    }
}

pub(super) fn date_separator_line(message_id: Id<MessageMarker>, width: usize) -> Line<'static> {
    let date = message_local_date(message_id);
    let label = format!(" {} ", date.format("%Y-%m-%d"));
    separator_line(&label, width, Style::default().fg(DIM))
}

pub(super) fn unread_divider_line(width: usize) -> Line<'static> {
    // Discord-style red bar with a small "New" tag pinned to the right
    // edge so the unread boundary is unambiguous in dark and light themes.
    const UNREAD: Color = Color::Rgb(237, 66, 69);
    const TAG: &str = " New ";

    let style = Style::default().fg(UNREAD);
    if width == 0 {
        return Line::from(Span::raw(""));
    }
    let tag_width = TAG.width();
    if width <= tag_width.saturating_add(2) {
        return Line::from(Span::styled("─".repeat(width), style));
    }
    let dash_count = width.saturating_sub(tag_width);
    Line::from(vec![
        Span::styled("─".repeat(dash_count), style),
        Span::styled(TAG, style.bold()),
    ])
}

pub(super) fn new_messages_notice_line(count: usize, width: usize) -> Line<'static> {
    let label = new_messages_notice_label(count);
    let text = if label.as_str().width() > width {
        truncate_display_width(&label, width)
    } else {
        let padding = width.saturating_sub(label.as_str().width());
        let left = padding / 2;
        let right = padding.saturating_sub(left);
        format!("{}{}{}", " ".repeat(left), label, " ".repeat(right))
    };
    Line::from(Span::styled(text, Style::default().fg(ACCENT).bold()))
}

fn new_messages_notice_label(count: usize) -> String {
    format!("↓ {count} new messages ")
}

fn separator_line(label: &str, width: usize, style: Style) -> Line<'static> {
    let label_width = label.width();
    let total = width.max(label_width.saturating_add(2));
    let dashes = total.saturating_sub(label_width);
    let left = dashes / 2;
    let right = dashes.saturating_sub(left);
    Line::from(Span::styled(
        format!("{}{}{}", "─".repeat(left), label, "─".repeat(right)),
        style,
    ))
}

#[cfg(test)]
pub(super) fn format_unix_millis_with_offset(
    unix_millis: u64,
    offset: chrono::FixedOffset,
) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&offset).format("%H:%M").to_string())
}

fn image_preview_spacer_lines(spacer: &InlinePreviewSpacer) -> Vec<Line<'static>> {
    let mut lines = (0..spacer.height)
        .map(|_| preview_spacer_blank_line(spacer.accent_color))
        .collect::<Vec<_>>();
    if spacer.overflow_count > 0 {
        lines.push(Line::from(vec![
            message_avatar_spacer_span(),
            Span::styled(
                format!("+{} more images", spacer.overflow_count),
                Style::default().fg(Color::White).bg(Color::Black).bold(),
            ),
        ]));
    }
    lines
}

fn preview_spacer_blank_line(accent_color: Option<u32>) -> Line<'static> {
    match accent_color {
        Some(color) => Line::from(vec![
            message_avatar_spacer_span(),
            Span::styled(
                EMBED_PREVIEW_GUTTER_PREFIX,
                Style::default().fg(embed_color(color)),
            ),
        ]),
        None => Line::from(""),
    }
}

fn inline_preview_spacers_for_message(
    message: &MessageState,
    preview_width: u16,
    max_preview_height: u16,
) -> Vec<InlinePreviewSpacer> {
    let previews = message.inline_previews();
    let album = super::super::media::image_preview_album_layout(
        &previews,
        preview_width,
        max_preview_height,
    );
    (album.height > 0)
        .then(|| {
            let accent_color = (previews.len() == 1)
                .then(|| previews[0].accent_color)
                .flatten();
            InlinePreviewSpacer {
                height: u16::try_from(album.height).unwrap_or(u16::MAX),
                accent_color,
                overflow_count: album.overflow_count,
            }
        })
        .into_iter()
        .collect()
}

fn total_inline_preview_height_for_message(
    message: &MessageState,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    inline_preview_spacers_for_message(message, preview_width, max_preview_height)
        .into_iter()
        .map(|spacer| {
            usize::from(spacer.height).saturating_add(usize::from(spacer.overflow_count > 0))
        })
        .sum()
}

fn inline_preview_rows_before_message(
    messages: &[&MessageState],
    message_index: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    messages
        .iter()
        .take(message_index)
        .map(|message| {
            total_inline_preview_height_for_message(message, preview_width, max_preview_height)
        })
        .sum()
}

pub(super) fn inline_image_preview_row(
    messages: &[&MessageState],
    state: &DashboardState,
    message_index: usize,
    content_width: usize,
    line_offset: usize,
    previous_preview_rows: usize,
) -> isize {
    let prior_rows = messages
        .iter()
        .enumerate()
        .take(message_index)
        .map(|(local_idx, message)| {
            let global = state.message_scroll().saturating_add(local_idx);
            state.message_base_line_count_for_width(message, content_width)
                + state.message_extra_top_lines(global)
        })
        .sum::<usize>();
    let current_rows = messages
        .get(message_index)
        .map(|message| {
            let global = state.message_scroll().saturating_add(message_index);
            state.message_body_line_count_for_width(message, content_width)
                + state.message_extra_top_lines(global)
        })
        .unwrap_or(0);
    let row = prior_rows
        .saturating_add(current_rows)
        .saturating_add(previous_preview_rows)
        .saturating_add(message_index.saturating_mul(MESSAGE_ROW_GAP))
        .saturating_sub(1);
    row as isize - line_offset as isize
}
