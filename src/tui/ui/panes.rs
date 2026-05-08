use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::discord::{
    ActivityInfo, ActivityKind, ChannelUnreadState, MessageState, PresenceStatus,
};

use super::super::{
    format::{truncate_display_width, truncate_display_width_from, truncate_text},
    message_format::format_attachment_summary,
    state::{
        ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MAX_MENTION_PICKER_VISIBLE,
        MemberEntry, MemberGroup, MentionPickerEntry, discord_color, folder_color, presence_color,
        presence_marker,
    },
};
use super::{
    active_text_style, channel_prefix, channel_unread_decoration, dm_presence_dot_span,
    highlight_style,
    layout::{composer_inner_width, panel_scrollbar_area},
    panel_block, panel_block_owned, panel_content_height, render_vertical_scrollbar,
    selection_marker, styled_list_item,
    types::{ACCENT, DIM, MessageAreas},
};

pub(super) fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let entries = state.visible_guild_pane_entries();
    let max_width = area.width.saturating_sub(6) as usize;
    let horizontal_scroll = state.guild_horizontal_scroll();
    let selected = state.focused_guild_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = state.is_active_guild_entry(entry);
            styled_list_item(
                match entry {
                    GuildPaneEntry::DirectMessages => {
                        let base_style = active_text_style(
                            is_active,
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        );
                        let unread_count = state.direct_message_unread_count();
                        let badge =
                            (unread_count > 0).then(|| notification_count_badge(unread_count));
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width.saturating_sub(badge_width);
                        let mut spans = vec![selection_marker(is_selected)];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                entry.label(),
                                horizontal_scroll,
                                label_width,
                            ),
                            base_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                    GuildPaneEntry::FolderHeader { folder, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let icon = if *collapsed { "📁" } else { "📂" };
                        let color = folder_color(folder.color);
                        let label = folder.name.as_deref().unwrap_or_default();
                        let title = if label.is_empty() {
                            icon.to_owned()
                        } else {
                            format!("{icon} {label}")
                        };
                        let label_width = max_width.saturating_sub(arrow.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(color)),
                            Span::styled(
                                truncate_display_width_from(&title, horizontal_scroll, label_width),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    GuildPaneEntry::Guild {
                        state: guild,
                        branch,
                    } => {
                        let prefix = branch.prefix();
                        let base_style = active_text_style(is_active, Style::default());
                        let unread = dashboard.guild_unread(guild.id);
                        let (badge, name_style) = if is_active {
                            let (badge, _) = channel_unread_decoration(unread, base_style, false);
                            (badge, base_style)
                        } else if unread == ChannelUnreadState::Seen {
                            (None, base_style)
                        } else {
                            channel_unread_decoration(unread, base_style, false)
                        };
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(prefix.width())
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                guild.name.as_str(),
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("Servers", state.focus() == FocusPane::Guilds))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.guild_scroll(),
        panel_content_height(area, "Servers"),
        state.guild_pane_entries().len(),
    );
}

fn notification_count_badge(count: usize) -> Span<'static> {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    let (badge, _) = channel_unread_decoration(
        ChannelUnreadState::Mentioned(count),
        Style::default(),
        false,
    );
    badge.expect("mentioned unread state always renders a badge")
}

pub(super) fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let entries = state.visible_channel_pane_entries();
    let max_width = area.width.saturating_sub(8) as usize;
    let horizontal_scroll = state.channel_horizontal_scroll();
    let selected = state.focused_channel_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = dashboard.is_active_channel_entry(entry);
            styled_list_item(
                match entry {
                    ChannelPaneEntry::CategoryHeader { state, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let label_width = max_width.saturating_sub(arrow.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(ACCENT)),
                            Span::styled(
                                truncate_display_width_from(
                                    &state.name,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    ChannelPaneEntry::Channel { state, branch } => {
                        let branch_prefix = branch.prefix();
                        let prefix_span = dm_presence_dot_span(state).unwrap_or_else(|| {
                            Span::styled(channel_prefix(&state.kind), Style::default().fg(DIM))
                        });
                        let prefix_width = prefix_span.content.width();
                        let base_style = active_text_style(is_active, Style::default());
                        let unread = dashboard.channel_unread(state.id);
                        let (badge, name_style) =
                            channel_unread_decoration(unread, base_style, is_active);
                        let badge = if state.guild_id.is_none()
                            && !is_active
                            && unread != ChannelUnreadState::Seen
                        {
                            let message_count = dashboard.channel_unread_message_count(state.id);
                            if message_count > 0 {
                                Some(notification_count_badge(message_count))
                            } else if unread == ChannelUnreadState::Unread {
                                Some(notification_count_badge(1))
                            } else {
                                badge
                            }
                        } else {
                            badge
                        };
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix_width)
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(prefix_span);
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                &state.name,
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        ListItem::new(Line::from(spans))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items)
        .block(panel_block(
            "Channels",
            state.focus() == FocusPane::Channels,
        ))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.channel_scroll(),
        panel_content_height(area, "Channels"),
        state.channel_pane_entries().len(),
    );
}

pub(super) fn render_composer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let inner_width = composer_inner_width(area.width);
    let prompt = composer_lines(state, inner_width);
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
                    .title(" Message Input ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(Color::White).bold()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub(super) fn render_composer_mention_picker(
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
    let Some(area) = mention_picker_area(message_areas, candidates.len()) else {
        return;
    };
    frame.render_widget(Clear, area);
    let inner_width = area.width.saturating_sub(2) as usize;
    let lines = mention_picker_lines(&candidates, state.composer_mention_selected(), inner_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" mention ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Picks a rectangle directly above the composer for the picker. Returns
/// `None` when there isn't enough room (very short terminal) so the caller
/// can silently skip drawing.
fn mention_picker_area(message_areas: MessageAreas, candidate_count: usize) -> Option<Rect> {
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
    let width = composer.width.clamp(20, 48).min(messages.width);
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
            let label = format!("{}{bot_marker}{username_hint}", entry.display_name);
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = Style::default().fg(presence_color(entry.status));
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(cursor, Style::default().fg(ACCENT)),
                Span::styled(presence_marker(entry.status).to_string(), row_style),
                Span::styled(" ", row_style),
                Span::styled(label, row_style),
            ])
        })
        .collect()
}

