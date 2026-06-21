use super::*;

pub(in crate::tui::ui) fn render_composer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    let inner_width = composer_inner_width(area.width);
    let ready_urls = ready_custom_emoji_urls(emoji_images);
    let prompt = composer_lines_with_loaded_custom_emoji_urls(state, inner_width, &ready_urls);
    let border_color = if state.is_composing() { ACCENT } else { DIM };

    frame.render_widget(
        Paragraph::new(prompt)
            .style(if state.is_composing() {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(DIM)
            })
            .block(
                Block::default()
                    .title(state.composer_title())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(Color::White).bold()),
            ),
        area,
    );
    if state.show_custom_emoji() {
        render_composer_custom_emoji_images(frame, area, state, emoji_images);
    }
    render_composer_attachment_previews(frame, area, state);
    if let Some(position) =
        composer_cursor_position_with_loaded_custom_emoji_urls(area, state, &ready_urls)
    {
        frame.set_cursor_position(position);
    }
}

fn ready_custom_emoji_urls(emoji_images: &[EmojiImage<'_>]) -> Vec<String> {
    emoji_images.iter().map(|image| image.url.clone()).collect()
}

#[cfg(test)]
pub(in crate::tui::ui) fn composer_cursor_position(
    area: Rect,
    state: &DashboardState,
) -> Option<Position> {
    composer_cursor_position_with_loaded_custom_emoji_urls(area, state, &[])
}

fn composer_cursor_position_with_loaded_custom_emoji_urls(
    area: Rect,
    state: &DashboardState,
    loaded_custom_emoji_urls: &[String],
) -> Option<Position> {
    if !state.is_composing() || area.width < 3 || area.height < 3 {
        return None;
    }

    let inner_width = composer_inner_width(area.width) as usize;
    let cursor = state.composer_cursor_byte_index();
    let display_input = composer_display_input(state, loaded_custom_emoji_urls);
    let display_cursor = display_input
        .map_byte_index(cursor)
        .min(display_input.input.len());
    let text_before_cursor = &display_input.input[..display_cursor];
    let prefixed = prefixed_composer_input(text_before_cursor);
    let wrapped = wrap_text_lines(&prefixed, inner_width);
    let mut prompt_row = wrapped.len().saturating_sub(1);
    let mut prompt_column = wrapped.last().map(|line| line.width()).unwrap_or_default();
    if prompt_column >= inner_width {
        prompt_row = prompt_row.saturating_add(1);
        prompt_column = 0;
    }

    let mut content_row = composer_rows_before_input(state);
    content_row = content_row.saturating_add(prompt_row);

    let x = area
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(prompt_column).unwrap_or(u16::MAX));
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(u16::try_from(content_row).unwrap_or(u16::MAX));
    let inner_right = area.x.saturating_add(area.width.saturating_sub(1));
    let inner_bottom = area.y.saturating_add(area.height.saturating_sub(1));
    if x >= inner_right || y >= inner_bottom {
        return None;
    }

    Some(Position { x, y })
}

pub(in crate::tui::ui) fn render_composer_mention_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
) {
    if state.composer_mention_query().is_none() {
        return;
    }
    let candidates = state.composer_mention_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(setup) = composer_picker_render_setup(
        message_areas,
        candidates.len(),
        state.composer_mention_selected(),
        |visible_count| state.composer_mention_window_start(visible_count, candidates.len()),
    ) else {
        return;
    };
    frame.render_widget(Clear, setup.area);
    let visible_candidates = &candidates[setup.visible_range.clone()];
    let lines = mention_picker_lines(visible_candidates, setup.selected_offset, setup.inner_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" mention ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), setup.area);
    render_composer_picker_scrollbar(frame, &setup, candidates.len());
}

pub(in crate::tui::ui) fn render_composer_command_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
) {
    if state.composer_command_query().is_none() {
        return;
    }
    let candidates = state.composer_command_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(setup) = composer_picker_render_setup(
        message_areas,
        candidates.len(),
        state.composer_command_selected(),
        |visible_count| state.composer_command_window_start(visible_count, candidates.len()),
    ) else {
        return;
    };
    let lines = command_picker_lines(
        &candidates[setup.visible_range.clone()],
        setup.selected_offset,
        setup.inner_width,
    );
    frame.render_widget(Clear, setup.area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title(" Commands ").borders(Borders::ALL)),
        setup.area,
    );
    render_composer_picker_scrollbar(frame, &setup, candidates.len());
}

