use super::*;
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};

pub(in crate::tui::ui) fn render_members(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
    let loading_members = state.is_member_list_loading();
    let groups = if loading_members {
        Vec::new()
    } else {
        state.members_grouped()
    };
    let scroll = state.member_scroll();
    let content_height = state.member_content_height();
    let visible_end = scroll.saturating_add(content_height);
    let mut lines: Vec<Line<'static>> = Vec::new();
    // (absolute_line_index, cdn_url) for activity rows that have a loaded emoji image.
    let mut emoji_line_urls: Vec<(usize, String)> = Vec::new();
    let content_width = (area.width as usize).saturating_sub(2);
    let max_name_width = (area.width as usize).saturating_sub(7).max(8);
    let selected_line = state
        .focused_member_selection_line_in_groups(&groups)
        .map(|line| line + state.member_scroll());
    let focused = state.focus() == FocusPane::Members;
    let mut line_index = 0usize;

    if loading_members {
        lines.push(Line::from(Span::styled(
            "Loading...",
            Style::default().fg(theme::current().dim),
        )));
    } else if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "No members loaded yet.",
            Style::default().fg(theme::current().dim),
        )));
    }

    for group in &groups {
        if line_index > 0 {
            if line_index >= scroll && line_index < visible_end {
                lines.push(Line::from(""));
            }
            line_index += 1;
        }
        if line_index >= scroll && line_index < visible_end {
            lines.push(member_group_header(group, content_width));
        }
        line_index += 1;
        for member in &group.entries {
            let member = *member;
            if line_index >= scroll && line_index < visible_end {
                let is_selected = focused && selected_line == Some(line_index);
                let marker_style = Style::default().fg(presence_color(member.status()));
                let name_style =
                    member_name_style(member, state.member_role_color(member), is_selected);

                let display_name = state.member_display_name(member);
                let display = member_display_label(
                    member,
                    &display_name,
                    state.member_horizontal_scroll(),
                    max_name_width,
                );
                lines.push(Line::from(vec![
                    selection_marker(is_selected),
                    Span::styled(
                        format!("{} ", presence_marker(member.status())),
                        marker_style,
                    ),
                    Span::styled(display, name_style),
                ]));
            }
            line_index += 1;

            if !matches!(
                member.status(),
                PresenceStatus::Offline | PresenceStatus::Unknown
            ) {
                let activities = state.user_activities(member.user_id());
                if !activities.is_empty() {
                    let h_scroll = state.member_horizontal_scroll();
                    if line_index >= scroll
                        && line_index < visible_end
                        && let Some(render) = primary_activity_summary(activities, emoji_images)
                    {
                        let line = match render.leading {
                            ActivityLeading::Image(url) => {
                                let body = truncate_display_width_from(
                                    &render.body,
                                    h_scroll,
                                    max_name_width.saturating_sub(3),
                                );
                                emoji_line_urls.push((line_index, url));
                                Line::from(vec![
                                    Span::raw("      "),
                                    Span::styled(body, Style::default().fg(theme::current().dim)),
                                ])
                            }
                            ActivityLeading::Icon(icon) => {
                                let body = truncate_display_width_from(
                                    &render.body,
                                    h_scroll,
                                    max_name_width.saturating_sub(2),
                                );
                                Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(
                                        icon.to_string(),
                                        Style::default().fg(theme::current().success),
                                    ),
                                    Span::raw(" "),
                                    Span::styled(body, Style::default().fg(theme::current().dim)),
                                ])
                            }
                            ActivityLeading::None => {
                                let body = truncate_display_width_from(
                                    &render.body,
                                    h_scroll,
                                    max_name_width,
                                );
                                Line::from(vec![
                                    Span::raw("    "),
                                    Span::styled(body, Style::default().fg(theme::current().dim)),
                                ])
                            }
                        };
                        lines.push(line);
                    }
                    line_index += 1;
                }
            }
        }
    }

    let block = panel_block_line(state.member_panel_title(), focused);
    let content_area = block.inner(area);
    frame.render_widget(Paragraph::new(lines).block(block), area);

    if state.show_custom_emoji() {
        let list = Rect {
            height: content_height as u16,
            ..content_area
        };
        overlay_emoji_slots(
            frame,
            list,
            emoji_images,
            &[],
            emoji_line_urls.iter().map(|(line_idx, url)| EmojiSlot {
                row_in_list: *line_idx as isize - scroll as isize,
                col: content_area.x as isize + 4,
                max_width: u16::MAX,
                url: url.clone(),
            }),
        );
    }

    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        scroll,
        content_height,
        state.member_line_count_in_groups(&groups),
    );
}

