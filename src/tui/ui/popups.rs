use super::message_list::render_image_preview;
use super::*;

pub(super) fn render_image_viewer(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    image_preview: Option<ImagePreview<'_>>,
) {
    let Some(item) = state.selected_image_viewer_item() else {
        return;
    };

    let popup = image_viewer_popup(area);
    let title_width = usize::from(popup.width.saturating_sub(4)).max(1);
    let title = truncate_display_width(&image_viewer_title(&item), title_width);
    frame.render_widget(Clear, popup);
    let block = panel_block_owned(title, true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [image_area, hint_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    if let Some(image_preview) = image_preview {
        render_image_preview(frame, image_area, image_preview.state);
    } else {
        frame.render_widget(
            Paragraph::new(format!("loading {}...", item.filename))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            image_area,
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "h/← previous · l/→ next · Enter/Space actions · Esc close",
            Style::default().fg(DIM),
        )))
        .alignment(Alignment::Center),
        hint_area,
    );
}

fn image_viewer_title(item: &ImageViewerItem) -> String {
    format!("Image {}/{} — {}", item.index, item.total, item.filename)
}

pub(super) fn render_image_viewer_action_menu(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_image_viewer_action_menu_open() {
        return;
    }

    let actions = state.selected_image_viewer_action_items();
    if actions.is_empty() {
        return;
    }
    let Some(selected) = state.selected_image_viewer_action_index() else {
        return;
    };
    let popup = centered_rect(area, 42, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected))
            .block(panel_block("Image actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_message_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_message_action_menu_open() {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }

    let selected = state.selected_message_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected))
            .block(panel_block("Message actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_guild_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_guild_action_menu_open() {
        return;
    }
    let actions = state.selected_guild_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_guild_action_index().unwrap_or(0);
    let title = state
        .guild_action_menu_title()
        .map(|name| format!("Server actions — {name}"))
        .unwrap_or_else(|| "Server actions".to_owned());
    let popup = centered_rect(area, 48, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(guild_action_menu_lines(&actions, selected))
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_channel_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_channel_action_menu_open() {
        return;
    }

    let title_suffix = state
        .channel_action_menu_title()
        .map(|name| format!(" — {name}"))
        .unwrap_or_default();

    if state.is_channel_action_threads_phase() {
        let threads = state.channel_action_thread_items();
        let selected = state.selected_channel_action_index().unwrap_or(0);
        let row_count = threads.len().max(1) as u16;
        let popup = centered_rect(area, 54, row_count.saturating_add(4));
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(channel_thread_menu_lines(&threads, selected))
                .block(panel_block_owned(format!("Threads{title_suffix}"), true))
                .wrap(Wrap { trim: false }),
            popup,
        );
        return;
    }

    let actions = state.selected_channel_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_channel_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(channel_action_menu_lines(&actions, selected))
            .block(panel_block_owned(
                format!("Channel actions{title_suffix}"),
                true,
            ))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_emoji_reaction_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    if !state.is_emoji_reaction_picker_open() {
        return;
    }

    let reactions = state.emoji_reaction_items();
    if reactions.is_empty() {
        return;
    }

    let selected = state.selected_emoji_reaction_index().unwrap_or(0);
    let desired_visible_items = reactions
        .len()
        .clamp(1, super::super::selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
    let popup = centered_rect(area, 42, (desired_visible_items as u16).saturating_add(4));
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    let visible_items = usize::from(content.height.saturating_sub(1)).min(desired_visible_items);
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(emoji_reaction_picker_lines(
            &reactions,
            selected,
            visible_items,
            &ready_urls,
        ))
        .block(block)
        .wrap(Wrap { trim: false }),
        popup,
    );
    render_emoji_reaction_images(
        frame,
        content,
        &reactions,
        selected,
        visible_items,
        emoji_images,
    );
    render_vertical_scrollbar(
        frame,
        Rect {
            height: visible_items as u16,
            ..content
        },
        visible_range.start,
        visible_items,
        reactions.len(),
    );
}