pub(in crate::tui::ui) fn render_composer_emoji_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if state.composer_emoji_query().is_none() {
        return;
    }
    let candidates = state.composer_emoji_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(setup) = composer_picker_render_setup(
        message_areas,
        candidates.len(),
        state.composer_emoji_selected(),
        |visible_count| state.composer_emoji_window_start(visible_count, candidates.len()),
    ) else {
        return;
    };
    frame.render_widget(Clear, setup.area);
    let visible_candidates = &candidates[setup.visible_range.clone()];
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let lines = emoji_picker_lines(
        visible_candidates,
        setup.selected_offset,
        setup.inner_width,
        &ready_urls,
        state.show_custom_emoji(),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" emoji ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), setup.area);
    if state.show_custom_emoji() {
        render_composer_emoji_picker_images(frame, setup.area, visible_candidates, emoji_images);
    }
    render_composer_picker_scrollbar(frame, &setup, candidates.len());
}

struct ComposerPickerRenderSetup {
    area: Rect,
    visible_range: std::ops::Range<usize>,
    selected_offset: usize,
    inner_width: usize,
}

fn composer_picker_render_setup(
    message_areas: MessageAreas,
    candidate_count: usize,
    selected: usize,
    window_start_for_visible_count: impl FnOnce(usize) -> usize,
) -> Option<ComposerPickerRenderSetup> {
    let area = composer_picker_area(message_areas, candidate_count)?;
    let visible_count = usize::from(area.height.saturating_sub(2))
        .min(candidate_count)
        .max(1);
    let selected = selected.min(candidate_count.saturating_sub(1));
    let window_start = window_start_for_visible_count(visible_count);
    let visible_end = (window_start + visible_count).min(candidate_count);
    let shows_scrollbar = candidate_count > visible_count;
    let inner_width = area
        .width
        .saturating_sub(2)
        .saturating_sub(u16::from(shows_scrollbar)) as usize;

    Some(ComposerPickerRenderSetup {
        area,
        visible_range: window_start..visible_end,
        selected_offset: selected.saturating_sub(window_start),
        inner_width,
    })
}

fn render_composer_picker_scrollbar(
    frame: &mut Frame,
    setup: &ComposerPickerRenderSetup,
    candidate_count: usize,
) {
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(setup.area),
        setup.visible_range.start,
        setup.visible_range.len(),
        candidate_count,
    );
}

/// Picks a rectangle directly above the composer for the picker. Returns
/// `None` when there isn't enough room (very short terminal) so the caller
/// can silently skip drawing.
fn composer_picker_area(message_areas: MessageAreas, candidate_count: usize) -> Option<Rect> {
    let composer = message_areas.composer;
    let messages = message_areas.list;
    if composer.x < messages.x || composer.width == 0 {
        return None;
    }
    // 1 row per candidate + 2 for the bordered block.
    let desired_height = (candidate_count.min(MAX_MENTION_PICKER_VISIBLE) as u16).saturating_add(2);
    let available_above = composer.y.saturating_sub(messages.y);
    let height = desired_height.min(available_above);
    if height < 3 {
        return None;
    }
    let width = composer.width.min(messages.width);
    let x = composer.x;
    let y = composer.y.saturating_sub(height);
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn mention_picker_lines(
    candidates: &[MentionPickerEntry],
    selected: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let max_label_width = width.saturating_sub(4).max(1);
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let cursor = if index == selected { "› " } else { "  " };
            let bot_marker = if entry.is_bot { " [BOT]" } else { "" };
            // Show the raw username next to the alias when they differ so the
            // user can see which row matched their query when they typed
            // against the username instead of the alias.
            let username_hint = entry
                .username
                .as_deref()
                .filter(|name| !name.eq_ignore_ascii_case(&entry.display_name))
                .map(|name| format!(" @{name}"))
                .unwrap_or_default();
            let label = format!("{}{bot_marker}{username_hint}", entry.display_label());
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = mention_picker_entry_style(entry);
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let marker = match entry.target {
                MentionPickerTarget::User(_) => presence_marker(entry.status).to_string(),
                MentionPickerTarget::Everyone(_) | MentionPickerTarget::Role(_) => "@".to_owned(),
                MentionPickerTarget::Channel(_) => "#".to_owned(),
            };
            Line::from(vec![
                Span::styled(cursor, Style::default().fg(ACCENT)),
                Span::styled(marker, row_style),
                Span::styled(" ", row_style),
                Span::styled(label, row_style),
            ])
        })
        .collect()
}

