use super::*;
use crate::tui::message::format::wrap_text_lines;
use crate::tui::state::{
    NotificationInboxChannelLoad, NotificationInboxItem, NotificationInboxLoad,
    NotificationInboxMessage, NotificationInboxTab, NotificationInboxUnreadItem,
};
use crate::tui::ui::message::list::message_author_style;

const NOTIFICATION_INBOX_POPUP_WIDTH: u16 = 82;

pub(in crate::tui::ui) fn render_notification_inbox_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(tab) = state.notification_inbox_tab() else {
        return;
    };

    let popup = notification_inbox_popup_area(area);
    let inner_width = usize::from(popup.width.saturating_sub(2)).max(1);

    let inner = render_modal_frame(frame, popup, "Inbox");
    frame.render_widget(
        Paragraph::new(notification_inbox_lines(
            state,
            tab,
            usize::from(inner.height),
            inner_width,
        )),
        inner,
    );
}

pub(in crate::tui::ui) fn notification_inbox_popup_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(2).clamp(10, 30);
    centered_rect(area, NOTIFICATION_INBOX_POPUP_WIDTH, height)
}

fn notification_inbox_lines(
    state: &DashboardState,
    tab: NotificationInboxTab,
    available_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let items = state.notification_inbox_items();
    let selected = state.selected_notification_inbox_index().unwrap_or(0);
    let help_lines = notification_inbox_help_lines(state, tab, width);
    let body_lines = available_lines.saturating_sub(3 + help_lines.len()).max(1);

    let mut lines = vec![
        notification_inbox_tab_line(
            tab,
            state.notification_inbox_unread_count(),
            state.notification_inbox_mention_count(),
        ),
        Line::from(Span::styled(
            "─".repeat(width.max(1)),
            theme::current().style(theme::HighlightGroup::Decoration),
        )),
    ];

    let status = match tab {
        NotificationInboxTab::Unreads => None,
        NotificationInboxTab::Mentions => state.notification_inbox_mentions_status(),
    };
    if status == Some(NotificationInboxLoad::Loading) {
        lines.push(notification_inbox_notice_line("Loading mentions…"));
    } else if status == Some(NotificationInboxLoad::Failed) {
        lines.push(notification_inbox_notice_line("Failed to load mentions."));
    } else if items.is_empty() {
        lines.push(notification_inbox_notice_line(match tab {
            NotificationInboxTab::Unreads => "You're all caught up! No unread channels.",
            NotificationInboxTab::Mentions => "No recent mentions.",
        }));
    } else {
        lines.extend(notification_inbox_body_lines(
            &items, selected, body_lines, width,
        ));
    }

    let footer_start = available_lines.saturating_sub(1 + help_lines.len());
    while lines.len() < footer_start {
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(String::new(), Style::default())));
    lines.extend(help_lines);
    lines
}

fn notification_inbox_notice_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_owned(),
        theme::current().style(theme::HighlightGroup::Hint),
    ))
}

fn notification_inbox_tab_line(
    tab: NotificationInboxTab,
    unread_count: usize,
    mention_count: usize,
) -> Line<'static> {
    let tab_span = |label: &str, count: usize, active: bool| {
        let text = format!(" {label} ({count}) ");
        let style = if active {
            theme::current().style(theme::HighlightGroup::ActiveTab)
        } else {
            theme::current().style(theme::HighlightGroup::Disabled)
        };
        Span::styled(text, style)
    };
    let separator = || {
        Span::styled(
            "│",
            theme::current().style(theme::HighlightGroup::Decoration),
        )
    };
    Line::from(vec![
        tab_span(
            "Unreads",
            unread_count,
            tab == NotificationInboxTab::Unreads,
        ),
        separator(),
        tab_span(
            "Mentions",
            mention_count,
            tab == NotificationInboxTab::Mentions,
        ),
    ])
}

