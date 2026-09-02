use super::*;
use crate::tui::media::{PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH};
use crate::tui::state::{UserProfileSettingsField, UserProfileSettingsTab};
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};

pub(in crate::tui::ui) fn render_user_profile_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    avatar: Option<AvatarImage>,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::UserProfile) {
        return;
    }

    let popup = user_profile_popup_area(area);
    let context = state
        .user_profile_popup_guild_id()
        .and_then(|guild_id| state.guild_name(guild_id))
        .unwrap_or_default();
    let frame_areas =
        render_popup_form_frame_with_footer_height(frame, popup, "Profile", context, 0);
    let areas = user_profile_popup_areas(frame_areas.content, state);

    // The document always uses the full content width. Only its identity lines
    // reserve avatar columns, so later sections can return to the left edge.
    let has_avatar = user_profile_popup_has_avatar_inside(
        areas.document,
        state.show_avatars() && state.user_profile_popup_has_avatar_preview(),
    );
    let document_area = Rect {
        width: areas.document.width.saturating_sub(1).max(1),
        ..areas.document
    };

    let popup_text =
        user_profile_popup_text_for_render(state, document_area.width, has_avatar, emoji_images);
    let total_lines = popup_text.lines.len();
    let viewport = document_area.height as usize;
    let scroll_position = state
        .user_profile_popup_scroll()
        .min(total_lines.saturating_sub(viewport));
    let lines = popup_text
        .lines
        .into_iter()
        .skip(scroll_position)
        .take(viewport)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        document_area,
    );
    render_vertical_scrollbar(
        frame,
        areas.document,
        scroll_position,
        viewport,
        total_lines,
    );

    if let Some((line, column)) = popup_text.cursor
        && let Some(visible_offset) = line.checked_sub(scroll_position)
        && visible_offset < viewport
    {
        let x = document_area
            .x
            .saturating_add(u16::try_from(column).unwrap_or(u16::MAX))
            .min(
                document_area
                    .x
                    .saturating_add(document_area.width.saturating_sub(1)),
            );
        let y = document_area.y.saturating_add(visible_offset as u16);
        frame.set_cursor_position(Position::new(x, y));
    }

    if state.show_custom_emoji() {
        let list = Rect {
            height: viewport as u16,
            ..document_area
        };
        overlay_emoji_slots(
            frame,
            list,
            emoji_images,
            &[],
            popup_text
                .emoji_overlays
                .iter()
                .map(|(line_idx, url)| EmojiSlot {
                    row_in_list: *line_idx as isize - scroll_position as isize,
                    col: document_area.x as isize,
                    max_width: u16::MAX,
                    image_size: crate::tui::text::EmojiImageSize::Compact,
                    url: url.clone(),
                }),
        );
    }

    if let Some(avatar) = avatar.filter(|avatar| has_avatar && avatar.visible_height > 0) {
        let avatar_area = Rect {
            x: areas.document.x,
            y: areas.document.y,
            width: PROFILE_POPUP_AVATAR_WIDTH.min(areas.document.width),
            height: avatar.visible_height.min(areas.document.height),
        };
        frame.render_widget(RatatuiImage::new(avatar.protocol), avatar_area);
    }

    render_user_profile_popup_status(frame, areas.status, state);
}

const USER_PROFILE_POPUP_WIDTH: u16 = 60;
const USER_PROFILE_POPUP_HEIGHT: u16 = 24;

/// Centered popup rect inside the messages area. Shared so the geometry
/// computation lives in one place and the scroll-clamping pass uses the
/// exact same width/height the renderer ends up drawing into.
pub(in crate::tui) fn user_profile_popup_area(area: Rect) -> Rect {
    let width = USER_PROFILE_POPUP_WIDTH
        .min(area.width.saturating_sub(2))
        .max(8);
    let height = USER_PROFILE_POPUP_HEIGHT
        .min(area.height.saturating_sub(2))
        .max(6);
    centered_rect(area, width, height)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UserProfilePopupAreas {
    status: Rect,
    document: Rect,
}

fn user_profile_popup_areas(inner: Rect, state: &DashboardState) -> UserProfilePopupAreas {
    let status_height =
        u16::try_from(user_profile_popup_status_lines(state, usize::from(inner.width)).len())
            .unwrap_or(u16::MAX)
            .min(inner.height);
    UserProfilePopupAreas {
        status: Rect {
            height: status_height,
            ..inner
        },
        document: Rect {
            y: inner.y.saturating_add(status_height),
            height: inner.height.saturating_sub(status_height),
            ..inner
        },
    }
}

fn user_profile_popup_areas_for_frame(area: Rect, state: &DashboardState) -> UserProfilePopupAreas {
    let popup = user_profile_popup_area(area);
    let inner = popup_form_areas_with_footer_height(popup, 0).content;
    user_profile_popup_areas(inner, state)
}

fn render_user_profile_popup_status(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if area.is_empty() {
        return;
    }
    let lines = user_profile_popup_status_lines(state, usize::from(area.width));
    frame.render_widget(Paragraph::new(lines), area);
}

fn user_profile_popup_status_lines(state: &DashboardState, width: usize) -> Vec<Line<'static>> {
    let Some((message, style)) = user_profile_settings_status(state) else {
        return Vec::new();
    };
    wrapped_styled_popup_lines(&message, width.max(1), style)
}

