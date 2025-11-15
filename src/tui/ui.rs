use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::{
    format::{EventItem, truncate_text},
    state::{DashboardState, EventFilter, FocusPane},
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

pub fn render(frame: &mut Frame, state: &DashboardState) {
    let [main, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [body, composer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(main);
    let [left, content] =
        Layout::horizontal([Constraint::Length(36), Constraint::Min(0)]).areas(body);
    let [sidebar, guilds, channels] = Layout::vertical([
        Constraint::Length(8),
        Constraint::Percentage(35),
        Constraint::Min(0),
    ])
    .areas(left);
    let [messages, lower] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Min(0)]).areas(content);
    let [events, detail] =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Min(0)]).areas(lower);

    render_sidebar(frame, sidebar, state);
    render_guilds(frame, guilds, state);
    render_channels(frame, channels, state);
    render_messages(frame, messages, state);
    render_events(frame, events, state);
    render_detail(frame, detail, state);
    render_composer(frame, composer, state);
    render_footer(frame, footer);
}

fn render_sidebar(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let status = if state.current_user().is_some() {
        "connected"
    } else {
        "connecting"
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Status ", Style::default().fg(DIM)),
            Span::raw(status),
        ]),
        Line::from(vec![
            Span::styled("User   ", Style::default().fg(DIM)),
            Span::raw(state.current_user().unwrap_or("waiting for Ready")),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Events ", Style::default().fg(DIM)),
            Span::raw(state.total_events().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Filter ", Style::default().fg(DIM)),
            Span::styled(state.filter().to_string(), Style::default().fg(ACCENT)),
        ]),
        Line::from(vec![
            Span::styled("Lagged ", Style::default().fg(DIM)),
            Span::raw(state.skipped_events().to_string()),
        ]),
        Line::from(""),
        filter_line("1", "all", state.filter() == EventFilter::All),
        filter_line("2", "messages", state.filter() == EventFilter::Messages),
        filter_line("3", "gateway", state.filter() == EventFilter::Gateway),
        filter_line("4", "errors", state.filter() == EventFilter::Errors),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Discord", state.focus() == FocusPane::Sidebar))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let guilds = state.guilds();
    let items: Vec<ListItem> = guilds
        .iter()
        .map(|guild| ListItem::new(guild.name.clone()))
        .collect();
    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(state.selected_guild()));
    }

    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block("Guilds", state.focus() == FocusPane::Guilds))
            .highlight_style(selected_style())
            .highlight_symbol("▶ "),
        area,
        &mut list_state,
    );
}

fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let channels = state.channels();
    let items: Vec<ListItem> = channels
        .iter()
        .map(|channel| {
            ListItem::new(Line::from(vec![
                Span::styled("# ", Style::default().fg(DIM)),
                Span::raw(channel.name.clone()),
                Span::styled(format!(" {}", channel.kind), Style::default().fg(DIM)),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(state.selected_channel()));
    }

    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block(
                "Channels",
                state.focus() == FocusPane::Channels,
            ))
            .highlight_style(selected_style())
            .highlight_symbol("▶ "),
        area,
        &mut list_state,
    );
}

fn render_messages(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let messages = state.messages();
    let items: Vec<ListItem> = messages
        .iter()
        .map(|message| {
            let content = message.content.as_deref().unwrap_or("<unavailable>");
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<12}", truncate_text(&message.author, 12)),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(truncate_text(content, 120)),
            ]))
        })
        .collect();
    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(state.selected_message()));
    }

    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block(
                "Messages",
                state.focus() == FocusPane::Messages,
            ))
            .highlight_style(selected_style())
            .highlight_symbol("▶ "),
        area,
        &mut list_state,
    );
}

fn render_events(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let visible = state.visible_events();
    let items: Vec<ListItem> = visible.iter().map(|event| event_row(event)).collect();
    let mut list_state = ListState::default();

    if !items.is_empty() {
        list_state.select(Some(state.selected()));
    }

    let list = List::new(items)
        .block(panel_block("Events", state.focus() == FocusPane::Events))
        .highlight_style(selected_style())
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut list_state);
}

fn render_detail(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let detail = if state.focus() == FocusPane::Messages {
        state
            .selected_message_item()
            .map(|message| {
                format!(
                    "Message\n\nChannel: {}\nMessage: {}\nAuthor: {} ({})\n\n{}",
                    message.channel_id.get(),
                    message.id.get(),
                    message.author,
                    message.author_id.get(),
                    message.content.as_deref().unwrap_or("<unavailable>")
                )
            })
            .unwrap_or_else(|| "No message selected.".to_owned())
    } else {
        state
            .selected_event()
            .map(|event| event.detail.clone())
            .unwrap_or_else(|| {
                "No event selected yet. Waiting for Discord gateway events...".to_owned()
            })
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(detail)
            .block(panel_block("Detail", state.focus() == FocusPane::Detail))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_composer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let channel = state
        .selected_channel_id()
        .map(|id| format!("channel {}", id.get()))
        .unwrap_or_else(|| "no channel selected".to_owned());
    let prompt = if state.is_composing() {
        format!("> {}", state.composer_input())
    } else {
        format!("press i to compose for {channel}")
    };

    frame.render_widget(
        Paragraph::new(prompt)
            .block(panel_block(
                "Composer",
                state.focus() == FocusPane::Composer,
            ))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(
            "q quit | tab focus | j/k move | i compose | enter send | esc cancel | 1 all 2 msg 3 gate 4 err | c clear",
        )
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center),
        area,
    );
}

fn selected_style() -> Style {
    Style::default()
        .bg(Color::Rgb(24, 54, 65))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

fn event_row(event: &EventItem) -> ListItem<'static> {
    let summary = truncate_text(&event.summary, 96);

    ListItem::new(Line::from(vec![
        Span::styled(format!("#{:<4}", event.seq), Style::default().fg(DIM)),
        Span::styled(
            format!("{:<7}", event.label()),
            Style::default().fg(event.color()),
        ),
        Span::raw(summary),
    ]))
}

fn filter_line(key: &'static str, label: &'static str, active: bool) -> Line<'static> {
    let style = if active {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };

    Line::from(vec![
        Span::styled(format!("{key} "), Style::default().fg(DIM)),
        Span::styled(label, style),
    ])
}

fn panel_block(title: &'static str, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { Color::DarkGray };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border))
        .title_style(Style::default().fg(Color::White).bold())
}