fn notification_inbox_body_lines(
    items: &[NotificationInboxItem],
    selected: usize,
    body_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let mut rows = Vec::<(Line<'static>, Option<usize>)>::new();
    for (index, item) in items.iter().enumerate() {
        for (offset, line) in notification_inbox_card_lines(item, index == selected, width)
            .into_iter()
            .enumerate()
        {
            rows.push((line, (offset == 0).then_some(index)));
        }
    }

    let total = rows.len();
    let start = if total <= body_lines {
        0
    } else {
        let selected_line = rows
            .iter()
            .position(|(_, index)| *index == Some(selected))
            .unwrap_or_default();
        selected_line
            .saturating_sub(body_lines / 3)
            .min(total - body_lines)
    };
    rows[start..total.min(start + body_lines)]
        .iter()
        .map(|(line, _)| line.clone())
        .collect()
}

fn notification_inbox_card_lines(
    item: &NotificationInboxItem,
    selected: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let marker = selectable_popup_marker(selected);
    let card_width = width.saturating_sub(marker.content.width()).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);
    let border = notification_inbox_border_style(selected);
    let (header, body) = notification_inbox_card_content(item);

    let mut lines = vec![
        Line::from(vec![
            marker,
            Span::styled(
                format!("╭{}╮", "─".repeat(card_width.saturating_sub(2))),
                border,
            ),
        ]),
        notification_inbox_inner_line(header, inner_width, selected),
    ];
    for content in body {
        lines.push(notification_inbox_inner_line(
            content,
            inner_width,
            selected,
        ));
    }
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
            border,
        ),
    ]));
    lines
}

fn notification_inbox_card_content(
    item: &NotificationInboxItem,
) -> (Vec<Span<'static>>, Vec<Vec<Span<'static>>>) {
    match item {
        NotificationInboxItem::Unread(item) => {
            let header = notification_inbox_unread_header(item);
            let body = if item.messages.is_empty() {
                vec![vec![Span::styled(
                    notification_inbox_unread_placeholder(item),
                    theme::current().style(theme::HighlightGroup::Placeholder),
                )]]
            } else {
                item.messages
                    .iter()
                    .map(notification_inbox_message_spans)
                    .collect()
            };
            (header, body)
        }
        NotificationInboxItem::Mention(item) => {
            let mut header = vec![Span::styled(
                format!("@ {}", item.title),
                theme::current().style(theme::HighlightGroup::Strong),
            )];
            if let Some(context) = &item.context {
                header.push(Span::styled(
                    format!("  {context}"),
                    theme::current().style(theme::HighlightGroup::SearchContext),
                ));
            }
            (
                header,
                vec![notification_inbox_message_spans(
                    &NotificationInboxMessage {
                        author_id: item.author_id,
                        author: item.author.clone(),
                        author_role_ids: item.author_role_ids.clone(),
                        author_role_color: item.author_role_color,
                        content: item.content.clone(),
                    },
                )],
            )
        }
    }
}

fn notification_inbox_inner_line(
    content: Vec<Span<'static>>,
    inner_width: usize,
    selected: bool,
) -> Line<'static> {
    let body = truncate_line_to_display_width(Line::from(content), inner_width);
    let border = notification_inbox_border_style(selected);
    let mut spans = vec![Span::raw("  "), Span::styled("│ ", border)];
    spans.extend(body.spans);
    spans.push(Span::styled(" │", border));
    Line::from(spans)
}

fn notification_inbox_unread_header(item: &NotificationInboxUnreadItem) -> Vec<Span<'static>> {
    let (badge, title_style) = channel_unread_decoration(item.unread, Style::default(), false);
    let mut spans = Vec::new();
    if let Some(badge) = badge {
        spans.push(badge);
    }
    spans.push(Span::styled(
        item.title.clone(),
        theme::current().apply(theme::HighlightGroup::Strong, title_style),
    ));
    if let Some(context) = &item.context {
        spans.push(Span::styled(
            format!("  {context}"),
            theme::current().style(theme::HighlightGroup::SearchContext),
        ));
    }
    spans
}

fn notification_inbox_message_spans(message: &NotificationInboxMessage) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("{}: ", message.author),
            message_author_style(message.author_role_color),
        ),
        Span::styled(
            message.content.clone(),
            theme::current().style(theme::HighlightGroup::Description),
        ),
    ]
}

fn notification_inbox_unread_placeholder(item: &NotificationInboxUnreadItem) -> String {
    if item.load == NotificationInboxChannelLoad::Loading {
        return "loading…".to_owned();
    }
    match item.unread {
        ChannelUnreadState::Mentioned(count) => {
            format!("{count} new mention{}", plural_suffix(count))
        }
        ChannelUnreadState::Notified(count) => {
            format!("{count} new message{}", plural_suffix(count))
        }
        ChannelUnreadState::Unread => "New messages".to_owned(),
        ChannelUnreadState::Seen => "No recent messages".to_owned(),
    }
}