fn user_profile_settings_status(state: &DashboardState) -> Option<(String, Style)> {
    let (message, group) = if state.user_profile_settings_saving() {
        (
            "Saving profile changes...".to_owned(),
            theme::HighlightGroup::Warning,
        )
    } else if let Some(status) = state.user_profile_settings_status() {
        let group = if status == "Saved profile changes" {
            theme::HighlightGroup::Success
        } else if status.contains("failed") {
            theme::HighlightGroup::Error
        } else {
            theme::HighlightGroup::Warning
        };
        (status.to_owned(), group)
    } else {
        let dirty_count = state.user_profile_settings_dirty_count();
        if dirty_count == 0 {
            return None;
        }
        (
            "Unsaved changes.".to_owned(),
            theme::HighlightGroup::Warning,
        )
    };

    Some((message, theme::current().style(group)))
}

pub(in crate::tui::ui) fn user_profile_popup_has_avatar(
    area: Rect,
    state: &DashboardState,
    has_avatar_url: bool,
) -> bool {
    let content = user_profile_popup_areas_for_frame(area, state).document;
    user_profile_popup_has_avatar_inside(content, has_avatar_url)
}

fn user_profile_popup_has_avatar_inside(inner: Rect, has_avatar_url: bool) -> bool {
    has_avatar_url && inner.width > PROFILE_POPUP_AVATAR_WIDTH + 2
}

/// Returns the visible avatar rectangle and the number of rows cropped from
/// its top. The media cache uses the crop to build the same kind of clipped
/// protocol used by message avatars, so scrolling does not resize or abruptly
/// hide the image.
pub(in crate::tui) fn user_profile_popup_avatar_viewport(
    area: Rect,
    state: &DashboardState,
) -> Option<(Rect, u16)> {
    let content = user_profile_popup_areas_for_frame(area, state).document;
    if !user_profile_popup_has_avatar_inside(content, true) {
        return None;
    }

    let top_clip_rows = u16::try_from(state.user_profile_popup_scroll())
        .unwrap_or(u16::MAX)
        .min(PROFILE_POPUP_AVATAR_HEIGHT);
    let visible_height = PROFILE_POPUP_AVATAR_HEIGHT
        .saturating_sub(top_clip_rows)
        .min(content.height);
    (visible_height > 0).then_some((
        Rect {
            x: content.x,
            y: content.y,
            width: PROFILE_POPUP_AVATAR_WIDTH.min(content.width),
            height: visible_height,
        },
        top_clip_rows,
    ))
}

/// Geometry the scroll-clamping pass needs: the inner text rect plus the
/// available width that `user_profile_popup_text` will lay out into.
pub(in crate::tui::ui) fn user_profile_popup_text_geometry(
    area: Rect,
    state: &DashboardState,
) -> (u16, u16) {
    let content = user_profile_popup_areas_for_frame(area, state).document;
    (content.width.saturating_sub(1).max(1), content.height)
}

fn user_profile_popup_text_for_render(
    state: &DashboardState,
    width: u16,
    has_avatar: bool,
    emoji_images: &[EmojiImage<'_>],
) -> UserProfilePopupText {
    if let Some(profile) = state.user_profile_popup_data() {
        user_profile_popup_text(
            profile,
            state,
            width,
            state.user_profile_popup_status(),
            state.user_profile_popup_activities(),
            emoji_images,
            has_avatar,
        )
    } else if let Some(message) = state.user_profile_popup_load_error() {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                truncate_display_width(&format!("Failed to load profile: {message}"), width.into()),
                theme::current().style(theme::HighlightGroup::Error),
            ))],
            emoji_overlays: Vec::new(),
            cursor: None,
            reveal_rows: None,
            picker_rows: None,
        }
    } else {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                "Loading profile...",
                theme::current().style(theme::HighlightGroup::Loading),
            ))],
            emoji_overlays: Vec::new(),
            cursor: None,
            reveal_rows: None,
            picker_rows: None,
        }
    }
}

/// Layout data used by `sync_view_heights` to clamp the viewport and reveal
/// the active field through the same scroll state used by other form popups.
pub(in crate::tui::ui) struct UserProfilePopupMetrics {
    pub total_lines: usize,
    pub reveal_rows: Option<std::ops::Range<usize>>,
    pub selected_picker_line: Option<usize>,
}