fn mention_picker_entry_style(entry: &MentionPickerEntry) -> Style {
    match entry.target {
        MentionPickerTarget::User(_) => Style::default().fg(presence_color(entry.status)),
        MentionPickerTarget::Everyone(_) | MentionPickerTarget::Role(_) => {
            Style::default().fg(discord_color(entry.role_color, Color::Magenta))
        }
        MentionPickerTarget::Channel(_) => Style::default().fg(Color::Cyan),
    }
}

fn command_picker_lines(
    candidates: &[CommandPickerEntry],
    selected: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let marker = selection_marker(index == selected);
            let marker_width = marker.content.width();
            let label_width = entry.label.width();
            let detail_width = inner_width
                .saturating_sub(marker_width)
                .saturating_sub(label_width)
                .saturating_sub(1);
            Line::from(vec![
                marker,
                Span::styled(
                    entry.label.clone(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    truncate_display_width(&entry.detail, detail_width),
                    Style::default().fg(DIM),
                ),
            ])
        })
        .collect()
}

pub(in crate::tui::ui) fn emoji_picker_lines(
    candidates: &[EmojiPickerEntry],
    selected: usize,
    width: usize,
    ready_custom_emoji_urls: &[String],
    show_custom_emoji: bool,
) -> Vec<Line<'static>> {
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let cursor = if index == selected { "› " } else { "  " };
            let custom_image_ready = show_custom_emoji
                && entry
                    .custom_image_url
                    .as_ref()
                    .is_some_and(|url| ready_custom_emoji_urls.iter().any(|ready| ready == url));
            let prefix_width = emoji_picker_entry_prefix_width(entry, custom_image_ready);
            let description = entry.available_as_link.then_some("available as image link");
            let description_width = description
                .map(|value| value.width().saturating_add(" - ".width()))
                .unwrap_or_default();
            let max_label_width = width
                .saturating_sub(2)
                .saturating_sub(prefix_width)
                .saturating_sub(description_width)
                .max(1);
            let label = format!(":{}: {}", entry.shortcode, entry.name);
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = if entry.available {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(DIM).add_modifier(Modifier::CROSSED_OUT)
            };
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let mut spans = vec![Span::styled(cursor, Style::default().fg(ACCENT))];
            spans.extend(emoji_picker_entry_prefix(
                entry,
                custom_image_ready,
                row_style,
            ));
            spans.push(Span::styled(label, row_style));
            if let Some(description) = description {
                spans.push(Span::styled(" - ", Style::default().fg(DIM)));
                spans.push(Span::styled(description, Style::default().fg(DIM)));
            }
            Line::from(spans)
        })
        .collect()
}

fn emoji_picker_entry_prefix_width(entry: &EmojiPickerEntry, custom_image_ready: bool) -> usize {
    if entry.custom_image_url.is_some() {
        usize::from(custom_image_ready) * usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))
    } else {
        entry.emoji.as_str().width().saturating_add(1)
    }
}

fn emoji_picker_entry_prefix(
    entry: &EmojiPickerEntry,
    custom_image_ready: bool,
    row_style: Style,
) -> Vec<Span<'static>> {
    if entry.custom_image_url.is_some() {
        if custom_image_ready {
            vec![Span::styled(
                " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))),
                row_style,
            )]
        } else {
            Vec::new()
        }
    } else {
        vec![
            Span::styled(entry.emoji.clone(), row_style),
            Span::styled(" ", row_style),
        ]
    }
}