pub(super) fn composer_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
    if state.is_composing() {
        let input = Line::from(format!("> {}", state.composer_input()));
        if let Some(message) = state.reply_target_message_state() {
            return vec![
                Line::from(Span::styled(
                    reply_target_hint(message, state, width),
                    Style::default().fg(DIM),
                )),
                input,
            ];
        }
        return vec![input];
    }

    vec![Line::from(composer_text(state, width))]
}

pub(super) fn composer_text(state: &DashboardState, width: u16) -> String {
    if state.is_composing() {
        let input = format!("> {}", state.composer_input());
        if let Some(message) = state.reply_target_message_state() {
            return format!("{}\n{input}", reply_target_hint(message, state, width));
        }
        return input;
    }

    if let Some(channel) = state.selected_channel_state() {
        let label = match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        };
        // Tell the user up-front if the keymap won't open the composer here,
        // so they don't repeatedly press `i` and wonder why nothing happens.
        if !state.can_send_in_selected_channel() {
            return format!("read-only · cannot send messages in {label}");
        }
        // SEND is allowed but ATTACH isn't — flag it so a future attachment
        // picker has a coherent UX, and the user knows uploads will be
        // refused before they try.
        if !state.can_attach_in_selected_channel() {
            return format!("press i to write in {label} (attachments disabled)");
        }
        return format!("press i to write in {label}");
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

pub(super) fn render_members(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let groups = state.members_grouped();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let max_name_width = (area.width as usize).saturating_sub(6).max(8);
    let selected_line = state
        .focused_member_selection_line()
        .map(|line| line + state.member_scroll());
    let focused = state.focus() == FocusPane::Members;
    let mut line_index = 0usize;

    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "No members loaded yet.",
            Style::default().fg(DIM),
        )));
    }

    for group in &groups {
        if !lines.is_empty() {
            lines.push(Line::from(""));
            line_index += 1;
        }
        lines.push(member_group_header(group));
        line_index += 1;
        for member in &group.entries {
            let member = *member;
            let is_selected = focused && selected_line == Some(line_index);
            let marker_style = Style::default().fg(presence_color(member.status()));
            let name_style =
                member_name_style(member, state.member_role_color(member), is_selected);

            let display =
                member_display_label(member, state.member_horizontal_scroll(), max_name_width);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", presence_marker(member.status())),
                    marker_style,
                ),
                Span::styled(display, name_style),
            ]));
            line_index += 1;

            // Three-space indent + max_name_width matches the name row's
            // envelope so the activity row never overflows or wraps.
            if !matches!(
                member.status(),
                PresenceStatus::Offline | PresenceStatus::Unknown
            ) {
                let activities = state.user_activities(member.user_id());
                if let Some(summary) = primary_activity_summary(activities) {
                    let summary = truncate_display_width_from(
                        &summary,
                        state.member_horizontal_scroll(),
                        max_name_width,
                    );
                    lines.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(summary, Style::default().fg(DIM)),
                    ]));
                    line_index += 1;
                }
            }
        }
    }

    let lines: Vec<_> = lines
        .into_iter()
        .skip(state.member_scroll())
        .take(state.member_content_height())
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block_owned(state.member_panel_title(), focused))
            .wrap(Wrap { trim: false }),
        area,
    );
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.member_scroll(),
        state.member_content_height(),
        state.member_line_count(),
    );
}

fn member_group_header(group: &MemberGroup<'_>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            group.label.clone(),
            Style::default()
                .fg(discord_color(group.color, DIM))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", group.entries.len()),
            Style::default().fg(DIM),
        ),
    ])
}