pub(in crate::tui::ui) fn user_profile_popup_metrics(
    state: &DashboardState,
    width: u16,
    has_avatar: bool,
) -> UserProfilePopupMetrics {
    let text = user_profile_popup_text_for_render(state, width, has_avatar, &[]);
    let selected_picker_line = text.picker_rows.as_ref().and_then(|rows| {
        let selected = state.active_selectable_popup_snapshot()?.selected;
        Some(
            rows.start
                .saturating_add(selected.min(rows.len().saturating_sub(1))),
        )
    });
    UserProfilePopupMetrics {
        total_lines: text.lines.len(),
        reveal_rows: text.reveal_rows,
        selected_picker_line,
    }
}

pub(in crate::tui::ui) fn user_profile_picker_list_layout(
    area: Rect,
    state: &DashboardState,
    snapshot: SelectablePopupSnapshot,
) -> SelectablePopupLayout {
    let popup = user_profile_popup_area(area);
    let content = user_profile_popup_areas_for_frame(area, state).document;
    let has_avatar = user_profile_popup_has_avatar_inside(
        content,
        state.show_avatars() && state.user_profile_popup_has_avatar_preview(),
    );
    let list = Rect {
        width: content.width.saturating_sub(1).max(1),
        ..content
    };
    let text = user_profile_popup_text_for_render(state, list.width, has_avatar, &[]);
    let picker_rows = text.picker_rows.unwrap_or(0..0);
    let document_scroll = state.user_profile_popup_scroll();
    let row_items = (0..usize::from(list.height))
        .map(|offset| {
            let line = document_scroll.saturating_add(offset);
            picker_rows
                .contains(&line)
                .then_some(line.saturating_sub(picker_rows.start))
        })
        .collect::<Vec<_>>();
    let scroll = row_items
        .iter()
        .flatten()
        .copied()
        .next()
        .unwrap_or(snapshot.scroll);
    SelectablePopupLayout {
        target: snapshot.target,
        popup,
        list,
        scroll,
        row_items,
    }
}

#[cfg(test)]
pub(in crate::tui::ui) fn user_profile_popup_lines(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, &[], &[], false).lines
}

#[cfg(test)]
pub(in crate::tui::ui) fn user_profile_popup_lines_with_activities(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, activities, &[], false).lines
}

pub(in crate::tui::ui) fn user_profile_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
    emoji_images: &[EmojiImage<'_>],
    has_avatar: bool,
) -> UserProfilePopupText {
    let is_self = state.current_user_id() == Some(profile.user_id);

    let inner_width = usize::from(width.max(8));
    let mut lines: Vec<Line<'static>> = Vec::new();

    if is_self {
        return user_profile_settings_popup_text(profile, state, inner_width, has_avatar);
    }

    push_profile_identity_lines(&mut lines, profile, status, inner_width, has_avatar, true);
    push_server_profile_section(&mut lines, profile, state, inner_width);

    let mut emoji_overlays: Vec<(usize, String)> = Vec::new();
    if !activities.is_empty() {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(&mut lines, "ACTIVITY", inner_width);
        let mut sorted_activities: Vec<&ActivityInfo> = activities.iter().collect();
        sorted_activities.sort_by_key(|a| activity_priority(a.kind));
        let mut first = true;
        for activity in sorted_activities {
            if !first {
                lines.push(Line::from(Span::raw(String::new())));
            }
            first = false;
            push_activity_lines(
                &mut lines,
                &mut emoji_overlays,
                activity,
                inner_width,
                emoji_images,
            );
        }
    }

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "ABOUT ME", inner_width);
    push_wrapped_paragraph(
        &mut lines,
        profile
            .bio
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no bio)"),
        inner_width,
    );

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "NOTE", inner_width);
    push_wrapped_paragraph(
        &mut lines,
        profile
            .note
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no note)"),
        inner_width,
    );

    lines.push(Line::from(Span::raw(String::new())));
    push_social_section(&mut lines, profile, state, inner_width);

    UserProfilePopupText {
        lines,
        emoji_overlays,
        cursor: None,
        reveal_rows: None,
        picker_rows: None,
    }
}

