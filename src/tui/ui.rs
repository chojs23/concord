use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::{Resize, StatefulImage, protocol::StatefulProtocol};

use super::{
    format::truncate_text,
    state::{
        ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MemberGroup, folder_color,
        presence_color, presence_marker,
    },
};
use crate::discord::{AttachmentInfo, MessageState};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const MIN_MESSAGE_INPUT_HEIGHT: u16 = 3;
const IMAGE_PREVIEW_HEIGHT: u16 = 10;

pub struct ImagePreview<'a> {
    pub message_index: usize,
    pub state: ImagePreviewState<'a>,
}

pub enum ImagePreviewState<'a> {
    Loading { filename: String },
    Failed { filename: String, message: String },
    Ready { protocol: &'a mut StatefulProtocol },
}

#[derive(Clone, Copy)]
struct DashboardAreas {
    guilds: Rect,
    channels: Rect,
    messages: Rect,
    members: Rect,
    footer: Rect,
}

struct MessageAreas {
    list: Rect,
    composer: Rect,
}

pub fn sync_view_heights(area: Rect, state: &mut DashboardState, image_preview_count: usize) {
    let areas = dashboard_areas(area);
    state.set_guild_view_height(panel_content_height(areas.guilds, "Servers"));
    state.set_channel_view_height(panel_content_height(areas.channels, "Channels"));
    state.set_message_view_height(
        message_list_area(areas.messages, state, image_preview_count).height as usize,
    );
    state.set_member_view_height(panel_content_height(areas.members, "Members"));
}

pub fn render(frame: &mut Frame, state: &DashboardState, image_previews: Vec<ImagePreview<'_>>) {
    let areas = dashboard_areas(frame.area());

    render_guilds(frame, areas.guilds, state);
    render_channels(frame, areas.channels, state);
    render_messages(frame, areas.messages, state, image_previews);
    render_members(frame, areas.members, state);
    render_footer(frame, areas.footer, state);
}

fn dashboard_areas(area: Rect) -> DashboardAreas {
    let [main, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);

    let [guilds, channels, center, members] = Layout::horizontal([
        Constraint::Length(20),
        Constraint::Length(24),
        Constraint::Min(40),
        Constraint::Length(26),
    ])
    .areas(main);

    DashboardAreas {
        guilds,
        channels,
        messages: center,
        members,
        footer,
    }
}

fn render_guilds(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let entries = state.visible_guild_pane_entries();
    let max_width = area.width.saturating_sub(6) as usize;
    let selected = state.focused_guild_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = state.is_active_guild_entry(entry);
            styled_list_item(
                match entry {
                    GuildPaneEntry::DirectMessages => ListItem::new(Line::from(vec![
                        selection_marker(is_selected),
                        Span::styled(
                            truncate_text(entry.label(), max_width),
                            active_text_style(
                                is_active,
                                Style::default()
                                    .fg(Color::Magenta)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ),
                    ])),
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
                            selection_marker(is_selected),
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
                            selection_marker(is_selected),
                            Span::styled(prefix, Style::default().fg(DIM)),
                            Span::styled(
                                truncate_text(state.name.as_str(), label_width),
                                active_text_style(is_active, Style::default()),
                            ),
                        ]))
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
}

fn render_channels(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let entries = state.visible_channel_pane_entries();
    let max_width = area.width.saturating_sub(8) as usize;
    let selected = state.focused_channel_selection();
    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_selected = selected == Some(index);
            let is_active = state.is_active_channel_entry(entry);
            styled_list_item(
                match entry {
                    ChannelPaneEntry::CategoryHeader { state, collapsed } => {
                        let arrow = if *collapsed { "▶ " } else { "▼ " };
                        let label_width = max_width.saturating_sub(arrow.chars().count());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
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
                            selection_marker(is_selected),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                            Span::styled(channel_prefix, Style::default().fg(DIM)),
                            Span::styled(
                                truncate_text(&state.name, label_width),
                                active_text_style(is_active, Style::default()),
                            ),
                        ]))
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
}

fn render_messages(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
) {
    let title_text = state
        .selected_channel_state()
        .map(|channel| match channel.kind.as_str() {
            "dm" | "Private" => format!("@{}", channel.name),
            "group-dm" | "Group" => channel.name.clone(),
            _ => format!("#{}", channel.name),
        })
        .unwrap_or_else(|| "no channel".to_owned());

    let block = panel_block_owned(title_text, state.focus() == FocusPane::Messages);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let message_areas = message_areas(inner, state);
    let preview_height =
        inline_image_preview_height(message_areas.list, !image_previews.is_empty());

    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let max_author_width = 14usize;
    let padding = 4usize;
    let content_width = (message_areas.list.width as usize)
        .saturating_sub(padding)
        .saturating_sub(max_author_width + 2);

    let items: Vec<ListItem> = messages
        .iter()
        .enumerate()
        .flat_map(|(index, message)| {
            let author = truncate_text(&message.author, max_author_width);
            let content = format_message_content(message, content_width.max(8));
            let mut items = vec![styled_list_item(
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{author:<width$} ", width = max_author_width),
                        Style::default().fg(Color::Green).bold(),
                    ),
                    Span::raw(content),
                ])),
                selected == Some(index),
            )];
            if image_previews
                .iter()
                .any(|preview| preview.message_index == index)
                && preview_height > 0
            {
                items.push(image_preview_spacer(preview_height));
            }
            items
        })
        .collect();

    let list = List::new(items).highlight_style(highlight_style());

    frame.render_widget(list, message_areas.list);
    for (preview_offset, image_preview) in image_previews.into_iter().enumerate() {
        let row =
            inline_image_preview_row(image_preview.message_index, preview_height, preview_offset);
        if let Some(preview_area) =
            inline_image_preview_area(message_areas.list, row, preview_height)
        {
            render_image_preview(frame, preview_area, image_preview.state);
        }
    }
    render_composer(frame, message_areas.composer, state);
}

