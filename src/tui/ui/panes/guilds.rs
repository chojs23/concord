use super::*;

pub(in crate::tui::ui) fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let dashboard = state;
    let focused = state.focus() == FocusPane::Guilds;
    let filter_query = state.guild_pane_filter_query();
    let block = panel_block("Servers", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (list_area, filter_area) = split_pane_filter_area(inner, filter_query.is_some());

    let entry_count = state.guild_pane_filtered_entries().len();
    let entries = state.visible_guild_pane_entries();
    let max_width = list_area.width.saturating_sub(6) as usize;
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
                        let base_style = selected_text_style(
                            is_selected,
                            active_text_style(
                                is_active,
                                theme::current().style(theme::HighlightGroup::Strong),
                            ),
                        );
                        let unread_count = state.direct_message_unread_count();
                        let badge = (unread_count > 0)
                            .then(|| {
                                notification_count_badge(ChannelUnreadState::Notified(
                                    u32::try_from(unread_count).unwrap_or(u32::MAX),
                                ))
                            })
                            .map(|badge| selected_text_span(is_selected, badge));
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
                        Line::from(spans)
                    }
                    GuildPaneEntry::FolderHeader { folder, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let icon = if *collapsed { "📁" } else { "📂" };
                        let folder_style = folder_style(folder.color);
                        let label = folder.name.as_deref().unwrap_or_default();
                        let title = if label.is_empty() {
                            icon.to_owned()
                        } else {
                            format!("{icon} {label}")
                        };
                        let label_width = max_width.saturating_sub(arrow.width());
                        let arrow_style =
                            selected_discord_text_style(is_selected, folder_style, folder.color);
                        let title_style = selected_discord_text_style(
                            is_selected,
                            theme::current().apply(theme::HighlightGroup::Strong, folder_style),
                            folder.color,
                        );
                        Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, arrow_style),
                            Span::styled(
                                truncate_display_width_from(&title, horizontal_scroll, label_width),
                                title_style,
                            ),
                        ])
                    }
                    GuildPaneEntry::Guild {
                        state: guild,
                        branch,
                    } => {
                        let prefix = branch.prefix();
                        let base_style = active_text_style(is_active, Style::default());
                        let is_muted = dashboard.guild_notification_muted(guild.id);
                        let unread = dashboard.sidebar_guild_unread(guild.id);
                        let (badge, mut name_style) = if is_active {
                            let (badge, _) = channel_unread_decoration(unread, base_style, false);
                            (badge, base_style)
                        } else if unread == ChannelUnreadState::Seen {
                            (None, base_style)
                        } else {
                            channel_unread_decoration(unread, base_style, false)
                        };
                        if is_muted {
                            name_style =
                                theme::current().apply(theme::HighlightGroup::Muted, name_style);
                        }
                        name_style = selected_text_style(is_selected, name_style);
                        let badge = badge.map(|badge| selected_text_span(is_selected, badge));
                        let badge_width =
                            badge.as_ref().map(|span| span.content.width()).unwrap_or(0);
                        let label_width = max_width
                            .saturating_sub(prefix.width())
                            .saturating_sub(badge_width);
                        let mut spans = vec![
                            selection_marker(is_selected),
                            Span::styled(
                                prefix,
                                theme::current().style(theme::HighlightGroup::Decoration),
                            ),
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
                        Line::from(spans)
                    }
                },
                is_selected,
            )
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, list_area);

    render_pane_filter_bar_with_cursor(
        frame,
        filter_area,
        filter_query,
        state.guild_pane_filter_cursor(),
        focused,
    );

    render_vertical_scrollbar(
        frame,
        list_area,
        state.guild_scroll(),
        list_area.height as usize,
        entry_count,
    );
}