fn user_profile_settings_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: usize,
    has_avatar: bool,
) -> UserProfilePopupText {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cursor = None;
    let mut reveal_rows = None;
    push_profile_identity_lines(
        &mut lines,
        profile,
        state.user_profile_popup_status(),
        width,
        has_avatar,
        false,
    );
    lines.push(Line::from(Span::raw(String::new())));

    let active_tab = state.user_profile_settings_tab();
    lines.push(Line::from(vec![
        profile_tab_span("g", "Global", active_tab == UserProfileSettingsTab::Global),
        Span::raw("  "),
        profile_tab_span(
            "v",
            "This Server",
            active_tab == UserProfileSettingsTab::Guild,
        ),
    ]));
    lines.push(Line::from(Span::raw(String::new())));

    let mut picker_rows = None;
    match active_tab {
        UserProfileSettingsTab::Global => {
            push_section_header(&mut lines, "PROFILE", width);
            push_profile_settings_field_lines(
                &mut lines,
                &mut cursor,
                &mut reveal_rows,
                state,
                width,
                &[
                    (UserProfileSettingsField::GlobalDisplayName, "Display name"),
                    (UserProfileSettingsField::GlobalPronouns, "Pronouns"),
                    (
                        UserProfileSettingsField::GlobalAvatarPath,
                        "Avatar image path or paste image",
                    ),
                ],
            );
            lines.push(Line::default());
            push_section_header(&mut lines, "PRESENCE", width);
            push_profile_settings_field_lines(
                &mut lines,
                &mut cursor,
                &mut reveal_rows,
                state,
                width,
                &[(UserProfileSettingsField::CurrentStatus, "Status")],
            );
            let status_rows = state.user_profile_status_picker_rows();
            if !status_rows.is_empty() {
                let start = lines.len().saturating_add(2);
                push_profile_status_picker_lines(&mut lines, width, &status_rows);
                picker_rows = Some(start..start.saturating_add(status_rows.len()));
            }

            lines.push(Line::default());
            push_profile_settings_field_lines(
                &mut lines,
                &mut cursor,
                &mut reveal_rows,
                state,
                width,
                &[(UserProfileSettingsField::ManualActivity, "Activity")],
            );
            let activity_rows = state.user_profile_activity_picker_rows();
            if !activity_rows.is_empty() {
                let start = lines.len().saturating_add(2);
                push_profile_activity_picker_lines(&mut lines, width, &activity_rows);
                picker_rows = Some(start..start.saturating_add(activity_rows.len()));
            }
        }
        UserProfileSettingsTab::Guild => {
            push_section_header(&mut lines, "SERVER PROFILE", width);
            if state.user_profile_popup_guild_id().is_none() {
                lines.push(Line::from(Span::styled(
                    "Server profile is available only inside a server.",
                    theme::current().style(theme::HighlightGroup::Disabled),
                )));
            } else {
                push_profile_settings_field_lines(
                    &mut lines,
                    &mut cursor,
                    &mut reveal_rows,
                    state,
                    width,
                    &[
                        (UserProfileSettingsField::GuildNickname, "Server nickname"),
                        (UserProfileSettingsField::GuildPronouns, "Server pronouns"),
                    ],
                );
            }
        }
    }

    lines.push(Line::default());
    push_profile_settings_action_lines(&mut lines, &mut reveal_rows, state);

    UserProfilePopupText {
        lines,
        emoji_overlays: Vec::new(),
        cursor,
        reveal_rows,
        picker_rows,
    }
}

fn push_profile_settings_action_lines(
    lines: &mut Vec<Line<'static>>,
    reveal_rows: &mut Option<std::ops::Range<usize>>,
    state: &DashboardState,
) {
    let active = state.user_profile_settings_active_field();
    let start = lines.len();
    lines.extend([
        popup_button_line("s", "Save", active == Some(UserProfileSettingsField::Save)),
        popup_button_line(
            &state.key_bindings().popup_close_key_label(),
            "Close",
            active == Some(UserProfileSettingsField::Close),
        ),
        popup_danger_button_line(
            "o",
            "Sign out",
            active == Some(UserProfileSettingsField::SignOut),
        ),
    ]);

    let selected_row = match active {
        Some(UserProfileSettingsField::Save) => Some(start),
        Some(UserProfileSettingsField::Close) => Some(start.saturating_add(1)),
        Some(UserProfileSettingsField::SignOut) => Some(start.saturating_add(2)),
        _ => None,
    };
    if let Some(row) = selected_row {
        *reveal_rows = Some(row..row.saturating_add(1));
    }
}

fn push_profile_status_picker_lines(
    lines: &mut Vec<Line<'static>>,
    width: usize,
    rows: &[(PresenceStatus, bool)],
) {
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "Choose status",
        theme::current().style(theme::HighlightGroup::Heading),
    )));
    for (status, selected) in rows {
        let style = selected_presence_style(*selected, *status);
        let marker = selectable_popup_marker(*selected);
        let label_width = width.saturating_sub(marker.content.width());
        lines.push(selected_row_line(
            Line::from(vec![
                marker,
                Span::styled(truncate_display_width(status.label(), label_width), style),
            ]),
            *selected,
        ));
    }
}