pub(super) fn render_poll_vote_picker(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(answers) = state.poll_vote_picker_items() else {
        return;
    };
    if answers.is_empty() {
        return;
    }

    let selected = state.selected_poll_vote_picker_index().unwrap_or(0);
    let popup = centered_rect(area, 58, (answers.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(poll_vote_picker_lines(answers, selected))
            .block(panel_block("Choose poll votes", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_member_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_member_action_menu_open() {
        return;
    }
    let actions = state.selected_member_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_member_action_index().unwrap_or(0);
    let title = state
        .member_action_menu_title()
        .map(|name| format!("Member actions — {name}"))
        .unwrap_or_else(|| "Member actions".to_owned());
    let popup = centered_rect(area, 48, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(member_action_menu_lines(&actions, selected))
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn guild_action_menu_lines(
    actions: &[GuildActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn member_action_menu_lines(actions: &[MemberActionItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn render_user_profile_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    avatar: Option<AvatarImage>,
) {
    if !state.is_user_profile_popup_open() {
        return;
    }

    const POPUP_WIDTH: u16 = 60;
    const POPUP_HEIGHT: u16 = 24;
    const AVATAR_CELL_WIDTH: u16 = 8;
    const AVATAR_CELL_HEIGHT: u16 = 4;
    let width = POPUP_WIDTH.min(area.width.saturating_sub(2)).max(8);
    let height = POPUP_HEIGHT.min(area.height.saturating_sub(2)).max(6);
    let popup = centered_rect(area, width, height);
    frame.render_widget(Clear, popup);

    let block = panel_block("Profile", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // The avatar sits inside the inner area; reserve a fixed column gutter
    // so the text section starts cleanly to its right.
    let has_avatar = avatar.is_some()
        && state.user_profile_popup_avatar_url().is_some()
        && inner.width > AVATAR_CELL_WIDTH + 2;
    let text_area = if has_avatar {
        let gutter = AVATAR_CELL_WIDTH + 2;
        Rect {
            x: inner.x + gutter,
            y: inner.y,
            width: inner.width.saturating_sub(gutter),
            height: inner.height,
        }
    } else {
        inner
    };

    let popup_text = if let Some(profile) = state.user_profile_popup_data() {
        user_profile_popup_text(
            profile,
            state,
            text_area.width.saturating_sub(0),
            state.user_profile_popup_status(),
            state.user_profile_popup_mutual_cursor(),
        )
    } else if let Some(message) = state.user_profile_popup_load_error() {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                truncate_display_width(
                    &format!("Failed to load profile: {message}"),
                    text_area.width.into(),
                ),
                Style::default().fg(Color::Red),
            ))],
            selected_line: None,
        }
    } else {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                "Loading profile...",
                Style::default().fg(DIM),
            ))],
            selected_line: None,
        }
    };
    let total_lines = popup_text.lines.len();
    let selected_line = popup_text.selected_line;
    let scroll_position =
        user_profile_popup_scroll_position(total_lines, selected_line, text_area.height as usize);
    let lines = user_profile_popup_visible_lines(
        popup_text.lines,
        selected_line,
        text_area.height as usize,
    );
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    render_vertical_scrollbar(
        frame,
        text_area,
        scroll_position,
        text_area.height as usize,
        total_lines,
    );

    if let Some(avatar) = avatar.filter(|_| has_avatar) {
        let avatar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: AVATAR_CELL_WIDTH.min(inner.width),
            height: AVATAR_CELL_HEIGHT.min(inner.height),
        };
        frame.render_widget(RatatuiImage::new(&avatar.protocol), avatar_area);
    }
}

#[cfg(test)]
pub(super) fn user_profile_popup_lines(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    mutual_cursor: Option<usize>,
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, mutual_cursor).lines
}

pub(super) fn user_profile_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    mutual_cursor: Option<usize>,
) -> UserProfilePopupText {
    let inner_width = usize::from(width.max(8));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut selected_line = None;

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

    let (badge_label, badge_color) = friend_status_badge(profile.friend_status);
    lines.push(Line::from(Span::styled(
        badge_label,
        Style::default()
            .fg(badge_color)
            .add_modifier(Modifier::BOLD),
    )));

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
        for (index, entry) in profile.mutual_guilds.iter().enumerate() {
            let name = state
                .guild_name(entry.guild_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("guild-{}", entry.guild_id.get()));
            let selected = mutual_cursor == Some(index);
            let marker = if selected { "› " } else { "  " };
            let body = match entry.nick.as_deref() {
                Some(nick) => format!("• {name} — {nick}"),
                None => format!("• {name}"),
            };
            let mut style = Style::default();
            if selected {
                selected_line = Some(lines.len());
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(vec![
                Span::styled(marker.to_owned(), Style::default().fg(ACCENT)),
                Span::styled(
                    truncate_display_width(&body, inner_width.saturating_sub(2)),
                    style,
                ),
            ]));
        }
    }

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(
        &mut lines,
        &format!("MUTUAL FRIENDS ({})", profile.mutual_friends_count),
    );

    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "j/k pick · Enter open · m send DM · Esc close",
        Style::default().fg(DIM),
    )));
    UserProfilePopupText {
        lines,
        selected_line,
    }
}

pub(super) fn user_profile_popup_visible_lines(
    lines: Vec<Line<'static>>,
    selected_line: Option<usize>,
    visible_height: usize,
) -> Vec<Line<'static>> {
    if visible_height == 0 {
        return Vec::new();
    }

    let scroll = user_profile_popup_scroll_position(lines.len(), selected_line, visible_height);

    lines
        .into_iter()
        .skip(scroll)
        .take(visible_height)
        .collect()
}

