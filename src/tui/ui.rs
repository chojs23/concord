use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::{Image as RatatuiImage, Resize, StatefulImage, protocol::StatefulProtocol};
use twilight_model::id::{
    Id,
    marker::{GuildMarker, MessageMarker},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{
    format::{truncate_display_width, truncate_text},
    state::{
        ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MemberGroup,
        MessageActionItem, folder_color, presence_color, presence_marker,
    },
};
use crate::discord::{
    AttachmentInfo, MessageKind, MessageSnapshotInfo, MessageState, PollInfo, ReplyInfo,
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const MIN_MESSAGE_INPUT_HEIGHT: u16 = 3;
const IMAGE_PREVIEW_HEIGHT: u16 = 10;
const IMAGE_PREVIEW_WIDTH: u16 = 72;
const MESSAGE_AVATAR_PLACEHOLDER: &str = "oo";
const MESSAGE_AVATAR_OFFSET: u16 = 3;
const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;

#[derive(Clone)]
struct MessageContentLine {
    text: String,
    style: Style,
}

impl MessageContentLine {
    fn plain(text: String) -> Self {
        Self {
            text,
            style: Style::default(),
        }
    }

    fn dim(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(DIM),
        }
    }

    fn accent(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(ACCENT),
        }
    }
}

pub struct ImagePreview<'a> {
    pub message_index: usize,
    pub preview_height: u16,
    pub state: ImagePreviewState<'a>,
}

pub struct AvatarImage {
    pub row: isize,
    pub visible_height: u16,
    pub protocol: ratatui_image::protocol::Protocol,
}

#[derive(Clone, Copy)]
pub struct ImagePreviewLayout {
    pub list_height: usize,
    pub content_width: usize,
    pub preview_width: u16,
    pub max_preview_height: u16,
}