fn push_profile_activity_picker_lines(
    lines: &mut Vec<Line<'static>>,
    width: usize,
    rows: &[(String, bool)],
) {
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "Choose activity",
        theme::current().style(theme::HighlightGroup::Heading),
    )));
    for (label, selected) in rows {
        let marker = selectable_popup_marker(*selected);
        let label_width = width.saturating_sub(marker.content.width());
        lines.push(selected_row_line(
            Line::from(vec![
                marker,
                Span::styled(
                    truncate_display_width(label, label_width),
                    selected_text_style(*selected, Style::default()),
                ),
            ]),
            *selected,
        ));
    }
}

fn profile_tab_span(shortcut: &str, label: &str, active: bool) -> Span<'static> {
    let text = if active {
        format!("[{shortcut}] {label}")
    } else {
        format!(" {shortcut}  {label}")
    };
    Span::styled(
        text,
        if active {
            theme::current().style(theme::HighlightGroup::ActiveTab)
        } else {
            theme::current().style(theme::HighlightGroup::Disabled)
        },
    )
}

fn push_profile_settings_field_lines(
    lines: &mut Vec<Line<'static>>,
    cursor: &mut Option<(usize, usize)>,
    reveal_rows: &mut Option<std::ops::Range<usize>>,
    state: &DashboardState,
    width: usize,
    fields: &[(UserProfileSettingsField, &str)],
) {
    const INLINE_FIELD_MIN_WIDTH: usize = 36;
    const INLINE_LABEL_WIDTH: usize = 17;

    let active = state.user_profile_settings_active_field();
    let editing = state.user_profile_settings_editing_field();
    for (index, (field, label)) in fields.iter().enumerate() {
        let field_start = lines.len();
        let selected = active == Some(*field);
        let value = state.user_profile_settings_field_value(*field);
        let is_editing = editing == Some(*field);
        let label_style = editable_field_label_style(selected, is_editing);
        let marker_style = if is_editing {
            theme::current().style(theme::HighlightGroup::Editing)
        } else if selected {
            theme::current().style(theme::HighlightGroup::ActiveField)
        } else {
            theme::current().style(theme::HighlightGroup::Disabled)
        };
        let display = if is_editing {
            value.as_str()
        } else if value.is_empty() {
            "(not set)"
        } else {
            &value
        };
        let value_style = if is_editing {
            theme::current().style(theme::HighlightGroup::Editing)
        } else if selected {
            theme::current().style(theme::HighlightGroup::ActiveField)
        } else if value.is_empty() {
            theme::current().style(theme::HighlightGroup::Placeholder)
        } else if *field == UserProfileSettingsField::CurrentStatus {
            theme::current().apply(
                theme::HighlightGroup::Muted,
                presence_style(state.user_profile_settings_presence_status()),
            )
        } else {
            theme::current().style(theme::HighlightGroup::Description)
        };

        if width >= INLINE_FIELD_MIN_WIDTH && label.width() <= INLINE_LABEL_WIDTH {
            let separator = " │ ";
            let value_width = width
                .saturating_sub(2)
                .saturating_sub(INLINE_LABEL_WIDTH)
                .saturating_sub(separator.width())
                .max(1);
            let label = truncate_display_width(label, INLINE_LABEL_WIDTH);
            let label_padding = INLINE_LABEL_WIDTH.saturating_sub(label.width());
            let mut value_lines = wrapped_profile_field_value(display, value_width);
            let cursor_position = is_editing.then(|| {
                wrapped_profile_field_cursor(
                    &value,
                    &value_lines,
                    state.user_profile_settings_edit_cursor_byte_index(),
                    value_width,
                )
            });
            if cursor_position.is_some_and(|(row, _)| row >= value_lines.len()) {
                value_lines.push(WrappedTextLine::empty());
            }
            let value_column = 2 + INLINE_LABEL_WIDTH + separator.width();
            let first_value = value_lines
                .first()
                .map(|line| line.text.as_str())
                .unwrap_or_default()
                .to_owned();
            lines.push(Line::from(vec![
                Span::styled(editable_field_marker(selected), marker_style),
                Span::styled(format!("{label}{}", " ".repeat(label_padding)), label_style),
                Span::styled(
                    separator,
                    theme::current().style(theme::HighlightGroup::ModalBorder),
                ),
                Span::styled(first_value, value_style),
            ]));
            for continuation in value_lines.iter().skip(1) {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(value_column)),
                    Span::styled(continuation.text.clone(), value_style),
                ]));
            }
            if let Some((row, column)) = cursor_position {
                *cursor = Some((
                    lines.len().saturating_sub(value_lines.len()) + row,
                    value_column + column,
                ));
            }
        } else {
            lines.push(Line::from(vec![
                Span::styled(editable_field_marker(selected), marker_style),
                Span::styled(
                    truncate_display_width(label, width.saturating_sub(2).max(1)),
                    label_style,
                ),
            ]));
            let value_prefix = "  │ ";
            let value_width = width.saturating_sub(value_prefix.width()).max(1);
            let mut value_lines = wrapped_profile_field_value(display, value_width);
            let cursor_position = is_editing.then(|| {
                wrapped_profile_field_cursor(
                    &value,
                    &value_lines,
                    state.user_profile_settings_edit_cursor_byte_index(),
                    value_width,
                )
            });
            if cursor_position.is_some_and(|(row, _)| row >= value_lines.len()) {
                value_lines.push(WrappedTextLine::empty());
            }
            for value_line in &value_lines {
                lines.push(Line::from(vec![
                    Span::styled(
                        value_prefix,
                        theme::current().style(theme::HighlightGroup::ModalBorder),
                    ),
                    Span::styled(value_line.text.clone(), value_style),
                ]));
            }
            if let Some((row, column)) = cursor_position {
                *cursor = Some((
                    lines.len().saturating_sub(value_lines.len()) + row,
                    value_prefix.width() + column,
                ));
            }
        }

        if *field == UserProfileSettingsField::GlobalAvatarPath && selected {
            lines.push(truncate_line_to_display_width(
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "[Ctrl+V] ",
                        theme::current().style(theme::HighlightGroup::Shortcut),
                    ),
                    Span::styled(
                        "Paste image",
                        theme::current().style(theme::HighlightGroup::Description),
                    ),
                ]),
                width.max(1),
            ));
        }

        if selected {
            *reveal_rows = if is_editing {
                cursor.map(|(row, _)| row..row.saturating_add(1))
            } else {
                Some(field_start..lines.len())
            };
        }

        if index + 1 < fields.len() {
            lines.push(Line::default());
        }
    }
}