fn render_image_preview(frame: &mut Frame, area: Rect, image_preview: ImagePreviewState<'_>) {
    match image_preview {
        ImagePreviewState::Loading { filename } => frame.render_widget(
            Paragraph::new(format!("loading {filename}..."))
                .style(Style::default().fg(DIM))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ImagePreviewState::Failed { filename, message } => frame.render_widget(
            Paragraph::new(format!("{filename}: {message}"))
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            area,
        ),
        ImagePreviewState::Ready { protocol, .. } => {
            let widget = StatefulImage::new().resize(Resize::Fit(None));
            frame.render_stateful_widget(widget, area, protocol);
        }
    }
}

fn image_preview_spacer(height: u16) -> ListItem<'static> {
    let lines = (0..height).map(|_| Line::from("")).collect::<Vec<_>>();
    ListItem::new(lines)
}

fn format_message_content(message: &MessageState, width: usize) -> String {
    let attachment_summary =
        (!message.attachments.is_empty()).then(|| format_attachment_summary(&message.attachments));
    let content = match (message.content.as_deref(), attachment_summary.as_deref()) {
        (Some(value), Some(attachments)) if !value.is_empty() => format!("{value} {attachments}"),
        (Some(value), None) if !value.is_empty() => value.to_owned(),
        (_, Some(attachments)) => attachments.to_owned(),
        (Some(_), None) => "<empty message>".to_owned(),
        (None, None) => "<message content unavailable>".to_owned(),
    };

    truncate_text(&content, width)
}

fn format_attachment_summary(attachments: &[AttachmentInfo]) -> String {
    attachments
        .iter()
        .map(format_attachment)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_attachment(attachment: &AttachmentInfo) -> String {
    let kind = if is_image_attachment(attachment) {
        "image"
    } else {
        "file"
    };
    let dimensions = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!(" {width}x{height}"),
        _ => String::new(),
    };

    format!("[{kind}: {}]{}", attachment.filename, dimensions)
}

fn is_image_attachment(attachment: &AttachmentInfo) -> bool {
    attachment.is_image()
}

fn styled_list_item<'a>(item: ListItem<'a>, selected: bool) -> ListItem<'a> {
    if selected {
        item.style(highlight_style())
    } else {
        item
    }
}

fn selection_marker(selected: bool) -> Span<'static> {
    if selected {
        Span::styled(
            "▸ ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("  ")
    }
}