pub enum ImagePreviewState<'a> {
    Loading { filename: String },
    Failed { filename: String, message: String },
    Ready { protocol: &'a mut StatefulProtocol },
    ReadyCropped(ratatui_image::protocol::Protocol),
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

pub fn sync_view_heights(area: Rect, state: &mut DashboardState) {
    let areas = dashboard_areas(area);
    state.set_guild_view_height(panel_content_height(areas.guilds, "Servers"));
    state.set_channel_view_height(panel_content_height(areas.channels, "Channels"));
    state.set_message_view_height(message_list_area(areas.messages, state).height as usize);
    state.set_member_view_height(panel_content_height(areas.members, "Members"));
}

pub fn image_preview_layout(area: Rect, state: &DashboardState) -> ImagePreviewLayout {
    let areas = dashboard_areas(area);
    let list = message_list_area(areas.messages, state);
    ImagePreviewLayout {
        list_height: list.height as usize,
        content_width: message_content_width(list),
        preview_width: inline_image_preview_width(list),
        max_preview_height: inline_image_preview_height(list, true),
    }
}

pub fn render(
    frame: &mut Frame,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
    avatar_images: Vec<AvatarImage>,
) {
    let areas = dashboard_areas(frame.area());

    render_guilds(frame, areas.guilds, state);
    render_channels(frame, areas.channels, state);
    render_messages(frame, areas.messages, state, image_previews, avatar_images);
    render_members(frame, areas.members, state);
    render_footer(frame, areas.footer, state);
    render_message_action_menu(frame, areas.messages, state);
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
    avatar_images: Vec<AvatarImage>,
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
    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let content_width = message_content_width(message_areas.list);

    let lines = message_viewport_lines(&messages, selected, state, content_width, &image_previews);

    frame.render_widget(Paragraph::new(lines), message_areas.list);
    for avatar in avatar_images {
        if let Some(area) =
            message_avatar_area(message_areas.list, avatar.row, avatar.visible_height)
        {
            frame.render_widget(RatatuiImage::new(&avatar.protocol), area);
        }
    }
    let mut previous_preview_rows = 0usize;
    for image_preview in image_previews.into_iter() {
        let row = inline_image_preview_row(
            &messages,
            state,
            image_preview.message_index,
            content_width,
            state.message_line_scroll(),
            previous_preview_rows,
        );
        if let Some(preview_area) =
            inline_image_preview_area(message_areas.list, row, image_preview.preview_height)
        {
            render_image_preview(frame, preview_area, image_preview.state);
        }
        previous_preview_rows =
            previous_preview_rows.saturating_add(image_preview.preview_height as usize);
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
        ImagePreviewState::ReadyCropped(protocol) => {
            frame.render_widget(RatatuiImage::new(&protocol), area);
        }
    }
}

fn preview_height_for_message(image_previews: &[ImagePreview<'_>], message_index: usize) -> u16 {
    image_previews
        .iter()
        .find(|preview| preview.message_index == message_index)
        .map(|preview| preview.preview_height)
        .unwrap_or(0)
}

fn message_viewport_lines(
    messages: &[&MessageState],
    selected: Option<usize>,
    state: &DashboardState,
    content_width: usize,
    image_previews: &[ImagePreview<'_>],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let author = message.author.clone();
        let content = format_message_content_lines(message, state, content_width.max(8));
        let preview_height = preview_height_for_message(image_previews, index);
        let line_offset = usize::from(index == 0) * state.message_line_scroll();
        let item_lines = message_item_lines(
            author,
            format_message_sent_time(message.id),
            content,
            content_width,
            preview_height,
            line_offset,
        );
        if selected == Some(index) {
            lines.extend(item_lines.into_iter().map(highlight_message_line));
        } else {
            lines.extend(item_lines);
        }
    }
    lines
}

fn message_item_lines(
    author: String,
    sent_time: String,
    content: Vec<MessageContentLine>,
    content_width: usize,
    preview_height: u16,
    line_offset: usize,
) -> Vec<Line<'static>> {
    let sent_time_width = sent_time.as_str().width();
    let author_width = content_width
        .saturating_sub(sent_time_width)
        .saturating_sub(1)
        .max(1);
    let author = truncate_display_width(&author, author_width);
    let mut lines = vec![Line::from(vec![
        message_avatar_span(),
        Span::styled(author, Style::default().fg(Color::Green).bold()),
        Span::raw(" "),
        Span::styled(sent_time, Style::default().fg(DIM)),
    ])];
    lines.extend(content.into_iter().map(|line| {
        Line::from(vec![
            message_avatar_spacer_span(),
            Span::styled(line.text, line.style),
        ])
    }));
    lines.extend(image_preview_spacer_lines(preview_height));
    lines.into_iter().skip(line_offset).collect()
}

fn message_content_width(list: Rect) -> usize {
    let padding = 4usize;
    (list.width as usize)
        .saturating_sub(padding)
        .saturating_sub(MESSAGE_AVATAR_OFFSET as usize)
        .max(8)
}

fn message_avatar_area(list: Rect, row: isize, visible_height: u16) -> Option<Rect> {
    if visible_height == 0 {
        return None;
    }

    let top = list.y as isize + row.max(0);
    let bottom = top.saturating_add(visible_height as isize);
    let list_bottom = list.y.saturating_add(list.height) as isize;
    if top >= list_bottom || bottom <= list.y as isize {
        return None;
    }

    Some(Rect {
        x: list.x,
        y: u16::try_from(top).ok()?,
        width: MESSAGE_AVATAR_PLACEHOLDER.width() as u16,
        height: visible_height,
    })
}

fn message_avatar_span() -> Span<'static> {
    Span::styled(
        format!("{MESSAGE_AVATAR_PLACEHOLDER} "),
        Style::default().fg(DIM),
    )
}

fn message_avatar_spacer_span() -> Span<'static> {
    Span::raw(" ".repeat(MESSAGE_AVATAR_OFFSET as usize))
}

fn highlight_message_line(mut line: Line<'static>) -> Line<'static> {
    for span in line.spans.iter_mut().skip(1) {
        span.style = span.style.patch(highlight_style());
    }
    line
}

fn format_message_sent_time(message_id: Id<MessageMarker>) -> String {
    let unix_millis = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
    format_unix_millis_local_time(unix_millis).unwrap_or_else(|| "--:--".to_owned())
}

fn format_unix_millis_local_time(unix_millis: u64) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&Local).format("%H:%M").to_string())
}

#[cfg(test)]
fn format_unix_millis_with_offset(unix_millis: u64, offset: chrono::FixedOffset) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&offset).format("%H:%M").to_string())
}

fn image_preview_spacer_lines(height: u16) -> Vec<Line<'static>> {
    (0..height).map(|_| Line::from("")).collect()
}