fn wrapped_profile_field_value(value: &str, width: usize) -> Vec<WrappedTextLine> {
    if value.is_empty() {
        return vec![WrappedTextLine::empty()];
    }
    wrap_text_with_metadata(value, &[], &[], width.max(1))
}

fn wrapped_profile_field_cursor(
    value: &str,
    lines: &[WrappedTextLine],
    cursor_byte: usize,
    width: usize,
) -> (usize, usize) {
    let cursor_byte = cursor_byte.min(value.len());
    let row = lines
        .iter()
        .rposition(|line| cursor_byte >= line.source_start && cursor_byte <= line.source_end)
        .unwrap_or_else(|| lines.len().saturating_sub(1));
    let line = &lines[row];
    let start = line.source_start.min(cursor_byte);
    let prefix = value.get(start..cursor_byte).unwrap_or_default();
    let column = prefix.width();
    if column >= width.max(1) {
        (row.saturating_add(1), 0)
    } else {
        (row, column)
    }
}

pub(in crate::tui::ui) fn user_profile_display_name_style(status: PresenceStatus) -> Style {
    let mut style = theme::current().style(theme::HighlightGroup::Strong);
    if matches!(status, PresenceStatus::Offline | PresenceStatus::Unknown) {
        style = theme::current().apply(theme::HighlightGroup::Muted, style);
    }
    style
}

fn friend_status_badge(status: FriendStatus) -> (String, Style) {
    let theme = theme::current();
    match status {
        FriendStatus::Friend => (
            "● Friend".to_owned(),
            theme.style(theme::HighlightGroup::RelationshipFriend),
        ),
        FriendStatus::IncomingRequest => (
            "● Incoming friend request".to_owned(),
            theme.style(theme::HighlightGroup::RelationshipIncoming),
        ),
        FriendStatus::OutgoingRequest => (
            "● Outgoing friend request".to_owned(),
            theme.style(theme::HighlightGroup::RelationshipOutgoing),
        ),
        FriendStatus::Blocked => (
            "● Blocked".to_owned(),
            theme.style(theme::HighlightGroup::RelationshipBlocked),
        ),
        FriendStatus::None | FriendStatus::Implicit => (
            "● Not friends".to_owned(),
            theme.style(theme::HighlightGroup::RelationshipNone),
        ),
    }
}

fn push_section_header(lines: &mut Vec<Line<'static>>, label: &str, width: usize) {
    let label = format!(" {label} ");
    let rule_width = width.saturating_sub(label.width());
    lines.push(Line::from(vec![
        Span::styled(
            truncate_display_width(&label, width),
            theme::current().style(theme::HighlightGroup::Heading),
        ),
        Span::styled(
            "─".repeat(rule_width),
            theme::current().style(theme::HighlightGroup::ModalBorder),
        ),
    ]));
}