fn active_text_style(active: bool, style: Style) -> Style {
    if active {
        style.fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        style
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
        format!("press i to write in {label}")
    } else {
        "select a channel to write a message".to_owned()
    };

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
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(DIM))
                    .title_style(Style::default().fg(Color::White).bold()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_members(frame: &mut Frame, area: Rect, state: &DashboardState) {
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
            let mut name_style = Style::default().fg(
                if member.status == crate::discord::PresenceStatus::Offline {
                    DIM
                } else {
                    Color::White
                },
            );
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
                Span::styled(
                    format!(" {} ", presence_marker(member.status)),
                    marker_style,
                ),
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

fn panel_content_height(area: Rect, title: &'static str) -> usize {
    panel_block(title, false).inner(area).height.max(1) as usize
}

fn render_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let user = state.current_user().unwrap_or("connecting...");
    let mut spans = vec![
        Span::styled(
            format!(" {user} "),
            Style::default().fg(Color::Green).bold(),
        ),
        Span::styled(
            "tab/1-4 focus | j/k move | enter/space tree | ←/→ close/open | i write | esc cancel | q quit",
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

fn message_list_area(area: Rect, state: &DashboardState, image_preview_count: usize) -> Rect {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let list = message_areas(inner, state).list;
    let preview_height = inline_image_preview_height(list, image_preview_count > 0);
    Rect {
        height: list
            .height
            .saturating_sub(inline_image_preview_rows(
                preview_height,
                image_preview_count,
            ))
            .max(1),
        ..list
    }
}

fn message_areas(area: Rect, state: &DashboardState) -> MessageAreas {
    let composer_height = composer_height(area, state);
    let [list, composer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(composer_height)]).areas(area);
    MessageAreas { list, composer }
}

fn inline_image_preview_height(area: Rect, visible: bool) -> u16 {
    if !visible || area.height < 5 {
        0
    } else {
        IMAGE_PREVIEW_HEIGHT
            .min(area.height.saturating_sub(1))
            .max(3)
    }
}

fn inline_image_preview_rows(preview_height: u16, preview_count: usize) -> u16 {
    let count = u16::try_from(preview_count).unwrap_or(u16::MAX);
    preview_height.saturating_mul(count)
}

fn inline_image_preview_row(
    message_index: usize,
    preview_height: u16,
    previous_previews: usize,
) -> usize {
    message_index.saturating_add(usize::from(preview_height).saturating_mul(previous_previews))
}

fn inline_image_preview_area(list: Rect, row: usize, preview_height: u16) -> Option<Rect> {
    let row = u16::try_from(row).ok()?;
    if preview_height == 0 || row.saturating_add(1) >= list.height {
        return None;
    }

    let author_offset = 15u16.min(list.width.saturating_sub(1));
    let height = preview_height.min(list.height.saturating_sub(row).saturating_sub(1));
    (height > 0).then_some(Rect {
        x: list.x.saturating_add(author_offset),
        y: list.y.saturating_add(row).saturating_add(1),
        width: list.width.saturating_sub(author_offset),
        height,
    })
}

fn composer_height(area: Rect, state: &DashboardState) -> u16 {
    let content_lines = if state.is_composing() || !state.composer_input().is_empty() {
        composer_prompt_line_count(state.composer_input(), area.width)
    } else {
        1
    };
    MIN_MESSAGE_INPUT_HEIGHT.max(content_lines.saturating_add(1))
}

fn composer_prompt_line_count(input: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let prompt = format!("> {input}");
    prompt
        .split('\n')
        .map(|line| {
            let width_lines = line.chars().count().div_ceil(width);
            width_lines.max(1) as u16
        })
        .sum::<u16>()
        .max(1)
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use twilight_model::id::Id;

    use super::{
        format_message_content, inline_image_preview_area, inline_image_preview_row,
        sync_view_heights,
    };
    use crate::{
        discord::{AttachmentInfo, MessageState},
        tui::state::DashboardState,
    };

    #[test]
    fn sync_view_heights_reserves_message_input_inside_messages_pane() {
        let mut state = DashboardState::new();

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state, 0);

        assert_eq!(state.message_view_height(), 14);
    }

    #[test]
    fn sync_view_heights_reserves_multiline_message_input_inside_messages_pane() {
        let mut state = DashboardState::new();
        state.push_composer_char('a');
        state.push_composer_char('\n');
        state.push_composer_char('b');
        state.push_composer_char('\n');
        state.push_composer_char('c');

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state, 0);

        assert_eq!(state.message_view_height(), 13);
    }

    #[test]
    fn composer_height_accounts_for_soft_wrapping() {
        let mut state = DashboardState::new();
        for _ in 0..100 {
            state.push_composer_char('x');
        }

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state, 0);

        assert!(state.message_view_height() < 14);
    }

    #[test]
    fn image_attachment_replaces_empty_message_placeholder() {
        let message = message_with_attachment(Some(String::new()), image_attachment());

        assert_eq!(
            format_message_content(&message, 200),
            "[image: cat.png] 640x480"
        );
    }

    #[test]
    fn attachment_summary_is_appended_to_text_content() {
        let message = message_with_attachment(Some("look".to_owned()), image_attachment());

        assert_eq!(
            format_message_content(&message, 200),
            "look [image: cat.png] 640x480"
        );
    }

    #[test]
    fn sync_view_heights_reserves_inline_image_preview_rows() {
        let mut state = DashboardState::new();

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state, 1);

        assert_eq!(state.message_view_height(), 4);
    }

    #[test]
    fn sync_view_heights_reserves_multiple_inline_image_preview_rows() {
        let mut state = DashboardState::new();

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state, 2);

        assert_eq!(state.message_view_height(), 1);
    }

    #[test]
    fn inline_image_preview_slot_follows_image_message_content() {
        let area = Rect::new(10, 5, 80, 12);

        assert_eq!(
            inline_image_preview_area(area, 2, 4),
            Some(Rect::new(25, 8, 65, 4))
        );
    }

    #[test]
    fn later_image_preview_slot_accounts_for_prior_preview_rows() {
        let area = Rect::new(10, 5, 80, 18);
        let row = inline_image_preview_row(2, 4, 1);

        assert_eq!(row, 6);
        assert_eq!(
            inline_image_preview_area(area, row, 4),
            Some(Rect::new(25, 12, 65, 4))
        );
    }

    fn message_with_attachment(
        content: Option<String>,
        attachment: AttachmentInfo,
    ) -> MessageState {
        MessageState {
            id: Id::new(1),
            channel_id: Id::new(2),
            author: "neo".to_owned(),
            content,
            attachments: vec![attachment],
        }
    }

    fn image_attachment() -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(3),
            filename: "cat.png".to_owned(),
            url: "https://cdn.discordapp.com/cat.png".to_owned(),
            proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }
}