fn render_composer_emoji_picker_images(
    frame: &mut Frame,
    area: Rect,
    candidates: &[EmojiPickerEntry],
    emoji_images: &[EmojiImage<'_>],
) {
    let content = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    if content.width <= EMOJI_REACTION_IMAGE_WIDTH || content.height == 0 {
        return;
    }

    for (offset, entry) in candidates.iter().enumerate() {
        let Some(url) = entry.custom_image_url.as_deref() else {
            continue;
        };
        let Some(image) = emoji_images.iter().find(|image| image.url == url) else {
            continue;
        };
        let y = content
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= content.y.saturating_add(content.height) {
            continue;
        }
        let image_area = Rect::new(
            content.x.saturating_add(2),
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(content.width.saturating_sub(2)),
            1,
        );
        if image_area.width > 0 {
            frame.render_widget(RatatuiImage::new(image.protocol), image_area);
        }
    }
}

#[cfg(test)]
pub(in crate::tui::ui) fn composer_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
    composer_lines_with_loaded_custom_emoji_urls(state, width, &[])
}

pub(in crate::tui::ui) fn composer_lines_with_loaded_custom_emoji_urls(
    state: &DashboardState,
    width: u16,
    loaded_custom_emoji_urls: &[String],
) -> Vec<Line<'static>> {
    if state.is_composing()
        || !state.composer_input().is_empty()
        || !state.pending_composer_attachments().is_empty()
        || state.clipboard_paste_pending()
    {
        let mut lines = pending_upload_lines(state, width);
        append_composer_upload_preview_lines(&mut lines, state, width);
        let display_input = composer_display_input(state, loaded_custom_emoji_urls);
        if state.is_composing()
            && let Some(message) = state.reply_target_message_state()
        {
            lines.push(Line::from(Span::styled(
                reply_target_hint(message, state, width),
                Style::default().fg(DIM),
            )));
        }
        let prefixed_input = prefixed_composer_input(&display_input.input);
        let wrapped = wrap_text_lines(&prefixed_input, width as usize);
        for subline in wrapped {
            lines.push(Line::from(subline));
        }
        return lines;
    }

    vec![Line::from(composer_text(state, width))]
}

struct ComposerDisplayInput {
    input: String,
    replacements: Vec<ComposerEmojiReplacement>,
}

struct ComposerEmojiReplacement {
    start: usize,
    end: usize,
    new_start: usize,
    new_len: usize,
}