fn user_profile_popup_scroll_position(
    total_lines: usize,
    selected_line: Option<usize>,
    visible_height: usize,
) -> usize {
    let max_scroll = total_lines.saturating_sub(visible_height);
    selected_line
        .map(|line| line.saturating_add(1).saturating_sub(visible_height))
        .unwrap_or(0)
        .min(max_scroll)
}

pub(super) fn user_profile_display_name_style(status: PresenceStatus) -> Style {
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

pub(super) fn render_reaction_users_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(popup_state) = state.reaction_users_popup() else {
        return;
    };

    // Compute the popup's eventual inner width up front so we can pre-truncate
    // every line to fit. Without this, ratatui's `Wrap` would split a long
    // username across rows and the wrap continuation overlaps neighbouring
    // lines, producing the trailing-fragment artefact reported by users.
    const POPUP_TARGET_WIDTH: u16 = 58;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));

    let max_visible_lines = reaction_users_visible_line_count(area);
    let lines = reaction_users_popup_lines(
        popup_state.reactions(),
        popup_state.scroll(),
        max_visible_lines,
        inner_width,
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block("Reacted users", true)),
        popup,
    );
    render_vertical_scrollbar(
        frame,
        Rect {
            height: max_visible_lines as u16,
            ..panel_scrollbar_area(popup)
        },
        popup_state.scroll(),
        max_visible_lines,
        popup_state.data_line_count(),
    );
}

pub(super) fn render_debug_log_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_debug_log_popup_open() {
        return;
    }

    const POPUP_TARGET_WIDTH: u16 = 78;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let visible_log_lines = usize::from(area.height).saturating_sub(6).max(1);
    let lines = debug_log_popup_lines(
        state.debug_log_lines(),
        state.debug_channel_visibility(),
        visible_log_lines,
        usize::from(popup_width.saturating_sub(2)),
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Debug logs", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn debug_log_popup_lines(
    entries: Vec<String>,
    channel_visibility: ChannelVisibilityStats,
    visible_log_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let visible_log_lines = visible_log_lines.max(1);
    let mut lines = Vec::new();

    // Header line: visible vs. permission-hidden channels for the active
    // scope. Helps the user diagnose "why is this channel missing" without
    // diving into the logs.
    let visibility_text = format!(
        "Channels: {} visible · {} hidden by permissions",
        channel_visibility.visible, channel_visibility.hidden,
    );
    lines.push(Line::from(Span::styled(
        visibility_text,
        Style::default().fg(ACCENT),
    )));
    lines.push(Line::from(Span::raw(String::new())));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "No errors recorded in this process.",
            Style::default().fg(DIM),
        )));
    } else {
        let wrapped = entries
            .into_iter()
            .flat_map(|entry| wrap_text_lines(&entry, width))
            .collect::<Vec<_>>();
        let start = wrapped.len().saturating_sub(visible_log_lines);
        for entry in wrapped.into_iter().skip(start) {
            lines.push(Line::from(Span::styled(
                entry,
                Style::default().fg(Color::Red),
            )));
        }
    }
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "Showing current-process ERROR logs only · ` / Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn message_action_menu_lines(
    actions: &[MessageActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let label = if action.enabled {
                action.label.to_owned()
            } else {
                format!("{} (unavailable)", action.label)
            };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(label, style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn channel_action_menu_lines(
    actions: &[ChannelActionItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn channel_thread_menu_lines(threads: &[ChannelThreadItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = if threads.is_empty() {
        vec![Line::from(Span::styled(
            "  (no threads)".to_owned(),
            Style::default().fg(DIM),
        ))]
    } else {
        threads
            .iter()
            .enumerate()
            .map(|(index, thread)| {
                let marker = if index == selected { "› " } else { "  " };
                let mut suffix = String::new();
                if thread.archived {
                    suffix.push_str(" [archived]");
                }
                if thread.locked {
                    suffix.push_str(" [locked]");
                }
                let mut style = Style::default();
                if index == selected {
                    style = style
                        .bg(Color::Rgb(40, 45, 90))
                        .add_modifier(Modifier::BOLD);
                }
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(ACCENT)),
                    Span::styled(format!("» {}", thread.label), style),
                    Span::styled(suffix, Style::default().fg(DIM)),
                ])
            })
            .collect()
    };
    lines.push(Line::from(Span::styled(
        "Enter open · Esc back",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn poll_vote_picker_lines(
    answers: &[PollVotePickerItem],
    selected: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let marker = if index == selected { "› " } else { "  " };
            let checkbox = if answer.selected { "[x]" } else { "[ ]" };
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{checkbox} {}", answer.label), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Space toggle · Enter vote · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

pub(super) fn reaction_users_popup_lines(
    reactions: &[ReactionUsersInfo],
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let data_lines = reaction_users_popup_data_lines(reactions);
    let visible_lines = max_visible_lines.min(data_lines.len());
    let scroll = scroll.min(data_lines.len().saturating_sub(visible_lines));
    let has_hidden_before = scroll > 0;
    let has_hidden_after = scroll.saturating_add(visible_lines) < data_lines.len();
    let mut lines: Vec<Line<'static>> = data_lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .map(|line| truncate_line_to_display_width(line, inner_width))
        .collect();
    let hint = match (has_hidden_before, has_hidden_after) {
        (true, true) => "j/k scroll · more above/below · Esc close",
        (true, false) => "j/k scroll · more above · Esc close",
        (false, true) => "j/k scroll · more below · Esc close",
        (false, false) => "Esc close",
    };
    lines.push(truncate_line_to_display_width(
        Line::from(Span::styled(hint, Style::default().fg(DIM))),
        inner_width,
    ));
    lines
}

fn truncate_line_to_display_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 {
        return Line::default();
    }
    let mut remaining = max_width;
    let mut new_spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 1);
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        if span.content.width() <= remaining {
            remaining = remaining.saturating_sub(span.content.width());
            new_spans.push(span);
            continue;
        }
        let truncated = truncate_display_width(&span.content, remaining);
        remaining = remaining.saturating_sub(truncated.width());
        new_spans.push(Span::styled(truncated, span.style));
    }
    if remaining > 0 {
        new_spans.push(Span::styled(" ".repeat(remaining), line.style));
    }
    let mut truncated = Line::from(new_spans);
    truncated.style = line.style;
    truncated.alignment = line.alignment;
    truncated
}

fn reaction_users_popup_data_lines(reactions: &[ReactionUsersInfo]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if reactions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No reactions found",
            Style::default().fg(DIM),
        )));
    }

    for reaction in reactions {
        let count = reaction.users.len();
        let user_label = if count == 1 { "user" } else { "users" };
        lines.push(Line::from(Span::styled(
            format!("{} · {count} {user_label}", reaction.emoji.status_label()),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        if reaction.users.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no users found",
                Style::default().fg(DIM),
            )));
        } else {
            lines.extend(
                reaction
                    .users
                    .iter()
                    .map(|user| Line::from(Span::raw(format!("  {}", user.display_name)))),
            );
        }
    }
    lines
}

