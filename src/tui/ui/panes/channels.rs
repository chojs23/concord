use super::*;

pub(in crate::tui::ui) fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let focused = state.focus() == FocusPane::Channels;
    let filter_query = state.channel_pane_filter_query();
    let block = panel_block("Channels", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Guild name, plus a boost line for boosted guilds when the pane can spare
    // the row. A short pane keeps the name only and still shows every channel.
    let boost_label = selected_channel_boost_label(state);
    let header_area = Rect {
        height: inner.height.min(channel_pane_header_height(state)),
        ..inner
    };
    if header_area.height > 0 {
        let width = header_area.width as usize;
        let server_name = selected_channel_server_label(state);
        let mut lines = vec![Line::from(Span::styled(
            truncate_display_width(&server_name, width),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))];
        if header_area.height >= 2
            && let Some(boost) = &boost_label
        {
            lines.push(Line::from(Span::styled(
                truncate_display_width(boost, width),
                Style::default().fg(DIM),
            )));
        }
        frame.render_widget(Paragraph::new(lines), header_area);
    }

    let channels_area = Rect {
        y: inner.y.saturating_add(header_area.height),
        height: inner.height.saturating_sub(header_area.height),
        ..inner
    };

    let (list_area, filter_area) = split_pane_filter_area(channels_area, filter_query.is_some());

    let channel_entries = state.channel_pane_filtered_entries();
    let channel_entry_count = channel_entries.len();
    let all_channel_entries;
    let populated_channel_entries = if state.channel_pane_filter_query().is_some() {
        all_channel_entries = state.channel_pane_entries();
        all_channel_entries.as_slice()
    } else {
        channel_entries.as_slice()
    };
    let populated_voice_channel_ids: HashSet<_> = populated_channel_entries
        .windows(2)
        .filter_map(|window| match (&window[0], &window[1]) {
            (
                ChannelPaneEntry::Channel { state: channel, .. }
                | ChannelPaneEntry::Thread { state: channel, .. },
                ChannelPaneEntry::VoiceParticipant { .. },
            ) => Some(channel.id),
            _ => None,
        })
        .collect();
    let channel_scroll = state.channel_scroll();
    let selected_channel = (state.focus() == FocusPane::Channels && channel_entry_count > 0)
        .then(|| state.selected_channel_from_entries(&channel_entries));
    let entries: Vec<_> = channel_entries
        .into_iter()
        .skip(channel_scroll)
        .take(list_area.height as usize)
        .collect();
    let scrollbar_width = usize::from(vertical_scrollbar_visible(
        list_area,
        list_area.height as usize,
        channel_entry_count,
    ));
    let max_width = (list_area.width as usize)
        .saturating_sub(selection_marker(false).content.width())
        .saturating_sub(scrollbar_width);
    let horizontal_scroll = state.channel_horizontal_scroll();
    let selected = selected_channel
        .filter(|selected| {
            *selected >= channel_scroll && *selected < channel_scroll + entries.len()
        })
        .map(|selected| selected - channel_scroll);
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
                        let mut label_style =
                            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
                        if dashboard.channel_notification_muted(state.id) {
                            label_style = label_style.add_modifier(Modifier::DIM);
                        }
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(ACCENT)),
                            Span::styled(
                                truncate_display_width_from(
                                    &state.name,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                label_style,
                            ),
                        ]))
                    }
                    ChannelPaneEntry::Channel { state, branch } => {
                        let branch_prefix = branch.prefix();
                        let dm_prefix_span = dm_presence_dot_span(state);
                        let channel_prefix = channel_prefix(&state.kind);
                        let prefix_width = dm_prefix_span
                            .as_ref()
                            .map_or_else(|| channel_prefix.width(), |span| span.content.width());
                        let populated_voice_channel =
                            state.is_voice() && populated_voice_channel_ids.contains(&state.id);
                        let base_style = active_text_style(is_active, Style::default());
                        let is_muted = dashboard.channel_notification_muted(state.id);
                        let unread = dashboard.sidebar_channel_unread(state.id);
                        let (badge, mut name_style) =
                            channel_unread_decoration(unread, base_style, is_active);
                        if state.is_voice() && dashboard.is_joined_voice_channel(state.id) {
                            name_style = name_style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
                        }
                        if is_muted {
                            name_style = name_style.add_modifier(Modifier::DIM);
                        }
                        let badge = if state.guild_id.is_none()
                            && !is_active
                            && unread != ChannelUnreadState::Seen
                        {
                            let message_count = dashboard.channel_unread_message_count(state.id);
                            if message_count > 0 {
                                let count = u32::try_from(message_count).unwrap_or(u32::MAX);
                                Some(notification_count_badge(ChannelUnreadState::Notified(
                                    count,
                                )))
                            } else if unread == ChannelUnreadState::Unread {
                                Some(notification_count_badge(ChannelUnreadState::Notified(1)))
                            } else {
                                badge
                            }
                        } else {
                            badge
                        };
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let request_tag = state.dm_request_tag();
                        // +3 reserves room for the surrounding " [" and "]".
                        let tag_width = request_tag
                            .map(|tag| tag.width().saturating_add(3))
                            .unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix_width)
                            .saturating_sub(badge_width)
                            .saturating_sub(tag_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        if let Some(prefix_span) = dm_prefix_span {
                            spans.push(prefix_span);
                        } else if populated_voice_channel {
                            spans.push(Span::styled("🔊", Style::default().fg(Color::Cyan)));
                            spans.push(Span::styled(" ", Style::default().fg(DIM)));
                        } else {
                            spans.push(Span::styled(channel_prefix, Style::default().fg(DIM)));
                        }
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                &state.name,
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        if let Some(tag) = request_tag {
                            spans.push(Span::styled(
                                format!(" [{tag}]"),
                                Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                            ));
                        }
                        ListItem::new(Line::from(spans))
                    }
                    ChannelPaneEntry::Thread {
                        state,
                        parent_branch,
                        branch,
                    } => {
                        let parent_prefix = parent_branch.participant_prefix();
                        let branch_prefix = branch.prefix();
                        let thread_prefix = if dashboard.is_forum_post_thread(state.id) {
                            "💬 "
                        } else {
                            "🧵 "
                        };
                        let base_style = active_text_style(is_active, Style::default());
                        let is_muted = dashboard.channel_notification_muted(state.id);
                        let unread = dashboard.sidebar_channel_unread(state.id);
                        let (badge, mut name_style) =
                            channel_unread_decoration(unread, base_style, is_active);
                        if is_muted {
                            name_style = name_style.add_modifier(Modifier::DIM);
                        }
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(parent_prefix.width())
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(thread_prefix.width())
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(parent_prefix, Style::default().fg(DIM)),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(thread_prefix, Style::default().fg(DIM)));
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
                    ChannelPaneEntry::VoiceParticipant {
                        participant,
                        parent_branch,
                        ..
                    } => {
                        let branch_prefix = parent_branch.participant_prefix();
                        let label_style = if participant.speaking {
                            Style::default().fg(Color::Green).bold()
                        } else {
                            Style::default().fg(DIM)
                        };
                        let prefix = "  • ";
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(false),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                            Span::styled(prefix, Style::default().fg(DIM)),
                            Span::styled(
                                voice_participant_label(
                                    participant,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                label_style,
                            ),
                        ]))
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items).highlight_style(highlight_style());
    frame.render_widget(list, list_area);

    render_pane_filter_bar_with_cursor(
        frame,
        filter_area,
        filter_query,
        state.channel_pane_filter_cursor(),
        focused,
    );

    render_vertical_scrollbar(
        frame,
        list_area,
        state.channel_scroll(),
        list_area.height as usize,
        channel_entry_count,
    );
}

