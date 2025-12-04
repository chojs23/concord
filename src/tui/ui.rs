use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};

use super::{
    format::truncate_text,
    state::{
        ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MemberGroup, folder_color,
        presence_color, presence_marker,
    },
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

pub fn render(frame: &mut Frame, state: &mut DashboardState) {
    let [main, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let [guilds, channels, center, members] = Layout::horizontal([
        Constraint::Length(20),
        Constraint::Length(24),
        Constraint::Min(40),
        Constraint::Length(26),
    ])
    .areas(main);

    let [messages, composer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(center);

    render_guilds(frame, guilds, state);
    render_channels(frame, channels, state);
    render_messages(frame, messages, state);
    render_composer(frame, composer, state);
    render_members(frame, members, state);
    render_footer(frame, footer, state);
}

fn render_guilds(frame: &mut Frame, area: Rect, state: &mut DashboardState) {
    state.set_guild_view_height(panel_content_height(area));
    let entries = state.visible_guild_pane_entries();
    let max_width = area.width.saturating_sub(4) as usize;
    let selected = state.focused_guild_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| styled_list_item(match entry {
            GuildPaneEntry::DirectMessages => ListItem::new(Line::from(Span::styled(
                truncate_text(entry.label(), max_width),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ))),
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
                let label_width = max_width.saturating_sub(arrow.chars().count());
                ListItem::new(Line::from(vec![
                    Span::styled(arrow, Style::default().fg(color)),
                    Span::styled(
                        truncate_text(&title, label_width),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
            GuildPaneEntry::Guild { state, branch } => {
                let prefix = branch.prefix();
                let label_width = max_width.saturating_sub(prefix.chars().count());
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(DIM)),
                    Span::raw(truncate_text(state.name.as_str(), label_width)),
                ]))
            }
        }, selected == Some(index)))
        .collect();

    let list = List::new(items)
        .block(panel_block("Servers", state.focus() == FocusPane::Guilds))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
}

fn render_channels(frame: &mut Frame, area: Rect, state: &mut DashboardState) {
    state.set_channel_view_height(panel_content_height(area));
    let entries = state.visible_channel_pane_entries();
    let max_width = area.width.saturating_sub(6) as usize;
    let selected = state.focused_channel_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| styled_list_item(match entry {
            ChannelPaneEntry::CategoryHeader { state, collapsed } => {
                let arrow = if *collapsed { "▶ " } else { "▼ " };
                let label_width = max_width.saturating_sub(arrow.chars().count());
                ListItem::new(Line::from(vec![
                    Span::styled(arrow, Style::default().fg(ACCENT)),
                    Span::styled(
                        truncate_text(&state.name, label_width),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
            ChannelPaneEntry::Channel { state, branch } => {
                let branch_prefix = branch.prefix();
                let channel_prefix = channel_prefix(&state.kind);
                let label_width = max_width
                    .saturating_sub(branch_prefix.chars().count())
                    .saturating_sub(channel_prefix.chars().count());
                ListItem::new(Line::from(vec![
                    Span::styled(branch_prefix, Style::default().fg(DIM)),
                    Span::styled(channel_prefix, Style::default().fg(DIM)),
                    Span::raw(truncate_text(&state.name, label_width)),
                ]))
            }
        }, selected == Some(index)))
        .collect();

    let list = List::new(items)
        .block(panel_block(
            "Channels",
            state.focus() == FocusPane::Channels,
        ))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
}

fn render_messages(frame: &mut Frame, area: Rect, state: &mut DashboardState) {
    state.set_message_view_height(panel_content_height(area));
    let title_text = state
        .selected_channel_state()
        .map(|channel| match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        })
        .unwrap_or_else(|| "no channel".to_owned());

    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let max_author_width = 14usize;
    let padding = 4usize;
    let content_width = (area.width as usize)
        .saturating_sub(padding)
        .saturating_sub(max_author_width + 2);

    let items: Vec<ListItem> = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let author = truncate_text(&message.author, max_author_width);
            let content = match message.content.as_deref() {
                Some(value) if !value.is_empty() => truncate_text(value, content_width.max(8)),
                Some(_) => "<empty message>".to_owned(),
                None => "<message content unavailable>".to_owned(),
            };
            styled_list_item(
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{author:<width$} ", width = max_author_width),
                        Style::default().fg(Color::Green).bold(),
                    ),
                    Span::raw(content),
                ])),
                selected == Some(index),
            )
        })
        .collect();

    let list = List::new(items)
        .block(panel_block_owned(
            title_text,
            state.focus() == FocusPane::Messages,
        ))
        .highlight_style(highlight_style());

    frame.render_widget(list, area);
}