pub(super) fn emoji_reaction_picker_lines(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
) -> Vec<Line<'static>> {
    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_items = max_visible_items.max(1).min(reactions.len().max(1));
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);

    let mut lines: Vec<Line<'static>> = reactions[visible_range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, reaction)| {
            let index = visible_range.start + offset;
            let marker = if index == selected { "› " } else { "  " };
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let thumbnail_ready = reaction
                .custom_image_url()
                .is_some_and(|url| thumbnail_urls.iter().any(|ready| ready == &url));
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format_emoji_reaction_item(reaction, thumbnail_ready), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter/Space react · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn render_emoji_reaction_images(
    frame: &mut Frame,
    area: Rect,
    reactions: &[EmojiReactionItem],
    selected: usize,
    visible_items: usize,
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    if area.width <= EMOJI_REACTION_IMAGE_WIDTH || area.height == 0 {
        return;
    }

    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_range =
        super::super::selection::visible_item_range(reactions.len(), selected, visible_items);
    for (offset, reaction) in reactions[visible_range].iter().enumerate() {
        let Some(url) = reaction.custom_image_url() else {
            continue;
        };
        let Some(image) = emoji_images.iter().find(|image| image.url == url) else {
            continue;
        };
        let y = area
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= area.y.saturating_add(area.height.saturating_sub(1)) {
            continue;
        }
        let image_area = Rect::new(
            area.x.saturating_add(2),
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(area.width.saturating_sub(2)),
            1,
        );
        frame.render_widget(RatatuiImage::new(image.protocol), image_area);
    }
}

fn format_emoji_reaction_item(reaction: &EmojiReactionItem, thumbnail_ready: bool) -> String {
    match &reaction.emoji {
        crate::discord::ReactionEmoji::Unicode(emoji) => format!("{} {}", emoji, reaction.label),
        crate::discord::ReactionEmoji::Custom { .. } if thumbnail_ready => format!(
            "{}{}",
            " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))),
            reaction.label
        ),
        crate::discord::ReactionEmoji::Custom { name, .. } => name
            .as_deref()
            .map(|name| format!(":{name}: {}", reaction.label))
            .unwrap_or_else(|| format!(":custom: {}", reaction.label)),
    }
}
