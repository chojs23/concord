use super::*;

pub(in crate::tui::ui) fn render_user_profile_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    avatar: Option<AvatarImage>,
    emoji_images: &[EmojiImage<'_>],
) {
    if !state.is_user_profile_popup_open() {
        return;
    }

    const AVATAR_CELL_HEIGHT: u16 = 4;
    let popup = user_profile_popup_area(area);
    frame.render_widget(Clear, popup);

    let block = panel_block("Profile", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // The avatar sits inside the inner area, so reserve a fixed column gutter
    // so the text section starts cleanly to its right.
    let has_avatar = user_profile_popup_has_avatar_inside(
        inner,
        state.show_avatars() && state.user_profile_popup_avatar_url().is_some(),
    );
    let text_area = user_profile_popup_text_area_inside(inner, has_avatar);

    let popup_text = user_profile_popup_text_for_render(state, text_area.width, emoji_images);
    let total_lines = popup_text.lines.len();
    let viewport = text_area.height as usize;
    let scroll_position = state
        .user_profile_popup_scroll()
        .min(total_lines.saturating_sub(viewport));
    let lines = popup_text
        .lines
        .into_iter()
        .skip(scroll_position)
        .take(viewport)
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    render_vertical_scrollbar(frame, text_area, scroll_position, viewport, total_lines);

    if state.show_custom_emoji() {
        for (line_idx, url) in &popup_text.emoji_overlays {
            let Some(image) = emoji_images.iter().find(|img| img.url == *url) else {
                continue;
            };
            let Some(visible_offset) = line_idx.checked_sub(scroll_position) else {
                continue;
            };
            if visible_offset >= viewport {
                continue;
            }
            let y = text_area.y.saturating_add(visible_offset as u16);
            frame.render_widget(
                ratatui_image::Image::new(image.protocol),
                Rect::new(text_area.x, y, 2, 1),
            );
        }
    }

    if let Some(avatar) = avatar.filter(|_| has_avatar) {
        let avatar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: USER_PROFILE_POPUP_AVATAR_CELL_WIDTH.min(inner.width),
            height: AVATAR_CELL_HEIGHT.min(inner.height),
        };
        frame.render_widget(RatatuiImage::new(avatar.protocol), avatar_area);
    }
}

const USER_PROFILE_POPUP_WIDTH: u16 = 60;
const USER_PROFILE_POPUP_HEIGHT: u16 = 24;
const USER_PROFILE_POPUP_AVATAR_CELL_WIDTH: u16 = 8;

/// Centered popup rect inside the messages area. Shared so the geometry
/// computation lives in one place and the scroll-clamping pass uses the
/// exact same width/height the renderer ends up drawing into.
pub(in crate::tui::ui) fn user_profile_popup_area(area: Rect) -> Rect {
    let width = USER_PROFILE_POPUP_WIDTH
        .min(area.width.saturating_sub(2))
        .max(8);
    let height = USER_PROFILE_POPUP_HEIGHT
        .min(area.height.saturating_sub(2))
        .max(6);
    centered_rect(area, width, height)
}

pub(in crate::tui::ui) fn user_profile_popup_has_avatar(area: Rect, has_avatar_url: bool) -> bool {
    let popup = user_profile_popup_area(area);
    let inner = panel_block("Profile", true).inner(popup);
    user_profile_popup_has_avatar_inside(inner, has_avatar_url)
}

fn user_profile_popup_has_avatar_inside(inner: Rect, has_avatar_url: bool) -> bool {
    has_avatar_url && inner.width > USER_PROFILE_POPUP_AVATAR_CELL_WIDTH + 2
}

fn user_profile_popup_text_area_inside(inner: Rect, has_avatar: bool) -> Rect {
    if has_avatar {
        let gutter = USER_PROFILE_POPUP_AVATAR_CELL_WIDTH + 2;
        Rect {
            x: inner.x + gutter,
            y: inner.y,
            width: inner.width.saturating_sub(gutter),
            height: inner.height,
        }
    } else {
        inner
    }
}

/// Geometry the scroll-clamping pass needs: the inner text rect plus the
/// available width that `user_profile_popup_text` will lay out into.
pub(in crate::tui::ui) fn user_profile_popup_text_geometry(
    area: Rect,
    has_avatar: bool,
) -> (u16, u16) {
    let popup = user_profile_popup_area(area);
    let inner = panel_block("Profile", true).inner(popup);
    let text_area = user_profile_popup_text_area_inside(inner, has_avatar);
    (text_area.width, text_area.height)
}

fn user_profile_popup_text_for_render(
    state: &DashboardState,
    width: u16,
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
        )
    } else if let Some(message) = state.user_profile_popup_load_error() {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                truncate_display_width(&format!("Failed to load profile: {message}"), width.into()),
                Style::default().fg(Color::Red),
            ))],
            emoji_overlays: Vec::new(),
        }
    } else {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                "Loading profile...",
                Style::default().fg(DIM),
            ))],
            emoji_overlays: Vec::new(),
        }
    }
}

/// Counts the lines the popup will draw, mirroring
/// `user_profile_popup_text_for_render` so the scroll-clamping pass in
/// `sync_view_heights` matches the eventual render exactly.
pub(in crate::tui::ui) fn user_profile_popup_total_lines(
    state: &DashboardState,
    width: u16,
) -> usize {
    user_profile_popup_text_for_render(state, width, &[])
        .lines
        .len()
}