impl ComposerDisplayInput {
    fn map_byte_index(&self, position: usize) -> usize {
        let mut delta = 0isize;
        for replacement in &self.replacements {
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
}

fn composer_display_input(
    state: &DashboardState,
    loaded_custom_emoji_urls: &[String],
) -> ComposerDisplayInput {
    let original = state.composer_input();
    let mut completions = state.composer_emoji_image_completions();
    completions.sort_by_key(|completion| completion.byte_start);
    if completions.is_empty() || loaded_custom_emoji_urls.is_empty() {
        return ComposerDisplayInput {
            input: original.to_owned(),
            replacements: Vec::new(),
        };
    }

    let mut input = String::with_capacity(original.len());
    let mut cursor = 0usize;
    let mut replacements = Vec::new();
    for completion in completions {
        if completion.byte_end > original.len()
            || !original.is_char_boundary(completion.byte_start)
            || !original.is_char_boundary(completion.byte_end)
        {
            continue;
        }

        let start = completion.byte_start;
        let end = completion.byte_end;
        if start < cursor {
            continue;
        }

        input.push_str(&original[cursor..start]);
        let new_start = input.len();
        if loaded_custom_emoji_urls
            .iter()
            .any(|url| url == &completion.url)
        {
            let placeholder = " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH));
            input.push_str(&placeholder);
            replacements.push(ComposerEmojiReplacement {
                start,
                end,
                new_start,
                new_len: placeholder.len(),
            });
        } else {
            input.push_str(&original[start..end]);
        }
        cursor = end;
    }
    input.push_str(&original[cursor..]);

    ComposerDisplayInput {
        input,
        replacements,
    }
}

fn render_composer_custom_emoji_images(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_composing() || area.width < 3 || area.height < 3 {
        return;
    }

    let ready_urls = ready_custom_emoji_urls(emoji_images);
    let display_input = composer_display_input(state, &ready_urls);
    let input = display_input.input.as_str();
    let inner_width = composer_inner_width(area.width) as usize;
    let content_row = composer_rows_before_input(state);

    for completion in state.composer_emoji_image_completions() {
        let Some(image) = emoji_images
            .iter()
            .find(|image| image.url == completion.url)
        else {
            continue;
        };
        let Some((row, column)) = composer_custom_emoji_image_position(
            input,
            display_input.map_byte_index(completion.byte_start),
            display_input.map_byte_index(completion.byte_end),
            inner_width,
        ) else {
            continue;
        };
        let x = area
            .x
            .saturating_add(1)
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
        let y = area
            .y
            .saturating_add(1)
            .saturating_add(u16::try_from(content_row.saturating_add(row)).unwrap_or(u16::MAX));
        let inner_right = area.x.saturating_add(area.width.saturating_sub(1));
        let inner_bottom = area.y.saturating_add(area.height.saturating_sub(1));
        if x >= inner_right || y >= inner_bottom {
            continue;
        }
        let image_area = Rect::new(
            x,
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(inner_right.saturating_sub(x)),
            1,
        );
        if image_area.width > 0 {
            frame.render_widget(RatatuiImage::new(image.protocol), image_area);
        }
    }
}

fn composer_custom_emoji_image_position(
    input: &str,
    byte_start: usize,
    byte_end: usize,
    inner_width: usize,
) -> Option<(usize, usize)> {
    if inner_width == 0 || byte_start > byte_end || byte_end > input.len() {
        return None;
    }
    let before = prefixed_composer_input(&input[..byte_start]);
    let through = prefixed_composer_input(&input[..byte_end]);
    let before_wrapped = wrap_text_lines(&before, inner_width);
    let through_wrapped = wrap_text_lines(&through, inner_width);
    if before_wrapped.len() != through_wrapped.len() {
        return None;
    }
    Some((
        before_wrapped.len().saturating_sub(1),
        before_wrapped
            .last()
            .map(|line| line.width())
            .unwrap_or_default(),
    ))
}

fn render_composer_attachment_previews(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let previews = state.composer_attachment_previews();
    if previews.is_empty() || composer_upload_preview_line_count(state) == 0 {
        return;
    }
    let Some(preview_row) = composer_upload_preview_start_row(state) else {
        return;
    };
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let y = inner.y.saturating_add(preview_row);
    if y >= inner.y.saturating_add(inner.height) {
        return;
    }
    let height = LOCAL_UPLOAD_PREVIEW_HEIGHT.min(inner.y.saturating_add(inner.height) - y);
    if height == 0 {
        return;
    }
    let tile_width = LOCAL_UPLOAD_PREVIEW_WIDTH.min(inner.width);
    if tile_width == 0 {
        return;
    }
    for (index, preview) in previews.into_iter().enumerate() {
        let x_offset = u16::try_from(index)
            .unwrap_or(u16::MAX)
            .saturating_mul(tile_width.saturating_add(1));
        let x = inner.x.saturating_add(x_offset);
        if x >= inner.x.saturating_add(inner.width) {
            break;
        }
        let preview_area = Rect {
            x,
            y,
            width: tile_width.min(inner.x.saturating_add(inner.width) - x),
            height,
        };
        render_composer_attachment_preview(frame, preview_area, preview);
    }
}

fn composer_upload_preview_start_row(state: &DashboardState) -> Option<u16> {
    (composer_upload_preview_line_count(state) > 0).then(|| {
        u16::try_from(state.pending_composer_upload_line_count())
            .unwrap_or(u16::MAX)
            .saturating_add(1)
    })
}

fn render_composer_attachment_preview(
    frame: &mut Frame,
    area: Rect,
    preview: LocalUploadPreviewView<'_>,
) {
    match preview {
        LocalUploadPreviewView::Loading { filename } => frame.render_widget(
            Paragraph::new(format!("loading {filename}..."))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        ),
        LocalUploadPreviewView::Ready { protocol } => {
            frame.render_widget(RatatuiImage::new(protocol), area);
        }
    }
}

fn pending_upload_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
    pending_upload_texts(state, width)
        .into_iter()
        .map(|label| Line::from(Span::styled(label, Style::default().fg(ACCENT))))
        .collect()
}

fn pending_upload_texts(state: &DashboardState, width: u16) -> Vec<String> {
    let max_width = usize::from(width).max(1);
    let mut lines: Vec<_> = state
        .pending_composer_attachments()
        .iter()
        .map(|attachment| {
            let label = format!(
                "upload: {} ({})",
                attachment.filename,
                format_byte_size(attachment.size_bytes)
            );
            truncate_display_width(&label, max_width)
        })
        .collect();
    if state.clipboard_paste_pending() {
        lines.push(truncate_display_width(
            "upload: ⠋ processing clipboard attachment...",
            max_width,
        ));
    }
    lines
}

fn append_composer_upload_preview_lines(
    lines: &mut Vec<Line<'static>>,
    state: &DashboardState,
    width: u16,
) {
    append_composer_upload_preview_rows(lines, state, width, |text| {
        Line::from(Span::styled(text, Style::default().fg(DIM)))
    });
}

fn append_composer_upload_preview_texts(
    lines: &mut Vec<String>,
    state: &DashboardState,
    width: u16,
) {
    append_composer_upload_preview_rows(lines, state, width, |text| text);
}

fn append_composer_upload_preview_rows<T>(
    lines: &mut Vec<T>,
    state: &DashboardState,
    width: u16,
    build_line: impl Fn(String) -> T,
) {
    if composer_upload_preview_line_count(state) == 0 {
        return;
    }
    let max_width = usize::from(width).max(1);
    let separator = truncate_display_width(&"─".repeat(max_width), max_width);
    lines.push(build_line(separator.clone()));
    for _ in 0..LOCAL_UPLOAD_PREVIEW_HEIGHT {
        lines.push(build_line(String::new()));
    }
    lines.push(build_line(separator));
}

pub(in crate::tui::ui) fn composer_text(state: &DashboardState, width: u16) -> String {
    if state.is_composing() {
        let mut lines = pending_upload_texts(state, width);
        append_composer_upload_preview_texts(&mut lines, state, width);
        let input = prefixed_composer_input(state.composer_input());
        if let Some(message) = state.reply_target_message_state() {
            lines.push(reply_target_hint(message, state, width));
        }
        lines.push(input);
        return lines.join("\n");
    }

    if !state.composer_input().is_empty()
        || !state.pending_composer_attachments().is_empty()
        || state.clipboard_paste_pending()
    {
        let mut lines = pending_upload_texts(state, width);
        append_composer_upload_preview_texts(&mut lines, state, width);
        lines.push(prefixed_composer_input(state.composer_input()));
        return lines.join("\n");
    }

    if let Some(channel) = state.selected_channel_state() {
        let label = match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        };
        if channel.is_forum() {
            if state.can_create_post_in_selected_channel() {
                return format!(
                    "press {} to create a post in {label}",
                    state.key_bindings().start_composer_key_label()
                );
            }
            return format!("read-only · cannot create posts in {label}");
        }
        // Tell the user up-front if the shortcut won't open the composer here,
        // so they don't repeatedly press `i` and wonder why nothing happens.
        if !state.can_send_in_selected_channel() {
            return format!("read-only · cannot send messages in {label}");
        }
        // SEND is allowed but ATTACH is not. Tell the user uploads will be
        // refused before they try.
        if !state.can_attach_in_selected_channel() {
            return format!(
                "press {} to write in {label} (attachments disabled)",
                state.key_bindings().start_composer_key_label()
            );
        }
        return format!(
            "press {} to write in {label}",
            state.key_bindings().start_composer_key_label()
        );
    }

    "select a channel to write a message".to_owned()
}

fn reply_target_hint(message: &MessageState, state: &DashboardState, width: u16) -> String {
    const PREFIX: &str = "reply to ";
    let excerpt_width = usize::from(width).saturating_sub(PREFIX.width()).max(1);
    format!(
        "{PREFIX}{}",
        truncate_display_width(&reply_target_excerpt(message, state), excerpt_width)
    )
}

fn reply_target_excerpt(message: &MessageState, state: &DashboardState) -> String {
    if let Some(content) = message
        .content
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let rendered = state.render_user_mentions(message.guild_id, &message.mentions, content);
        return rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    }

    if !message.attachments.is_empty() {
        return format_attachment_summary(&message.attachments);
    }

    if message.content.is_some() {
        "<empty message>".to_owned()
    } else {
        "<message content unavailable>".to_owned()
    }
}
