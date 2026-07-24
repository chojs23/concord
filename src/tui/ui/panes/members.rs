use super::*;
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};

const MEMBER_ACTIVITY_LEADING_WIDTH: usize = 4;

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
    // (absolute_line_index, relative_column, cdn_url) for loaded activity emoji images.
    let mut emoji_line_urls: Vec<(usize, usize, String)> = Vec::new();
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
            theme::current().style(theme::HighlightGroup::Loading),
        )));
    } else if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "No members loaded yet.",
            theme::current().style(theme::HighlightGroup::Placeholder),
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
                let marker_style = selected_presence_style(is_selected, member.status());
                let name_style =
                    member_name_style(member, state.member_role_color(member), is_selected);

                let display_name = state.member_display_name(member);
                let display = member_display_label(
                    member,
                    &display_name,
                    state.member_horizontal_scroll(),
                    max_name_width,
                );
                let mut spans = vec![
                    selection_marker(is_selected),
                    Span::styled(
                        format!("{} ", presence_marker(member.status())),
                        marker_style,
                    ),
                ];
                if member.is_bot()
                    && let Some(name) = display.strip_suffix(" [bot]")
                {
                    spans.push(Span::styled(name.to_owned(), name_style));
                    spans.push(Span::styled(
                        " [bot]",
                        selected_text_style(
                            is_selected,
                            theme::current().style(theme::HighlightGroup::Emphasis),
                        ),
                    ));
                } else {
                    spans.push(Span::styled(display, name_style));
                }
                let mut line = Line::from(spans);
                let padding = content_width.saturating_sub(line.width());
                if padding > 0 {
                    line.spans.push(Span::raw(" ".repeat(padding)));
                }
                lines.push(selected_row_line(line, is_selected));
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
                        let activity_line = compact_activity_line(
                            render,
                            MEMBER_ACTIVITY_LEADING_WIDTH,
                            MEMBER_ACTIVITY_LEADING_WIDTH.saturating_add(max_name_width),
                            h_scroll,
                        );
                        if let Some(image) = activity_line.image {
                            emoji_line_urls.push((line_index, image.column, image.url));
                        }
                        lines.push(activity_line.line);
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
            emoji_line_urls
                .iter()
                .map(|(line_idx, column, url)| EmojiSlot {
                    row_in_list: *line_idx as isize - scroll as isize,
                    col: content_area.x as isize + *column as isize,
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
    let style = match group.color {
        Some(color) if color != 0 => apply_discord_foreground(
            theme::current().apply(
                theme::HighlightGroup::MemberGroupHeading,
                normal_text_style(),
            ),
            Some(color),
        ),
        _ => theme::current().apply(
            theme::HighlightGroup::Muted,
            theme::current().style(theme::HighlightGroup::MemberGroupHeading),
        ),
    };
    Line::from(vec![
        Span::styled(label, style),
        Span::styled(count_suffix, style),
    ])
}

pub(in crate::tui::ui) fn member_name_style(
    member: MemberEntry<'_>,
    role_color: Option<u32>,
    is_selected: bool,
) -> Style {
    let mut style = apply_discord_foreground(normal_text_style(), role_color);
    if matches!(
        member.status(),
        PresenceStatus::Offline | PresenceStatus::Unknown
    ) {
        style = theme::current().apply(theme::HighlightGroup::Muted, style);
    }
    if member.is_bot() {
        style = theme::current().apply(theme::HighlightGroup::Emphasis, style);
    }
    selected_discord_text_style(is_selected, style, role_color)
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

/// Compact ordering prefers live game signals before a custom status:
/// Streaming > Playing > Listening > Watching > Competing > Custom > Unknown.
pub(in crate::tui::ui) fn primary_activity_summary(
    activities: &[ActivityInfo],
    emoji_images: &[EmojiImage<'_>],
) -> Option<ActivityRender> {
    primary_compact_activity(activities)
        .map(|activity| build_activity_render(activity, emoji_images, true))
}