#[cfg(test)]
fn format_message_content(message: &MessageState, width: usize) -> String {
    format_message_content_lines(message, &DashboardState::new(), width)
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_message_content_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let attachment_summary =
        (!message.attachments.is_empty()).then(|| format_attachment_summary(&message.attachments));
    let mut lines = Vec::new();

    if let Some(line) = message
        .reply
        .as_ref()
        .map(|reply| format_reply_line(reply, message.guild_id, state, width))
    {
        lines.push(line);
    } else if let Some(poll) = message.poll.as_ref() {
        lines.extend(format_poll_lines(poll, width));
    } else if let Some(line) = format_message_kind_line(message.message_kind) {
        lines.push(line);
    }

    if let Some(value) = message.content.as_deref().filter(|value| !value.is_empty()) {
        lines.extend(
            wrap_text_lines(
                &state.render_user_mentions(message.guild_id, &message.mentions, value),
                width,
            )
            .into_iter()
            .map(MessageContentLine::plain),
        );
    }
    if let Some(attachments) = attachment_summary {
        lines.push(MessageContentLine::accent(truncate_text(
            &attachments,
            width,
        )));
    }
    if let Some(snapshot) = message.forwarded_snapshots.first() {
        lines.extend(format_forwarded_snapshot(snapshot, state, width));
    }

    if lines.is_empty() {
        lines.push(MessageContentLine::plain(if message.content.is_some() {
            "<empty message>".to_owned()
        } else {
            "<message content unavailable>".to_owned()
        }));
    }

    lines
}

pub(crate) fn wrapped_text_line_count(value: &str, width: usize) -> usize {
    wrap_text_lines(value, width).len()
}

fn wrap_text_lines(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    for line in value.split('\n') {
        if line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        for grapheme in line.graphemes(true) {
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width.saturating_add(grapheme_width) > width
            {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }

            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
        }
        lines.push(current);
    }
    lines
}

fn format_poll_lines(poll: &PollInfo, width: usize) -> Vec<MessageContentLine> {
    let helper = if poll.allow_multiselect {
        "Select one or more answers"
    } else {
        "Select one answer"
    };
    let mut lines = vec![
        MessageContentLine::plain(truncate_text(&poll.question, width)),
        MessageContentLine::dim(truncate_text(helper, width)),
    ];
    let total_votes = poll
        .answers
        .iter()
        .filter_map(|answer| answer.vote_count)
        .sum::<u64>();
    lines.extend(poll.answers.iter().enumerate().map(|(index, answer)| {
        MessageContentLine::plain(truncate_text(
            &format_poll_answer(index, answer, total_votes),
            width,
        ))
    }));
    lines.push(MessageContentLine::dim(truncate_text(
        &format_poll_footer(poll, total_votes),
        width,
    )));
    lines
}

fn format_poll_answer(
    index: usize,
    answer: &crate::discord::PollAnswerInfo,
    total_votes: u64,
) -> String {
    let marker = if answer.me_voted { "◉" } else { "◯" };
    let results = answer.vote_count.map(|count| {
        let percent = count
            .saturating_mul(100)
            .checked_div(total_votes)
            .unwrap_or(0);
        format!("  {count} votes  {percent}%")
    });
    match results {
        Some(results) => format!("  {marker} {}. {}{results}", index + 1, answer.text),
        None => format!("  {marker} {}. {}", index + 1, answer.text),
    }
}

fn format_poll_footer(poll: &PollInfo, total_votes: u64) -> String {
    let vote_label = if total_votes == 1 { "vote" } else { "votes" };
    match poll.results_finalized {
        Some(true) => format!("{total_votes} {vote_label} · Final results"),
        Some(false) => format!("{total_votes} {vote_label} · Results may still change"),
        None => "Use message actions to view results".to_owned(),
    }
}

fn format_reply_line(
    reply: &ReplyInfo,
    guild_id: Option<Id<GuildMarker>>,
    state: &DashboardState,
    width: usize,
) -> MessageContentLine {
    let content = reply
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or("<empty message>");
    let content = state.render_user_mentions(guild_id, &reply.mentions, content);
    MessageContentLine::dim(truncate_text(
        &format!("╭─ {} : {}", reply.author, content),
        width,
    ))
}

fn format_message_kind_line(message_kind: MessageKind) -> Option<MessageContentLine> {
    if message_kind.is_regular() {
        return None;
    }

    let label = match message_kind.code() {
        19 => "↳ Reply",
        _ => "<unsupported message type>",
    };

    Some(MessageContentLine::dim(label.to_owned()))
}