fn push_profile_identity_lines(
    lines: &mut Vec<Line<'static>>,
    profile: &UserProfileInfo,
    status: PresenceStatus,
    width: usize,
    has_avatar: bool,
    show_relationship: bool,
) {
    let start = lines.len();
    let indent = if has_avatar {
        usize::from(PROFILE_POPUP_AVATAR_WIDTH.saturating_add(2))
    } else {
        0
    };
    let available = width.saturating_sub(indent).max(1);
    let display_name = truncate_display_width(
        &sanitize_for_display_width(profile.display_name()),
        available,
    );
    let presence = format!("{} {}", presence_marker(status), status.label());
    let name_width = display_name.width();
    let presence_width = presence.width();
    if name_width.saturating_add(presence_width).saturating_add(2) <= available {
        lines.push(profile_identity_line(
            indent,
            vec![
                Span::styled(display_name, user_profile_display_name_style(status)),
                Span::raw(
                    " ".repeat(available.saturating_sub(name_width.saturating_add(presence_width))),
                ),
                Span::styled(presence, presence_style(status)),
            ],
        ));
    } else {
        lines.push(profile_identity_line(
            indent,
            vec![Span::styled(
                display_name,
                user_profile_display_name_style(status),
            )],
        ));
        lines.push(profile_identity_line(
            indent,
            vec![Span::styled(
                truncate_display_width(&presence, available),
                presence_style(status),
            )],
        ));
    }

    let username = sanitize_for_display_width(&profile.username);
    let pronouns = profile
        .guild_pronouns
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            profile
                .pronouns
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })
        .map(sanitize_for_display_width);
    let account = pronouns.map_or_else(
        || format!("@{username}"),
        |pronouns| format!("@{username}  ·  {pronouns}"),
    );
    lines.push(profile_identity_line(
        indent,
        vec![Span::styled(
            truncate_display_width(&account, available),
            theme::current().style(theme::HighlightGroup::Description),
        )],
    ));

    if show_relationship {
        let (friend_badge, friend_style) = friend_status_badge(profile.friend_status);
        lines.push(profile_identity_line(
            indent,
            vec![Span::styled(
                truncate_display_width(&friend_badge, available),
                friend_style,
            )],
        ));
    }

    if has_avatar {
        let avatar_height = usize::from(PROFILE_POPUP_AVATAR_HEIGHT);
        while lines.len().saturating_sub(start) < avatar_height {
            lines.push(Line::default());
        }
    }
}

fn profile_identity_line(indent: usize, mut spans: Vec<Span<'static>>) -> Line<'static> {
    if indent > 0 {
        spans.insert(0, Span::raw(" ".repeat(indent)));
    }
    Line::from(spans)
}

fn push_server_profile_section(
    lines: &mut Vec<Line<'static>>,
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: usize,
) {
    if state.user_profile_popup_guild_id().is_none() {
        return;
    }

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(lines, "SERVER PROFILE", width);
    let nickname = profile
        .guild_nick
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(sanitize_for_display_width)
        .unwrap_or_else(|| "(none)".to_owned());
    lines.push(popup_form_summary_line(
        "Nickname", false, &nickname, None, false, true, width,
    ));

    let roles = state.user_profile_popup_roles();
    push_profile_role_lines(lines, roles.as_deref(), width);
}

fn push_profile_role_lines(
    lines: &mut Vec<Line<'static>>,
    roles: Option<&[&RoleState]>,
    width: usize,
) {
    const PREFIX: &str = "  Roles  ";
    let prefix_width = PREFIX.width();
    let placeholder = match roles {
        Some([]) => Some("(none)"),
        None => Some("(unavailable)"),
        Some(_) => None,
    };
    if let Some(placeholder) = placeholder {
        lines.push(Line::from(vec![
            Span::styled(PREFIX, popup_form_field_label_style(false, false)),
            Span::styled(
                truncate_display_width(placeholder, width.saturating_sub(prefix_width)),
                theme::current().style(theme::HighlightGroup::Placeholder),
            ),
        ]));
        return;
    }

    let roles = roles.expect("role availability is checked above");
    let available = width.saturating_sub(prefix_width).max(1);
    let max_label_width = available.saturating_sub(2).max(1);
    let mut spans = vec![Span::styled(
        PREFIX,
        popup_form_field_label_style(false, false),
    )];
    let mut line_width = prefix_width;

    for role in roles {
        let name = sanitize_for_display_width(&role.name);
        let chip = format!("[{}]", truncate_display_width(&name, max_label_width));
        let chip_width = chip.width();
        let separator_width = usize::from(line_width > prefix_width);

        if line_width > prefix_width && line_width + separator_width + chip_width > width {
            lines.push(Line::from(spans));
            spans = vec![Span::raw(" ".repeat(prefix_width))];
            line_width = prefix_width;
        }

        if line_width > prefix_width {
            spans.push(Span::raw(" "));
            line_width += 1;
        }
        spans.push(Span::styled(
            chip,
            apply_discord_foreground(
                theme::current().style(theme::HighlightGroup::Strong),
                role.color,
            ),
        ));
        line_width += chip_width;
    }

    if line_width > prefix_width {
        lines.push(Line::from(spans));
    }
}

fn push_social_section(
    lines: &mut Vec<Line<'static>>,
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: usize,
) {
    push_section_header(lines, "SOCIAL", width);
    lines.push(popup_form_summary_line(
        "Mutual servers",
        false,
        &profile.mutual_guilds.len().to_string(),
        None,
        false,
        true,
        width,
    ));
    for mutual in &profile.mutual_guilds {
        let guild_name = state
            .guild_name(mutual.guild_id)
            .map(sanitize_for_display_width)
            .unwrap_or_else(|| format!("guild-{}", mutual.guild_id.get()));
        let label = mutual
            .nick
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(sanitize_for_display_width)
            .map_or(guild_name.clone(), |nick| {
                format!("{guild_name}  ·  {nick}")
            });
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  • {label}"), width),
            theme::current().style(theme::HighlightGroup::Description),
        )));
    }
    let mutual_friend_count = profile
        .mutual_friends_count
        .max(u32::try_from(profile.mutual_friends.len()).unwrap_or(u32::MAX));
    lines.push(popup_form_summary_line(
        "Mutual friends",
        false,
        &mutual_friend_count.to_string(),
        None,
        false,
        true,
        width,
    ));
    for friend in &profile.mutual_friends {
        let display_name = sanitize_for_display_width(friend.display_name());
        let username = sanitize_for_display_width(&friend.username);
        let label = if display_name == username {
            format!("@{username}")
        } else {
            format!("{display_name}  ·  @{username}")
        };
        lines.push(Line::from(Span::styled(
            truncate_display_width(&format!("  • {label}"), width),
            theme::current().style(theme::HighlightGroup::Description),
        )));
    }
}

