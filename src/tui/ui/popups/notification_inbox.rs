use super::*;
use crate::tui::state::{
    NotificationInboxChannelLoad, NotificationInboxItem, NotificationInboxLoad,
    NotificationInboxMessage, NotificationInboxTab,
};

const NOTIFICATION_INBOX_POPUP_WIDTH: u16 = 76;

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
    // The block borders take 2 rows; the tab line, the rule, a spacer, and the
    // help line take 4 more. The rest is the scrollable channel/message body.
    let body_lines = usize::from(popup.height.saturating_sub(6)).max(1);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(notification_inbox_lines(
            state,
            tab,
            body_lines,
            inner_width,
        ))
        .block(panel_block("Inbox", true)),
        popup,
    );
}

pub(in crate::tui::ui) fn notification_inbox_popup_area(area: Rect) -> Rect {
    let height = area.height.saturating_sub(2).clamp(10, 28);
    centered_rect(area, NOTIFICATION_INBOX_POPUP_WIDTH, height)
}

fn notification_inbox_lines(
    state: &DashboardState,
    tab: NotificationInboxTab,
    body_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let items = state.notification_inbox_items();
    let selected = state.selected_notification_inbox_index().unwrap_or(0);

    let mut lines = vec![
        notification_inbox_tab_line(
            tab,
            state.notification_inbox_unread_count(),
            state.notification_inbox_mention_count(),
        ),
        Line::from(Span::styled(
            "─".repeat(width.max(1)),
            Style::default().fg(DIM),
        )),
    ];

    let mentions_loading = tab == NotificationInboxTab::Mentions
        && state.notification_inbox_mentions_status() == Some(NotificationInboxLoad::Loading);
    let mentions_failed = tab == NotificationInboxTab::Mentions
        && state.notification_inbox_mentions_status() == Some(NotificationInboxLoad::Failed);

    if mentions_loading {
        lines.push(notification_inbox_notice_line("Loading mentions…"));
    } else if mentions_failed {
        lines.push(notification_inbox_notice_line("Failed to load mentions."));
    } else if items.is_empty() {
        lines.push(notification_inbox_notice_line(match tab {
            NotificationInboxTab::Unreads => "You're all caught up! No unread channels.",
            NotificationInboxTab::Mentions => "No new mentions.",
        }));
    } else {
        lines.extend(notification_inbox_body_lines(
            &items, selected, body_lines, width,
        ));
    }

    lines.push(Line::from(Span::styled(String::new(), Style::default())));
    if state.notification_inbox_is_confirming_mark_all() {
        lines.push(notification_inbox_confirm_line(tab));
    } else {
        lines.push(notification_inbox_help_line());
    }
    lines
}

fn notification_inbox_confirm_line(tab: NotificationInboxTab) -> Line<'static> {
    let scope = match tab {
        NotificationInboxTab::Unreads => "all unread channels",
        NotificationInboxTab::Mentions => "all mentions",
    };
    Line::from(vec![
        Span::styled(
            format!("Mark {scope} as read? "),
            Style::default()
                .fg(MENTION_ORANGE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("[y] confirm  [n] cancel", Style::default().fg(DIM)),
    ])
}

fn notification_inbox_notice_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_owned(), Style::default().fg(DIM)))
}

fn notification_inbox_tab_line(
    tab: NotificationInboxTab,
    unread_count: usize,
    mention_count: usize,
) -> Line<'static> {
    let tab_span = |label: &str, count: usize, active: bool| {
        let text = format!(" {label} ({count}) ");
        let style = if active {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        Span::styled(text, style)
    };
    Line::from(vec![
        tab_span(
            "Unreads",
            unread_count,
            tab == NotificationInboxTab::Unreads,
        ),
        Span::styled("│", Style::default().fg(DIM)),
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
    let mut rows: Vec<(Line<'static>, Option<usize>)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let card = notification_inbox_card_lines(item, index == selected, width);
        for (offset, line) in card.into_iter().enumerate() {
            // The card's top border carries the channel index for scroll anchoring.
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
            .unwrap_or(0);
        // Keep the selected card's top border in the upper third.
        selected_line
            .saturating_sub(body_lines / 3)
            .min(total - body_lines)
    };
    let end = total.min(start + body_lines);
    rows[start..end]
        .iter()
        .map(|(line, _)| line.clone())
        .collect()
}

fn notification_inbox_card_lines(
    item: &NotificationInboxItem,
    selected: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let marker = if selected { "› " } else { "  " };
    let card_width = width.saturating_sub(marker.width()).max(4);
    let inner_width = card_width.saturating_sub(4).max(1);
    let border = notification_inbox_border_style(selected);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(marker, border),
            Span::styled(
                format!("╭{}╮", "─".repeat(card_width.saturating_sub(2))),
                border,
            ),
        ]),
        notification_inbox_inner_line(notification_inbox_header_spans(item), inner_width, selected),
    ];

    if item.messages.is_empty() {
        lines.push(notification_inbox_inner_line(
            vec![Span::styled(
                notification_inbox_placeholder_text(item),
                Style::default().fg(DIM),
            )],
            inner_width,
            selected,
        ));
    } else {
        for message in &item.messages {
            lines.push(notification_inbox_inner_line(
                notification_inbox_message_spans(message),
                inner_width,
                selected,
            ));
        }
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

fn notification_inbox_header_spans(item: &NotificationInboxItem) -> Vec<Span<'static>> {
    let (badge, title_style) = channel_unread_decoration(item.unread, Style::default(), false);
    let mut spans = Vec::new();
    if let Some(badge) = badge {
        spans.push(badge);
    }
    spans.push(Span::styled(
        item.title.clone(),
        title_style.add_modifier(Modifier::BOLD),
    ));
    if let Some(context) = &item.context {
        spans.push(Span::styled(
            format!("  {context}"),
            Style::default().fg(DIM),
        ));
    }
    spans
}

fn notification_inbox_message_spans(message: &NotificationInboxMessage) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!("{}: ", message.author),
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        ),
        Span::styled(message.content.clone(), Style::default().fg(DIM)),
    ]
}

fn notification_inbox_placeholder_text(item: &NotificationInboxItem) -> String {
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
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    }
}

fn plural_suffix(count: u32) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn notification_inbox_help_line() -> Line<'static> {
    Line::from(Span::styled(
        popup_shortcut_help_text(&[
            ("Enter", "open"),
            ("r", "mark read"),
            ("a", "mark all read"),
            ("←/→", "switch tab"),
            ("Esc", "close"),
        ]),
        Style::default().fg(DIM),
    ))
}