fn notification_inbox_border_style(selected: bool) -> Style {
    if selected {
        theme::current().style(theme::HighlightGroup::SelectionBorder)
    } else {
        theme::current().style(theme::HighlightGroup::Border)
    }
}

fn plural_suffix(count: u32) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn notification_inbox_help_lines(
    state: &DashboardState,
    tab: NotificationInboxTab,
    width: usize,
) -> Vec<Line<'static>> {
    let key_bindings = state.key_bindings();
    let activate = key_bindings.notification_inbox_activate_label();
    let mark_read = key_bindings.notification_inbox_mark_read_label();
    let mark_all_read = key_bindings.notification_inbox_mark_all_read_label();
    let switch_tab = key_bindings.notification_inbox_tab_switch_label(tab);

    let mut shortcuts = vec![(activate.as_str(), "open")];
    if !mark_read.is_empty() {
        shortcuts.push((mark_read.as_str(), "mark read"));
    }
    if tab == NotificationInboxTab::Unreads && !mark_all_read.is_empty() {
        shortcuts.push((mark_all_read.as_str(), "mark all read"));
    }
    if !switch_tab.is_empty() {
        shortcuts.push((switch_tab.as_str(), "switch tab"));
    }

    notification_inbox_help_text_lines(&shortcuts, width)
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                theme::current().style(theme::HighlightGroup::Hint),
            ))
        })
        .collect()
}

fn notification_inbox_help_text_lines(items: &[(&str, &str)], width: usize) -> Vec<String> {
    let width = width.max(1);
    let separator = " · ";
    let mut lines = Vec::new();
    let mut current = String::new();

    for item in items {
        let entry = popup_shortcut_help_text(&[*item]);
        if current.is_empty() {
            if entry.width() <= width {
                current = entry;
            } else {
                lines.extend(wrap_text_lines(&entry, width));
            }
            continue;
        }

        if current.width() + separator.width() + entry.width() <= width {
            current.push_str(separator);
            current.push_str(&entry);
        } else {
            lines.push(current);
            current = String::new();
            if entry.width() <= width {
                current = entry;
            } else {
                lines.extend(wrap_text_lines(&entry, width));
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::{KeymapBinding, KeymapOptions};

    #[test]
    fn notification_inbox_helper_uses_primary_keys_and_wraps_at_popup_width() {
        let default_state = DashboardState::new();
        let default_help =
            notification_inbox_help_lines(&default_state, NotificationInboxTab::Mentions, 80)
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>()
                .join("\n");

        assert!(default_help.contains("[h/l] switch tab"), "{default_help}");

        let notification_inbox_actions = BTreeMap::from([
            ("MarkRead".to_owned(), KeymapBinding::one("q")),
            ("MarkAllRead".to_owned(), KeymapBinding::one("w")),
        ]);
        let mappings = BTreeMap::from([
            (
                "CycleFocusPrevious".to_owned(),
                KeymapBinding {
                    keys: vec!["z".to_owned(), "Left".to_owned()],
                    description: None,
                },
            ),
            (
                "CycleFocusNext".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned(), "Right".to_owned()],
                    description: None,
                },
            ),
        ]);
        let state = DashboardState::new_with_options(
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            KeymapOptions {
                notification_inbox_actions,
                mappings,
                ..Default::default()
            },
            Default::default(),
        );

        let help_lines = notification_inbox_help_lines(&state, NotificationInboxTab::Unreads, 36);
        let rendered = help_lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("[q] mark read"), "{rendered}");
        assert!(rendered.contains("[w] mark all read"), "{rendered}");
        assert!(rendered.contains("[z/x] switch tab"), "{rendered}");
        assert!(!rendered.contains("Left"), "{rendered}");
        assert!(!rendered.contains("Right"), "{rendered}");
        assert!(!rendered.contains("[r]"), "{rendered}");
        assert!(!rendered.contains("[a]"), "{rendered}");
        assert!(help_lines.len() > 1);
        assert!(help_lines.iter().all(|line| line.width() <= 36));

        let popup_lines = notification_inbox_lines(&state, NotificationInboxTab::Unreads, 8, 36);
        assert_eq!(popup_lines.len(), 8);
        assert_eq!(
            &popup_lines[popup_lines.len() - help_lines.len()..],
            help_lines.as_slice()
        );
    }
}