fn selected_channel_server_label(state: &DashboardState) -> String {
    state
        .selected_guild_id()
        .and_then(|guild_id| state.guild_name(guild_id))
        .unwrap_or("Direct Messages")
        .to_owned()
}

fn selected_guild_is_boosted(state: &DashboardState) -> bool {
    matches!(
        state.selected_guild_boost(),
        Some((tier, count)) if tier.level() != 0 || count != 0
    )
}

/// Header rows the channel pane reserves: the guild name, plus one for the boost
/// line. Single source shared by the renderer, the scroll viewport, and
/// hit-testing so they cannot drift and clip the last channel row.
pub(in crate::tui::ui) fn channel_pane_header_height(state: &DashboardState) -> u16 {
    if selected_guild_is_boosted(state) {
        2
    } else {
        1
    }
}

fn selected_channel_boost_label(state: &DashboardState) -> Option<String> {
    if !selected_guild_is_boosted(state) {
        return None;
    }
    let (tier, count) = state.selected_guild_boost()?;
    let boosts = if count == 1 { "boost" } else { "boosts" };
    Some(format!("⚡ Level {} · {count} {boosts}", tier.level()))
}

fn voice_participant_label(
    participant: &crate::discord::VoiceParticipantState,
    horizontal_scroll: usize,
    max_width: usize,
) -> String {
    let mut indicators = String::new();
    if participant.self_stream {
        indicators.push_str(" 🔴");
    }
    if participant.mute || participant.self_mute {
        indicators.push_str(" 🔇");
    }
    if participant.deaf || participant.self_deaf {
        indicators.push_str(" 🎧");
    }

    let indicator_width = indicators.width();
    if indicator_width == 0 {
        return truncate_display_width_from(
            &participant.display_name,
            horizontal_scroll,
            max_width,
        );
    }
    if max_width <= indicator_width {
        return truncate_display_width(&indicators, max_width);
    }

    format!(
        "{}{}",
        truncate_display_width_from(
            &participant.display_name,
            horizontal_scroll,
            max_width.saturating_sub(indicator_width),
        ),
        indicators
    )
}