fn member_group_header(group: &MemberGroup<'_>, content_width: usize) -> Line<'static> {
    let count_suffix = format!(" - {}", group.entries.len());
    let label_max = content_width.saturating_sub(count_suffix.width());
    let label = truncate_display_width(&sanitize_for_display_width(&group.label), label_max);
    Line::from(vec![
        Span::styled(
            label,
            Style::default()
                .fg(discord_color(group.color, theme::current().dim))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(count_suffix, Style::default().fg(theme::current().dim)),
    ])
}

pub(in crate::tui::ui) fn member_name_style(
    member: MemberEntry<'_>,
    role_color: Option<u32>,
    is_selected: bool,
) -> Style {
    let mut style = Style::default().fg(discord_color(role_color, theme::current().text));
    if matches!(
        member.status(),
        PresenceStatus::Offline | PresenceStatus::Unknown
    ) {
        style = style.add_modifier(Modifier::DIM);
    }
    if member.is_bot() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if is_selected {
        style = style
            .bg(theme::current().background)
            .add_modifier(Modifier::BOLD);
    }
    style
}

pub(in crate::tui::ui) fn member_display_label(
    member: MemberEntry<'_>,
    display_name: &str,
    horizontal_scroll: usize,
    max_width: usize,
) -> String {
    let display_name = sanitize_for_display_width(display_name);
    if !member.is_bot() {
        return truncate_display_width_from(&display_name, horizontal_scroll, max_width);
    }

    const BOT_SUFFIX: &str = " [bot]";
    let suffix_width = BOT_SUFFIX.width();
    if max_width <= suffix_width {
        return truncate_display_width_from(
            &format!("{}{}", display_name, BOT_SUFFIX),
            horizontal_scroll,
            max_width,
        );
    }

    format!(
        "{}{}",
        truncate_display_width_from(
            &display_name,
            horizontal_scroll,
            max_width.saturating_sub(suffix_width),
        ),
        BOT_SUFFIX
    )
}

/// Priority: Custom > Streaming > Listening > Playing > Watching > Competing > Unknown.
/// Returns `(display_text, Option<cdn_url>)`. When the cdn_url is `Some`, the
/// text contains a 2-space placeholder at the start for the image overlay.
pub(in crate::tui::ui) fn primary_activity_summary(
    activities: &[ActivityInfo],
    emoji_images: &[EmojiImage<'_>],
) -> Option<ActivityRender> {
    let mut sorted: Vec<&ActivityInfo> = activities.iter().collect();
    sorted.sort_by_key(|a| activity_priority(a.kind));
    let mut image_only_fallback: Option<ActivityRender> = None;
    for activity in sorted {
        let render = build_activity_render(activity, emoji_images, true);
        if !render.body.trim().is_empty() {
            return Some(render);
        }
        if matches!(render.leading, ActivityLeading::Image(_)) && image_only_fallback.is_none() {
            image_only_fallback = Some(render);
        }
    }
    image_only_fallback
}

/// Member-list ordering. Intentionally differs from
/// `popups::activity_priority`: see [`primary_activity_summary`].
fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Streaming => 0,
        ActivityKind::Playing => 1,
        ActivityKind::Listening => 2,
        ActivityKind::Watching => 3,
        ActivityKind::Competing => 4,
        ActivityKind::Custom => 5,
        ActivityKind::Unknown => 6,
    }
}