#[cfg(test)]
pub(in crate::tui::ui) fn user_profile_popup_lines(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, &[], &[]).lines
}

#[cfg(test)]
pub(in crate::tui::ui) fn user_profile_popup_lines_with_activities(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, activities, &[]).lines
}

pub(in crate::tui::ui) fn user_profile_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    activities: &[ActivityInfo],
    emoji_images: &[EmojiImage<'_>],
) -> UserProfilePopupText {
    let is_self = state.current_user_id() == Some(profile.user_id);

    let inner_width = usize::from(width.max(8));
    let mut lines: Vec<Line<'static>> = Vec::new();

    let display_name = profile.display_name().to_owned();
    lines.push(Line::from(Span::styled(
        truncate_display_width(&display_name, inner_width),
        user_profile_display_name_style(status),
    )));
    lines.push(Line::from(Span::styled(
        truncate_display_width(&format!("@{}", profile.username), inner_width),
        Style::default().fg(DIM),
    )));

    if let Some(pronouns) = profile.pronouns.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate_display_width(pronouns, inner_width),
            Style::default().fg(DIM),
        )));
    }

    if !is_self {
        let (badge_label, badge_color) = friend_status_badge(profile.friend_status);
        lines.push(Line::from(Span::styled(
            badge_label,
            Style::default()
                .fg(badge_color)
                .add_modifier(Modifier::BOLD),
        )));
    }

    let mut emoji_overlays: Vec<(usize, String)> = Vec::new();
    if !activities.is_empty() {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(&mut lines, "ACTIVITY");
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
    push_section_header(&mut lines, "ABOUT ME");
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
    push_section_header(&mut lines, "NOTE");
    push_wrapped_paragraph(
        &mut lines,
        profile
            .note
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no note)"),
        inner_width,
    );

    if !is_self {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(
            &mut lines,
            &format!("MUTUAL SERVERS ({})", profile.mutual_guilds.len()),
        );
        if profile.mutual_guilds.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (none)".to_owned(),
                Style::default().fg(DIM),
            )));
        } else {
            for entry in &profile.mutual_guilds {
                let name = state
                    .guild_name(entry.guild_id)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("guild-{}", entry.guild_id.get()));
                let body = match entry.nick.as_deref() {
                    Some(nick) => format!("• {name} - {nick}"),
                    None => format!("• {name}"),
                };
                lines.push(Line::from(vec![
                    Span::styled("  ".to_owned(), Style::default().fg(ACCENT)),
                    Span::styled(
                        truncate_display_width(&body, inner_width.saturating_sub(2)),
                        Style::default(),
                    ),
                ]));
            }
        }
    }

    if !is_self {
        lines.push(Line::from(Span::raw(String::new())));
        push_section_header(
            &mut lines,
            &format!("MUTUAL FRIENDS ({})", profile.mutual_friends_count),
        );
    }

    UserProfilePopupText {
        lines,
        emoji_overlays,
    }
}

pub(in crate::tui::ui) fn user_profile_display_name_style(status: PresenceStatus) -> Style {
    Style::default()
        .fg(presence_color(status))
        .add_modifier(Modifier::BOLD)
}

fn friend_status_badge(status: FriendStatus) -> (String, Color) {
    match status {
        FriendStatus::Friend => ("● Friend".to_owned(), Color::Green),
        FriendStatus::IncomingRequest => ("● Incoming friend request".to_owned(), Color::Yellow),
        FriendStatus::OutgoingRequest => ("● Outgoing friend request".to_owned(), Color::Yellow),
        FriendStatus::Blocked => ("● Blocked".to_owned(), Color::Red),
        FriendStatus::None => ("● Not friends".to_owned(), DIM),
    }
}

fn push_section_header(lines: &mut Vec<Line<'static>>, label: &str) {
    lines.push(Line::from(Span::styled(
        label.to_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
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
                        Style::default().fg(DIM),
                    ),
                ])
            }
            ActivityLeading::Icon(icon) => Line::from(vec![
                Span::styled(icon.to_string(), Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::styled(
                    truncate_display_width(&render.body, width.saturating_sub(2)),
                    Style::default().fg(DIM),
                ),
            ]),
            ActivityLeading::None => Line::from(Span::styled(
                truncate_display_width(&render.body, width),
                Style::default().fg(DIM),
            )),
        };
        lines.push(line);
    }
    if let Some(secondary) = activity_secondary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&secondary, width),
            Style::default().fg(DIM),
        )));
    }
    if let Some(tertiary) = activity_tertiary_line(activity) {
        lines.push(Line::from(Span::styled(
            truncate_display_width(&tertiary, width),
            Style::default().fg(DIM),
        )));
    }
}

/// Profile-popup ordering. Intentionally differs from
/// `panes::activity_priority`: the popup has the vertical space to lead with
/// the user's Custom Status, while the member-list row uses one line per
/// member and prefers game-at-a-glance signals.
fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Custom => 0,
        ActivityKind::Streaming => 1,
        ActivityKind::Playing => 2,
        ActivityKind::Listening => 3,
        ActivityKind::Watching => 4,
        ActivityKind::Competing => 5,
        ActivityKind::Unknown => 6,
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
            lines.push(Line::from(Span::raw(truncate_display_width(
                trimmed, width,
            ))));
        }
    }
}