fn styled_list_item<'a>(item: ListItem<'a>, selected: bool) -> ListItem<'a> {
    if selected {
        item.style(highlight_style())
    } else {
        item
    }
}

fn render_composer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let prompt = if state.is_composing() {
        format!("> {}", state.composer_input())
    } else if let Some(channel) = state.selected_channel_state() {
        let label = match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        };
        format!("press i to compose in {label}")
    } else {
        "select a channel to start composing".to_owned()
    };

    frame.render_widget(
        Paragraph::new(prompt)
            .style(if state.is_composing() {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(DIM)
            })
            .block(panel_block(
                "Composer",
                state.focus() == FocusPane::Composer,
            ))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_members(frame: &mut Frame, area: Rect, state: &mut DashboardState) {
    state.set_member_view_height(panel_content_height(area));
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
            let is_selected = focused && selected_line == Some(line_index);
            let marker_style = Style::default().fg(presence_color(member.status));
            let mut name_style = Style::default().fg(if member.status == crate::discord::PresenceStatus::Offline {
                DIM
            } else {
                Color::White
            });
            if member.is_bot {
                name_style = name_style.add_modifier(Modifier::ITALIC);
            }
            if is_selected {
                name_style = name_style
                    .bg(Color::Rgb(24, 54, 65))
                    .add_modifier(Modifier::BOLD);
            }

            let mut display = truncate_text(&member.display_name, max_name_width);
            if member.is_bot {
                display = format!("{display} [bot]");
            }
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", presence_marker(member.status)), marker_style),
                Span::styled(display, name_style),
            ]));
            line_index += 1;
        }
    }

    let lines: Vec<_> = lines
        .into_iter()
        .skip(state.member_scroll())
        .take(state.member_content_height())
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Members", focused))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn member_group_header(group: &MemberGroup<'_>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            group.status.label().to_owned(),
            Style::default()
                .fg(presence_color(group.status))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", group.entries.len()),
            Style::default().fg(DIM),
        ),
    ])
}

fn panel_content_height(area: Rect) -> usize {
    area.height.saturating_sub(2).max(1) as usize
}

fn render_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let user = state.current_user().unwrap_or("connecting...");
    let mut spans = vec![
        Span::styled(
            format!(" {user} "),
            Style::default().fg(Color::Green).bold(),
        ),
        Span::styled(
            "tab focus | j/k move | enter/space tree | ←/→ close/open | i compose | esc cancel | q quit",
            Style::default().fg(DIM),
        ),
    ];
    if let Some(error) = state.last_error() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("err: {}", truncate_text(error, 60)),
            Style::default().fg(Color::Red),
        ));
    }
    if state.skipped_events() > 0 {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("lagged {}", state.skipped_events()),
            Style::default().fg(Color::Yellow),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
        area,
    );
}

fn channel_prefix(kind: &str) -> &'static str {
    match kind {
        "dm" | "Private" => "@ ",
        "group-dm" | "Group" => "● ",
        "voice" | "GuildVoice" => "🔊 ",
        "category" | "GuildCategory" => "▾ ",
        "thread" | "GuildPublicThread" | "GuildPrivateThread" | "GuildNewsThread" => "» ",
        _ => "# ",
    }
}

fn highlight_style() -> Style {
    Style::default()
        .bg(Color::Rgb(24, 54, 65))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

fn panel_block(title: &'static str, focused: bool) -> Block<'static> {
    panel_block_owned(title.to_owned(), focused)
}

fn panel_block_owned(title: String, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { Color::DarkGray };

    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border))
        .title_style(Style::default().fg(Color::White).bold())
}