pub(super) fn member_name_style(
    member: MemberEntry<'_>,
    role_color: Option<u32>,
    is_selected: bool,
) -> Style {
    let mut style = Style::default().fg(discord_color(role_color, Color::White));
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
            .bg(Color::Rgb(24, 54, 65))
            .add_modifier(Modifier::BOLD);
    }
    style
}

pub(super) fn member_display_label(
    member: MemberEntry<'_>,
    horizontal_scroll: usize,
    max_width: usize,
) -> String {
    let display_name = member.display_name();
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
pub(super) fn primary_activity_summary(activities: &[ActivityInfo]) -> Option<String> {
    activities
        .iter()
        .min_by_key(|activity| activity_priority(activity.kind))
        .map(format_activity_summary)
}

fn activity_priority(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Custom => 0,
        ActivityKind::Streaming => 1,
        ActivityKind::Listening => 2,
        ActivityKind::Playing => 3,
        ActivityKind::Watching => 4,
        ActivityKind::Competing => 5,
        ActivityKind::Unknown => 6,
    }
}

fn format_activity_summary(activity: &ActivityInfo) -> String {
    match activity.kind {
        ActivityKind::Custom => {
            let emoji = activity
                .emoji
                .as_ref()
                .map(|emoji| {
                    if emoji.id.is_some() {
                        format!(":{}:", emoji.name)
                    } else {
                        emoji.name.clone()
                    }
                })
                .unwrap_or_default();
            let body = activity.state.clone().unwrap_or_default();
            match (emoji.is_empty(), body.is_empty()) {
                (true, true) => activity.name.clone(),
                (false, true) => emoji,
                (true, false) => body,
                (false, false) => format!("{emoji} {body}"),
            }
        }
        ActivityKind::Playing => format!("Playing {}", activity.name),
        ActivityKind::Streaming => format!("Streaming {}", activity.name),
        ActivityKind::Listening => match (activity.details.as_deref(), activity.state.as_deref()) {
            (Some(track), Some(artist)) => {
                format!("Listening to {} — {} by {}", activity.name, track, artist)
            }
            (Some(track), None) => format!("Listening to {} — {}", activity.name, track),
            _ => format!("Listening to {}", activity.name),
        },
        ActivityKind::Watching => format!("Watching {}", activity.name),
        ActivityKind::Competing => format!("Competing in {}", activity.name),
        ActivityKind::Unknown => activity.name.clone(),
    }
}

pub(super) fn render_header(frame: &mut Frame, area: Rect) {
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default().fg(Color::Cyan).bold(),
        )))
        .alignment(Alignment::Left),
        area,
    );
}

pub(super) fn render_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let user = footer_user_label(state);
    let mut spans = vec![
        Span::styled(
            format!(" {user} "),
            Style::default().fg(Color::Green).bold(),
        ),
        Span::styled(footer_hint(state), Style::default().fg(DIM)),
    ];
    if let Some(status) = state.last_status() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            truncate_text(status, 72),
            Style::default().fg(Color::Green),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
}

pub(super) fn footer_user_label(state: &DashboardState) -> &str {
    state.current_user().unwrap_or("Loading Concord...")
}

pub(super) fn footer_hint(state: &DashboardState) -> &'static str {
    if state.is_debug_log_popup_open() {
        "`/esc close debug logs"
    } else if state.is_reaction_users_popup_open() {
        "esc close reacted users"
    } else if state.is_poll_vote_picker_open() {
        "j/k choose answer | space toggle | enter vote | esc close"
    } else if state.is_emoji_reaction_picker_open() {
        "j/k choose emoji | enter/space react | esc close"
    } else if state.is_image_viewer_action_menu_open() {
        "enter/space download image | esc close menu"
    } else if state.is_image_viewer_open() {
        "h/← previous image | l/→ next image | enter/space actions | esc close"
    } else if state.is_user_profile_popup_open() {
        "j/k pick mutual server | enter open server | esc close"
    } else if state.is_message_action_menu_open()
        || state.is_guild_action_menu_open()
        || state.is_member_action_menu_open()
    {
        "j/k choose action | enter select | esc close | q quit"
    } else if state.is_channel_action_menu_open() {
        if state.is_channel_action_threads_phase() {
            "j/k choose thread | enter open | esc/← back | q quit"
        } else {
            "j/k choose action | enter select | esc close | q quit"
        }
    } else if state.focus() == FocusPane::Members {
        "tab/1-4 focus | j/k move | H/L scroll name | enter/space profile | a actions | i write | q quit"
    } else if state.focus() == FocusPane::Channels {
        "tab/1-4 focus | j/k move | H/L scroll name | enter/space open | h/← close | l/→ open | a actions | ` logs | i write | q quit"
    } else if state.focus() == FocusPane::Guilds {
        "tab/1-4 focus | j/k move | J/K scroll | H/L scroll name | enter/space action/tree | h/← close | l/→ open | ` logs | i write | esc cancel | q quit"
    } else {
        "tab/1-4 focus | j/k move | J/K scroll | enter/space actions | ` logs | i write | esc cancel | q quit"
    }
}