fn format_forwarded_snapshot(
    snapshot: &MessageSnapshotInfo,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let attachment_summary = (!snapshot.attachments.is_empty())
        .then(|| format_attachment_summary(&snapshot.attachments));
    let mut lines = vec![MessageContentLine::plain("↱ Forwarded".to_owned())];
    if let Some(content) = snapshot
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let content_width = width.saturating_sub(2).max(1);
        let content = state.render_user_mentions(
            state.forwarded_snapshot_mention_guild_id(snapshot),
            &snapshot.mentions,
            content,
        );
        lines.extend(
            wrap_text_lines(&content, content_width)
                .into_iter()
                .map(|line| MessageContentLine::plain(format!("│ {line}"))),
        );
    }
    if let Some(attachments) = attachment_summary {
        lines.push(MessageContentLine::accent(truncate_text(
            &format!("│ {attachments}"),
            width,
        )));
    }
    if lines.len() == 1 {
        lines.push(MessageContentLine::plain("│ <empty message>".to_owned()));
    }
    let mut metadata = Vec::new();
    if let Some(channel_id) = snapshot.source_channel_id {
        metadata.push(state.channel_label(channel_id));
    }
    if let Some(timestamp) = snapshot.timestamp.as_deref() {
        metadata.push(format_forwarded_time(timestamp));
    }
    if !metadata.is_empty() {
        lines.push(MessageContentLine::dim(truncate_text(
            &format!("│ {}", metadata.join(" · ")),
            width,
        )));
    }

    lines
}

fn format_forwarded_time(timestamp: &str) -> String {
    timestamp
        .split_once('T')
        .and_then(|(_, time)| time.get(0..5))
        .unwrap_or(timestamp)
        .to_owned()
}

fn format_attachment_summary(attachments: &[AttachmentInfo]) -> String {
    attachments
        .iter()
        .map(format_attachment)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_attachment(attachment: &AttachmentInfo) -> String {
    let kind = if attachment.is_image() {
        "image"
    } else if attachment.is_video() {
        "video"
    } else {
        "file"
    };
    let dimensions = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!(" {width}x{height}"),
        _ => String::new(),
    };

    format!("[{kind}: {}]{}", attachment.filename, dimensions)
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
        Span::styled(footer_hint(state), Style::default().fg(DIM)),
    ];
    if let Some(error) = state.last_error() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("err: {}", truncate_text(error, 60)),
            Style::default().fg(Color::Red),
        ));
    } else if let Some(status) = state.last_status() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            truncate_text(status, 72),
            Style::default().fg(Color::Green),
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

fn footer_hint(state: &DashboardState) -> &'static str {
    if state.is_message_action_menu_open() {
        "j/k choose action | enter select | esc close | q quit"
    } else {
        "tab/1-4 focus | j/k move | J/K scroll | enter/space action/tree | ←/→ close/open | i write | esc cancel | q quit"
    }
}