fn push_activity_lines(
    lines: &mut Vec<Line<'static>>,
    emoji_overlays: &mut Vec<(usize, String)>,
    activity: &ActivityInfo,
    width: usize,
    emoji_images: &[EmojiImage<'_>],
) {
    let render = build_activity_render(activity, emoji_images, false);
    if !render.is_empty() {
        let line_index = lines.len();
        // The leading marker costs 2 columns, either a 2-cell image or an icon
        // plus one space. The plain-body variant gets the full width.
        let line = match render.leading {
            ActivityLeading::Image(url) => {
                emoji_overlays.push((line_index, url));
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        truncate_display_width(&render.body, width.saturating_sub(2)),
                        theme::current().style(theme::HighlightGroup::Activity),
                    ),
                ])
            }
            ActivityLeading::Icon(icon) => Line::from(vec![
                Span::styled(icon.to_string(), Style::default()),
                Span::raw(" "),
                Span::styled(
                    truncate_display_width(&render.body, width.saturating_sub(2)),
                    theme::current().style(theme::HighlightGroup::Activity),
                ),
            ]),
            ActivityLeading::None => Line::from(Span::styled(
                truncate_display_width(&render.body, width),
                theme::current().style(theme::HighlightGroup::Activity),
            )),
        };
        lines.push(line);
    }
    if let Some(secondary) = activity_secondary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&secondary, width),
            theme::current().style(theme::HighlightGroup::Activity),
        )));
    }
    if let Some(tertiary) = activity_tertiary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&tertiary, width),
            theme::current().style(theme::HighlightGroup::Activity),
        )));
    }
}

fn activity_secondary_line(activity: &ActivityInfo) -> Option<String> {
    match activity.kind {
        ActivityKind::Custom => None,
        _ => activity.details.clone(),
    }
}

fn activity_tertiary_line(activity: &ActivityInfo) -> Option<String> {
    match activity.kind {
        ActivityKind::Custom => None,
        ActivityKind::Listening => activity
            .state
            .as_deref()
            .map(|artist| format!("by {artist}")),
        ActivityKind::Streaming => activity.url.clone(),
        _ => activity.state.clone(),
    }
}

fn push_wrapped_paragraph(lines: &mut Vec<Line<'static>>, text: &str, width: usize) {
    for line in text.split('\n') {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(Span::raw(String::new())));
        } else {
            push_wrapped_styled_popup_text(lines, trimmed, width, Style::default());
        }
    }
}

/// Profile-popup ordering intentionally differs from the compact sidebar
/// ordering. The popup has room to lead with Custom Status, while sidebar rows
/// prefer game-at-a-glance signals.
fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Custom => 0,
        ActivityKind::Streaming => 1,
        ActivityKind::Playing => 2,
        ActivityKind::Listening => 3,
        ActivityKind::Watching => 4,
        ActivityKind::Competing => 5,
        ActivityKind::Hang => 6,
        ActivityKind::Unknown(_) => 7,
    }
}
