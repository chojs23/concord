use super::members::primary_activity_summary;
use super::*;
use crate::tui::ui::emoji_overlay::{EmojiSlot, overlay_emoji_slots};

pub(in crate::tui::ui) fn render_channels(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: &[EmojiImage<'_>],
) {
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
            theme::current().style(theme::HighlightGroup::Heading),
        ))];
        if header_area.height >= 2
            && let Some(boost) = &boost_label
        {
            lines.push(Line::from(Span::styled(
                truncate_display_width(boost, width),
                theme::current().style(theme::HighlightGroup::Description),
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
    let channel_line_count = state.channel_entry_line_count(&channel_entries);
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
    let content_height = state.channel_content_height();
    let selected_line = state.focused_channel_selection_line(&channel_entries);
    let entries: Vec<_> = channel_entries
        .iter()
        .flat_map(|entry| {
            std::iter::once((entry, false)).chain(
                state
                    .channel_entry_has_activity_row(entry)
                    .then_some((entry, true)),
            )
        })
        .enumerate()
        .skip(channel_scroll)
        .take(content_height)
        .collect();
    let scrollbar_width = usize::from(vertical_scrollbar_visible(
        list_area,
        list_area.height as usize,
        channel_line_count,
    ));
    let max_width = (list_area.width as usize)
        .saturating_sub(selection_marker(false).content.width())
        .saturating_sub(scrollbar_width);
    let horizontal_scroll = state.channel_horizontal_scroll();
    let mut emoji_line_urls: Vec<(usize, String)> = Vec::new();
    let items: Vec<ListItem> = entries
        .iter()
        .filter_map(|(line_index, (entry, is_activity_row))| {
            if *is_activity_row {
                let ChannelPaneEntry::Channel {
                    state: channel,
                    branch,
                    ..
                } = entry
                else {
                    return None;
                };
                let recipient = channel.recipients.first()?;
                let activities = state.user_activities(recipient.user_id);
                let render = primary_activity_summary(activities, emoji_images)?;
                let dm_prefix_width = dm_presence_dot_span(channel).map_or_else(
                    || channel_prefix(&channel.kind).width(),
                    |span| span.content.width(),
                );
                let leading_width = branch.prefix().width() + dm_prefix_width + 2;
                let body_width = max_width.saturating_sub(leading_width);
                let body = truncate_display_width_from(
                    &render.body,
                    horizontal_scroll,
                    body_width.saturating_sub(
                        usize::from(matches!(render.leading, ActivityLeading::Icon(_))) * 2,
                    ),
                );
                let leading = " ".repeat(leading_width);
                let line = match render.leading {
                    ActivityLeading::Image(url) => {
                        emoji_line_urls.push((*line_index, url));
                        Line::from(vec![
                            Span::raw(leading),
                            Span::styled(
                                body,
                                theme::current().style(theme::HighlightGroup::Activity),
                            ),
                        ])
                    }
                    ActivityLeading::Icon(icon) => Line::from(vec![
                        Span::raw(leading),
                        Span::styled(
                            icon.to_string(),
                            theme::current().style(theme::HighlightGroup::PresenceOnline),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            body,
                            theme::current().style(theme::HighlightGroup::Activity),
                        ),
                    ]),
                    ActivityLeading::None => Line::from(vec![
                        Span::raw(leading),
                        Span::styled(
                            body,
                            theme::current().style(theme::HighlightGroup::Activity),
                        ),
                    ]),
                };
                return Some(ListItem::new(line));
            }

            let is_selected = selected_line == Some(*line_index);
            let is_active = dashboard.is_active_channel_entry(entry);
            Some(styled_list_item(
                match entry {
                    ChannelPaneEntry::CategoryHeader { state, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let label_width = max_width.saturating_sub(arrow.width());
                        let mut label_style =
                            theme::current().style(theme::HighlightGroup::CategoryHeading);
                        if dashboard.channel_notification_muted(state.id) {
                            label_style =
                                theme::current().apply(theme::HighlightGroup::Muted, label_style);
                        }
                        label_style = selected_text_style(is_selected, label_style);
                        Line::from(vec![
                            selection_marker(is_selected),
                            Span::raw(arrow),
                            Span::styled(
                                truncate_display_width_from(
                                    &state.name,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                label_style,
                            ),
                        ])
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
                            name_style = theme::current()
                                .apply(theme::HighlightGroup::JoinedVoiceChannel, name_style);
                        }
                        if is_muted {
                            name_style =
                                theme::current().apply(theme::HighlightGroup::Muted, name_style);
                        }
                        name_style = selected_text_style(is_selected, name_style);
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
                        let badge = badge.map(|badge| selected_text_span(is_selected, badge));
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
                            Span::styled(
                                branch_prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        if let Some(prefix_span) = dm_prefix_span {
                            spans.push(prefix_span);
                        } else if populated_voice_channel {
                            spans.push(Span::styled(
                                "🔊",
                                theme::current().style(theme::HighlightGroup::ChannelTypeMarker),
                            ));
                            spans.push(Span::raw(" "));
                        } else {
                            spans.push(Span::styled(
                                channel_prefix,
                                theme::current().style(theme::HighlightGroup::ChannelTypeMarker),
                            ));
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
                                theme::current().apply(
                                    theme::HighlightGroup::Emphasis,
                                    theme::current().style(theme::HighlightGroup::Description),
                                ),
                            ));
                        }
                        Line::from(spans)
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
                            name_style =
                                theme::current().apply(theme::HighlightGroup::Muted, name_style);
                        }
                        name_style = selected_text_style(is_selected, name_style);
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(parent_prefix.width())
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(thread_prefix.width())
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(
                                parent_prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
                            Span::styled(
                                branch_prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
                        ];
                        if let Some(badge) = badge {
                            spans.push(badge);
                        }
                        spans.push(Span::styled(
                            thread_prefix,
                            theme::current().style(theme::HighlightGroup::ChannelTypeMarker),
                        ));
                        spans.push(Span::styled(
                            truncate_display_width_from(
                                &state.name,
                                horizontal_scroll,
                                label_width,
                            ),
                            name_style,
                        ));
                        Line::from(spans)
                    }
                    ChannelPaneEntry::VoiceParticipant {
                        participant,
                        parent_branch,
                        ..
                    } => {
                        let branch_prefix = parent_branch.participant_prefix();
                        let label_style = if participant.speaking {
                            theme::current().style(theme::HighlightGroup::VoiceSpeaking)
                        } else {
                            theme::current().style(theme::HighlightGroup::Muted)
                        };
                        let prefix = "  • ";
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(prefix.width());
                        Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(
                                branch_prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
                            Span::styled(
                                prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
                            Span::styled(
                                voice_participant_label(
                                    participant,
                                    horizontal_scroll,
                                    label_width,
                                ),
                                selected_text_style(is_selected, label_style),
                            ),
                        ])
                    }
                },
                is_selected,
            ))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, list_area);

    if state.show_custom_emoji() {
        overlay_emoji_slots(
            frame,
            list_area,
            emoji_images,
            &[],
            emoji_line_urls.iter().map(|(line_index, url)| EmojiSlot {
                row_in_list: *line_index as isize - channel_scroll as isize,
                col: list_area.x as isize + 2,
                max_width: u16::MAX,
                url: url.clone(),
            }),
        );
    }

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
        channel_line_count,
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