fn render_message_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_message_action_menu_open() {
        return;
    }

    let actions = state.selected_message_action_items();
    if actions.is_empty() {
        return;
    }

    let selected = state.selected_message_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(message_action_menu_lines(&actions, selected))
            .block(panel_block("Message actions", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn message_action_menu_lines(actions: &[MessageActionItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
            let label = if action.enabled {
                action.label.to_owned()
            } else {
                format!("{} (unavailable)", action.label)
            };
            let mut style = if action.enabled {
                Style::default()
            } else {
                Style::default().fg(DIM)
            };
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(label, style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
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

fn message_list_area(area: Rect, state: &DashboardState) -> Rect {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    message_areas(inner, state).list
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

fn inline_image_preview_width(area: Rect) -> u16 {
    area.width
        .saturating_sub(inline_image_content_offset(area))
        .min(IMAGE_PREVIEW_WIDTH)
}

fn inline_image_content_offset(area: Rect) -> u16 {
    MESSAGE_AVATAR_OFFSET.min(area.width.saturating_sub(1))
}

fn inline_image_preview_row(
    messages: &[&MessageState],
    state: &DashboardState,
    message_index: usize,
    content_width: usize,
    line_offset: usize,
    previous_preview_rows: usize,
) -> isize {
    let row = messages
        .iter()
        .take(message_index.saturating_add(1))
        .map(|message| state.message_base_line_count_for_width(message, content_width))
        .sum::<usize>()
        .saturating_add(previous_preview_rows)
        .saturating_sub(1);
    row as isize - line_offset as isize
}

fn inline_image_preview_area(list: Rect, row: isize, preview_height: u16) -> Option<Rect> {
    if preview_height == 0 {
        return None;
    }

    let content_offset = inline_image_content_offset(list);
    let desired_top = list.y as isize + row + 1;
    let desired_bottom = desired_top.saturating_add(preview_height as isize);
    let list_top = list.y as isize;
    let list_bottom = list.y.saturating_add(list.height) as isize;
    let visible_top = desired_top.max(list_top);
    let visible_bottom = desired_bottom.min(list_bottom);
    if visible_top >= visible_bottom {
        return None;
    }

    Some(Rect {
        x: list.x.saturating_add(content_offset),
        y: u16::try_from(visible_top).ok()?,
        width: list.width.saturating_sub(content_offset),
        height: u16::try_from(visible_bottom - visible_top).ok()?,
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
    wrap_text_lines(&prompt, width).len() as u16
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Rect, style::Style};
    use twilight_model::id::Id;
    use unicode_width::UnicodeWidthStr;

    use super::{
        ACCENT, DIM, DISCORD_EPOCH_MILLIS, MessageContentLine, composer_prompt_line_count,
        format_message_content, format_message_content_lines, format_message_sent_time,
        format_unix_millis_with_offset, highlight_style, inline_image_preview_area,
        inline_image_preview_row, message_action_menu_lines, message_item_lines,
        message_viewport_lines, sync_view_heights, wrap_text_lines,
    };
    use crate::{
        discord::{
            AppEvent, AttachmentInfo, ChannelInfo, MemberInfo, MentionInfo, MessageKind,
            MessageSnapshotInfo, MessageState, PollAnswerInfo, PollInfo, PresenceStatus, ReplyInfo,
        },
        tui::{
            format::truncate_display_width,
            state::{DashboardState, MessageActionItem, MessageActionKind},
        },
    };

    #[test]
    fn sync_view_heights_reserves_message_input_inside_messages_pane() {
        let mut state = DashboardState::new();

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

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

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

        assert_eq!(state.message_view_height(), 13);
    }

    #[test]
    fn composer_height_accounts_for_soft_wrapping() {
        let mut state = DashboardState::new();
        for _ in 0..100 {
            state.push_composer_char('x');
        }

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

        assert!(state.message_view_height() < 14);
    }

    #[test]
    fn composer_prompt_line_count_uses_display_width_for_wide_chars() {
        assert_eq!(composer_prompt_line_count("가나다", 4), 2);
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
    fn attachment_summary_uses_own_accent_line_after_text_content() {
        let message = message_with_attachment(Some("look".to_owned()), image_attachment());
        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["look", "[image: cat.png] 640x480"]);
        assert_eq!(lines[1].style, Style::default().fg(ACCENT));
    }

    #[test]
    fn message_content_preserves_explicit_newlines() {
        let mut message =
            message_with_attachment(Some("hello\nworld".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello", "world"]);
    }

    #[test]
    fn message_content_wraps_long_lines_to_content_width() {
        let mut message =
            message_with_attachment(Some("abcdefghijkl".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 5);

        assert_eq!(line_texts(&lines), vec!["abcde", "fghij", "kl"]);
    }

    #[test]
    fn message_content_wraps_wide_characters_by_terminal_width() {
        let mut message =
            message_with_attachment(Some("가나다라마사".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 10);

        assert_eq!(line_texts(&lines), vec!["가나다라마", "사"]);
    }

    #[test]
    fn message_content_renders_known_user_mentions() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["hello @alice"]);
    }

    #[test]
    fn message_content_keeps_unknown_user_mentions_raw() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello <@10>"]);
    }

    #[test]
    fn message_content_renders_mentions_from_message_metadata() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "alice")];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello @alice"]);
    }

    #[test]
    fn message_content_does_not_split_grapheme_clusters() {
        let lines = wrap_text_lines("👨‍👩‍👧‍👦", 7);

        assert_eq!(lines, vec!["👨‍👩‍👧‍👦".to_owned()]);
    }

    #[test]
    fn message_content_preserves_blank_lines() {
        let mut message =
            message_with_attachment(Some("one\n\nthree".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["one", "", "three"]);
    }

    #[test]
    fn video_attachment_is_labeled_as_video() {
        let message = message_with_attachment(Some(String::new()), video_attachment());

        assert_eq!(
            format_message_content(&message, 200),
            "[video: clip.mp4] 1920x1080"
        );
    }

    #[test]
    fn non_default_message_type_adds_dim_label_line() {
        let mut message =
            message_with_attachment(Some("reply body".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(19);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec!["↳ Reply", "reply body", "[image: cat.png] 640x480"]
        );
        assert_eq!(lines[0].style, Style::default().fg(DIM));
    }

    #[test]
    fn reply_message_uses_preview_instead_of_type_label() {
        let mut message = message_with_attachment(Some("asdf".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "딱구형".to_owned(),
            content: Some("잘되는군".to_owned()),
            mentions: Vec::new(),
        });

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec!["╭─ 딱구형 : 잘되는군", "asdf", "[image: cat.png] 640x480"]
        );
        assert_eq!(lines[0].style, Style::default().fg(DIM));
    }

    #[test]
    fn reply_preview_renders_known_user_mentions() {
        let mut message = message_with_attachment(Some("asdf".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "neo".to_owned(),
            content: Some("hello <@10>".to_owned()),
            mentions: Vec::new(),
        });
        message.attachments.clear();
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
    }

    #[test]
    fn reply_preview_renders_mentions_from_reply_metadata() {
        let mut message = message_with_attachment(Some("asdf".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "neo".to_owned(),
            content: Some("hello <@10>".to_owned()),
            mentions: vec![mention_info(10, "alice")],
        });
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
    }

    #[test]
    fn unsupported_message_type_uses_placeholder() {
        let mut message = message_with_attachment(Some("body".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(46);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(lines[0].text, "<unsupported message type>");
    }

    #[test]
    fn poll_message_replaces_empty_message_placeholder() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.poll = Some(poll_info(false));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec![
                "오늘 뭐 먹지?",
                "Select one answer",
                "  ◉ 1. 김치찌개  2 votes  66%",
                "  ◯ 2. 라멘  1 votes  33%",
                "3 votes · Results may still change"
            ]
        );
    }

    #[test]
    fn poll_message_notes_multiselect() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.poll = Some(poll_info(true));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(lines[1].text, "Select one or more answers");
        assert_eq!(lines[1].style, Style::default().fg(DIM));
    }

    #[test]
    fn message_action_menu_marks_selected_and_disabled_actions() {
        let actions = vec![
            MessageActionItem {
                kind: MessageActionKind::Reply,
                label: "Reply",
                enabled: true,
            },
            MessageActionItem {
                kind: MessageActionKind::DownloadImage,
                label: "Download image",
                enabled: false,
            },
        ];

        let lines = message_action_menu_lines(&actions, 1);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "  Reply",
                "› Download image (unavailable)",
                "Enter select · Esc close"
            ]
        );
    }

    #[test]
    fn forwarded_snapshot_replaces_empty_message_placeholder() {
        let message =
            message_with_forwarded_snapshot(forwarded_snapshot(Some("forwarded text"), Vec::new()));

        assert_eq!(
            format_message_content(&message, 200),
            "↱ Forwarded │ forwarded text"
        );
    }

    #[test]
    fn forwarded_snapshot_attachment_replaces_empty_message_placeholder() {
        let message =
            message_with_forwarded_snapshot(forwarded_snapshot(Some(""), vec![image_attachment()]));

        assert_eq!(
            format_message_content(&message, 200),
            "↱ Forwarded │ [image: cat.png] 640x480"
        );
    }

    #[test]
    fn forwarded_snapshot_content_appends_attachment_summary() {
        let message = message_with_forwarded_snapshot(forwarded_snapshot(
            Some("hello"),
            vec![image_attachment()],
        ));

        assert_eq!(
            format_message_content(&message, 200),
            "↱ Forwarded │ hello │ [image: cat.png] 640x480"
        );
    }

    #[test]
    fn forwarded_snapshot_content_wraps_after_prefix() {
        let message =
            message_with_forwarded_snapshot(forwarded_snapshot(Some("abcdefghijkl"), Vec::new()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 7);

        assert_eq!(
            line_texts(&lines),
            vec!["↱ Forwarded", "│ abcde", "│ fghij", "│ kl"]
        );
    }

    #[test]
    fn forwarded_snapshot_content_renders_known_user_mentions() {
        let message =
            message_with_forwarded_snapshot(forwarded_snapshot(Some("hello <@10>"), Vec::new()));
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["↱ Forwarded", "│ hello <@10>"]);
    }

    #[test]
    fn forwarded_snapshot_content_uses_source_channel_guild_for_mentions() {
        let mut snapshot = forwarded_snapshot(Some("hello <@10>"), Vec::new());
        snapshot.source_channel_id = Some(Id::new(9));
        let message = message_with_forwarded_snapshot(snapshot);
        let mut state = state_with_member(10, "outer");
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(2)),
            channel_id: Id::new(9),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "source".to_owned(),
            kind: "GuildText".to_owned(),
        }));
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(2),
            member: member_info(10, "source"),
        });

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(
            line_texts(&lines),
            vec!["↱ Forwarded", "│ hello @source", "│ #source"]
        );
    }

    #[test]
    fn forwarded_snapshot_content_renders_mentions_from_snapshot_metadata() {
        let mut snapshot = forwarded_snapshot(Some("hello <@10>"), Vec::new());
        snapshot.mentions = vec![mention_info(10, "alice")];
        let message = message_with_forwarded_snapshot(snapshot);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["↱ Forwarded", "│ hello @alice"]);
    }

    #[test]
    fn forwarded_snapshot_content_wraps_wide_characters_after_prefix() {
        let message =
            message_with_forwarded_snapshot(forwarded_snapshot(Some("가나다라마사"), Vec::new()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 12);

        assert_eq!(
            line_texts(&lines),
            vec!["↱ Forwarded", "│ 가나다라마", "│ 사"]
        );
    }

    #[test]
    fn forwarded_snapshot_lines_include_channel_and_time() {
        let mut state = DashboardState::new();
        state.push_event(crate::discord::AppEvent::ChannelUpsert(
            crate::discord::ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(9),
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            },
        ));
        let mut snapshot = forwarded_snapshot(Some("hello"), Vec::new());
        snapshot.source_channel_id = Some(Id::new(9));
        snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
        let message = message_with_forwarded_snapshot(snapshot);

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(
            line_texts(&lines),
            vec!["↱ Forwarded", "│ hello", "│ #general · 12:34"]
        );
        assert_eq!(lines[2].style, Style::default().fg(DIM));
    }

    #[test]
    fn image_preview_rows_are_part_of_the_message_item() {
        let lines = message_item_lines(
            "neo".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("look".to_owned())],
            14,
            3,
            0,
        );

        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn text_only_message_item_has_header_and_content_rows() {
        let lines = message_item_lines(
            "neo".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("look".to_owned())],
            14,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   look"]
        );
    }

    #[test]
    fn message_item_lines_can_start_after_line_offset() {
        let lines = message_item_lines(
            "neo".to_owned(),
            "00:00".to_owned(),
            vec![
                MessageContentLine::plain("first".to_owned()),
                MessageContentLine::plain("second".to_owned()),
                MessageContentLine::plain("third".to_owned()),
            ],
            14,
            0,
            2,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["   second", "   third"]
        );
    }

    #[test]
    fn message_item_header_uses_display_width_for_korean_author() {
        let ascii = message_item_lines(
            "bruised8".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("난다".to_owned())],
            14,
            0,
            0,
        );
        let korean = message_item_lines(
            "장방이".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain(
                "그리고 그 티비 대가리도".to_owned(),
            )],
            14,
            0,
            0,
        );

        assert_eq!(line_texts_from_ratatui(&ascii)[0], "oo bruised8 00:00");
        assert_eq!(line_texts_from_ratatui(&korean)[0], "oo 장방이 00:00");
    }

    #[test]
    fn shared_truncation_uses_display_width_for_wide_characters() {
        let author = truncate_display_width("가나다라마바사아자", 8);

        assert_eq!(author, "가나...");
        assert_eq!(author.width(), 7);
    }

    #[test]
    fn message_sent_time_formats_with_timezone_offset() {
        let kst = chrono::FixedOffset::east_opt(9 * 60 * 60).expect("KST offset should be valid");

        assert_eq!(
            format_unix_millis_with_offset(DISCORD_EPOCH_MILLIS, kst),
            Some("09:00".to_owned())
        );
    }

    #[test]
    fn message_viewport_lines_keep_rows_from_tall_following_message() {
        let mut selected = message_with_attachment(Some("selected".to_owned()), image_attachment());
        selected.attachments.clear();
        let mut tall_following = message_with_attachment(
            Some("abcdefghijklmnopqrstuvwx".to_owned()),
            image_attachment(),
        );
        tall_following.attachments.clear();
        let messages = [&selected, &tall_following];

        let visible_rows =
            message_viewport_lines(&messages, Some(0), &DashboardState::new(), 5, &[])
                .into_iter()
                .take(4)
                .collect::<Vec<_>>();
        let visible_text = line_texts_from_ratatui(&visible_rows);
        let sent_time = format_message_sent_time(Id::new(1));

        assert!(visible_text[0].starts_with("oo "));
        assert!(visible_text[0].ends_with(&sent_time));
        assert!(visible_text[1].ends_with("selected"));
        assert!(visible_text[2].starts_with("oo "));
        assert!(visible_text[2].ends_with(&sent_time));
        assert!(visible_text[3].ends_with("abcdefgh"));
    }

    #[test]
    fn selected_message_highlight_skips_avatar_column() {
        let mut message =
            message_with_attachment(Some("abcdefghijkl".to_owned()), image_attachment());
        message.attachments.clear();
        let messages = [&message];

        let lines = message_viewport_lines(&messages, Some(0), &DashboardState::new(), 5, &[]);
        let sent_time = format_message_sent_time(Id::new(1));

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                format!("oo . {sent_time}"),
                "   abcdefgh".to_owned(),
                "   ijkl".to_owned(),
            ]
        );
        assert_eq!(lines[0].spans[0].style.bg, None);
        assert_eq!(lines[1].spans[0].style.bg, None);
        assert_eq!(lines[1].spans[1].style.bg, highlight_style().bg);
    }

    #[test]
    fn message_preview_rows_do_not_shrink_message_viewport() {
        let mut state = DashboardState::new();

        sync_view_heights(Rect::new(0, 0, 100, 20), &mut state);

        assert_eq!(state.message_view_height(), 14);
    }

    #[test]
    fn inline_image_preview_slot_follows_image_message_content() {
        let area = Rect::new(10, 5, 80, 12);

        assert_eq!(
            inline_image_preview_area(area, 2, 4),
            Some(Rect::new(13, 8, 77, 4))
        );
    }

    #[test]
    fn later_image_preview_slot_accounts_for_prior_preview_rows() {
        let area = Rect::new(10, 5, 80, 18);
        let messages = [
            message_with_attachment(Some("one".to_owned()), image_attachment()),
            message_with_attachment(Some("two".to_owned()), image_attachment()),
            message_with_attachment(Some("three".to_owned()), image_attachment()),
        ];
        let messages = messages.iter().collect::<Vec<_>>();
        let state = DashboardState::new();
        let row = inline_image_preview_row(&messages, &state, 2, 200, 0, 4);

        assert_eq!(row, 12);
        assert_eq!(
            inline_image_preview_area(area, row, 4),
            Some(Rect::new(13, 18, 77, 4))
        );
    }

    #[test]
    fn forwarded_card_rows_push_inline_preview_slot_down() {
        let mut snapshot = forwarded_snapshot(Some("hello"), vec![image_attachment()]);
        snapshot.source_channel_id = Some(Id::new(9));
        snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
        let message = message_with_forwarded_snapshot(snapshot);
        let messages = [&message];
        let state = DashboardState::new();

        assert_eq!(inline_image_preview_row(&messages, &state, 0, 200, 0, 0), 4);
    }

    #[test]
    fn inline_image_preview_area_hides_preview_at_list_bottom() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(
            inline_image_preview_area(area, 3, 4),
            Some(Rect::new(13, 9, 77, 2))
        );
    }

    #[test]
    fn inline_image_preview_area_clips_preview_at_list_top() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(
            inline_image_preview_area(area, -2, 4),
            Some(Rect::new(13, 5, 77, 3))
        );
    }

    #[test]
    fn inline_image_preview_area_returns_none_when_preview_starts_below_list() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(inline_image_preview_area(area, 5, 4), None);
    }

    #[test]
    fn inline_image_preview_area_returns_none_when_preview_ends_above_list() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(inline_image_preview_area(area, -5, 4), None);
    }

    fn message_with_attachment(
        content: Option<String>,
        attachment: AttachmentInfo,
    ) -> MessageState {
        MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            poll: None,
            content,
            mentions: Vec::new(),
            attachments: vec![attachment],
            forwarded_snapshots: Vec::new(),
        }
    }

    fn message_with_forwarded_snapshot(snapshot: MessageSnapshotInfo) -> MessageState {
        MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![snapshot],
        }
    }

    fn poll_info(allow_multiselect: bool) -> PollInfo {
        PollInfo {
            question: "오늘 뭐 먹지?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "김치찌개".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "라멘".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect,
            results_finalized: Some(false),
        }
    }

    fn forwarded_snapshot(
        content: Option<&str>,
        attachments: Vec<AttachmentInfo>,
    ) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: content.map(str::to_owned),
            mentions: Vec::new(),
            attachments,
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn state_with_member(user_id: u64, display_name: &str) -> DashboardState {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GuildCreate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            channels: Vec::new(),
            members: vec![member_info(user_id, display_name)],
            presences: vec![(Id::new(user_id), PresenceStatus::Online)],
        });
        state
    }

    fn member_info(user_id: u64, display_name: &str) -> MemberInfo {
        MemberInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
            is_bot: false,
            avatar_url: None,
        }
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
        }
    }

    fn line_texts(lines: &[MessageContentLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    fn line_texts_from_ratatui(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
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

    fn video_attachment() -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(4),
            filename: "clip.mp4".to_owned(),
            url: "https://cdn.discordapp.com/clip.mp4".to_owned(),
            proxy_url: "https://media.discordapp.net/clip.mp4".to_owned(),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }
}
