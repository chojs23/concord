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
    format::{RenderedText, TextHighlight, truncate_display_width, truncate_text},
    state::{
        ChannelPaneEntry, DashboardState, EmojiReactionItem, FocusPane, GuildPaneEntry,
        MemberEntry, MemberGroup, MessageActionItem, PollVotePickerItem, ThreadSummary,
        folder_color, presence_color, presence_marker,
    },
};
use crate::discord::{
    AttachmentInfo, ChannelState, MessageKind, MessageSnapshotInfo, MessageState, PollInfo,
    PresenceStatus, ReactionEmoji, ReactionInfo, ReactionUsersInfo, ReplyInfo,
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
const MAX_EMOJI_REACTION_VISIBLE_ITEMS: usize = 10;
const MAX_REACTION_USERS_VISIBLE_LINES: usize = 14;
const EMOJI_REACTION_IMAGE_WIDTH: u16 = 2;

#[derive(Clone)]
struct MessageContentLine {
    text: String,
    style: Style,
    mention_highlights: Vec<TextHighlight>,
}

impl MessageContentLine {
    fn plain(text: String) -> Self {
        Self {
            text,
            style: Style::default(),
            mention_highlights: Vec::new(),
        }
    }

    fn styled_text(text: String, style: Style, mention_highlights: Vec<TextHighlight>) -> Self {
        Self {
            text,
            style,
            mention_highlights,
        }
    }

    fn dim(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(DIM),
            mention_highlights: Vec::new(),
        }
    }

    fn accent(text: String) -> Self {
        Self {
            text,
            style: Style::default().fg(ACCENT),
            mention_highlights: Vec::new(),
        }
    }

    fn spans(&self) -> Vec<Span<'static>> {
        if self.mention_highlights.is_empty() {
            return vec![Span::styled(self.text.clone(), self.style)];
        }

        let mut spans = Vec::new();
        let mut cursor = 0usize;
        for highlight in &self.mention_highlights {
            if highlight.start > cursor {
                spans.push(Span::styled(
                    self.text[cursor..highlight.start].to_owned(),
                    self.style,
                ));
            }
            spans.push(Span::styled(
                self.text[highlight.start..highlight.end].to_owned(),
                self.style.patch(mention_highlight_style()),
            ));
            cursor = highlight.end;
        }
        if cursor < self.text.len() {
            spans.push(Span::styled(self.text[cursor..].to_owned(), self.style));
        }
        spans
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

pub struct EmojiReactionImage<'a> {
    pub url: String,
    pub protocol: &'a ratatui_image::protocol::Protocol,
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
    state.set_reaction_users_popup_view_height(reaction_users_visible_line_count(areas.messages));
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
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    let areas = dashboard_areas(frame.area());

    render_guilds(frame, areas.guilds, state);
    render_channels(frame, areas.channels, state);
    render_messages(
        frame,
        areas.messages,
        state,
        image_previews,
        avatar_images,
        &emoji_images,
    );
    render_members(frame, areas.members, state);
    render_footer(frame, areas.footer, state);
    render_message_action_menu(frame, areas.messages, state);
    render_poll_vote_picker(frame, areas.messages, state);
    render_emoji_reaction_picker(frame, areas.messages, state, emoji_images);
    render_reaction_users_popup(frame, areas.messages, state);
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
                                channel_name_style(state, is_active),
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
    emoji_images: &[EmojiReactionImage<'_>],
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
    render_inline_reaction_emojis(
        frame,
        message_areas.list,
        &messages,
        state,
        content_width,
        &image_previews,
        emoji_images,
    );
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

/// Walks visible messages in render order, computes the absolute (row, col) of
/// each custom-emoji reaction image overlay, and paints it on top of the
/// already-rendered message text. Mirrors `visible_avatar_targets`'s row
/// accounting in `mod.rs`: each message contributes
/// `base_lines + preview_height` rendered rows, with the first message
/// optionally clipped at the top by `state.message_line_scroll()`.
fn render_inline_reaction_emojis(
    frame: &mut Frame,
    list: Rect,
    messages: &[&MessageState],
    state: &DashboardState,
    content_width: usize,
    image_previews: &[ImagePreview<'_>],
    emoji_images: &[EmojiReactionImage<'_>],
) {
    if emoji_images.is_empty() || list.height == 0 || list.width <= MESSAGE_AVATAR_OFFSET {
        return;
    }

    let list_top = list.y as isize;
    let list_bottom = list_top + list.height as isize;
    let list_left = list.x as isize;
    let list_right = list_left + list.width as isize;
    let avatar_offset = MESSAGE_AVATAR_OFFSET as isize;

    let mut rendered_rows: isize = 0;

    for (index, message) in messages.iter().enumerate() {
        if rendered_rows >= list.height as isize {
            break;
        }
        let line_offset = if index == 0 {
            state.message_line_scroll() as isize
        } else {
            0
        };
        let base_rows = state.message_base_line_count_for_width(message, content_width) as isize;
        let preview_height = preview_height_for_message(image_previews, index) as isize;

        let layout = lay_out_reaction_chips(&message.reactions, content_width);
        if !layout.slots.is_empty() {
            // Reactions live in the last `layout.lines.len()` rows of the
            // message's base content (header + body), before the preview
            // spacer. Their first row is therefore at:
            //     message_top + (base_rows - reaction_lines)
            let message_top = rendered_rows - line_offset;
            let reaction_strip_top =
                message_top + base_rows.saturating_sub(layout.lines.len() as isize);

            for slot in layout.slots {
                let row_in_list = reaction_strip_top + slot.line as isize;
                if row_in_list < 0 || row_in_list >= list.height as isize {
                    continue;
                }
                let Some(image) = emoji_images.iter().find(|img| img.url == slot.url) else {
                    continue;
                };
                let absolute_row = list_top + row_in_list;
                let absolute_col = list_left + avatar_offset + slot.col as isize;
                if absolute_col >= list_right {
                    continue;
                }
                let max_width = (list_right - absolute_col).max(0) as u16;
                let image_width = EMOJI_REACTION_IMAGE_WIDTH.min(max_width);
                if image_width == 0 {
                    continue;
                }
                let image_area = Rect {
                    x: absolute_col as u16,
                    y: absolute_row as u16,
                    width: image_width,
                    height: 1,
                };
                if image_area.y >= list_bottom as u16 {
                    continue;
                }
                frame.render_widget(RatatuiImage::new(image.protocol), image_area);
            }
        }

        rendered_rows = rendered_rows.saturating_add((base_rows + preview_height) - line_offset);
    }
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
        let mut spans = vec![message_avatar_spacer_span()];
        spans.extend(line.spans());
        Line::from(spans)
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

    if let Some(system_lines) = format_system_message_lines(message, state, width) {
        return system_lines;
    }

    let renders_poll_card = message.reply.is_none() && message.poll.is_some();

    if let Some(line) = message
        .reply
        .as_ref()
        .map(|reply| format_reply_line(reply, message.guild_id, state, width))
    {
        lines.push(line);
    } else if let Some(poll) = message.poll.as_ref() {
        let content = message
            .content
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| {
                state.render_user_mentions_with_highlights(
                    message.guild_id,
                    &message.mentions,
                    value,
                )
            });
        lines.extend(format_poll_lines(poll, content, width));
    } else if let Some(line) = format_message_kind_line(message.message_kind) {
        lines.push(line);
    }

    let standalone_content = (!renders_poll_card)
        .then(|| message.content.as_deref().filter(|value| !value.is_empty()))
        .flatten();
    if let Some(value) = standalone_content {
        lines.extend(wrap_rendered_text_lines(
            state.render_user_mentions_with_highlights(message.guild_id, &message.mentions, value),
            width,
            Style::default(),
        ));
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
    if !message.reactions.is_empty() {
        lines.extend(format_reaction_lines(&message.reactions, width));
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

fn format_reaction_lines(reactions: &[ReactionInfo], width: usize) -> Vec<MessageContentLine> {
    lay_out_reaction_chips(reactions, width)
        .lines
        .into_iter()
        .map(MessageContentLine::accent)
        .collect()
}

/// Position of a custom-emoji image overlay relative to the start of a
/// message's reaction strip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReactionImageSlot {
    pub(crate) line: u16,
    pub(crate) col: u16,
    pub(crate) url: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReactionLayout {
    pub(crate) lines: Vec<String>,
    pub(crate) slots: Vec<ReactionImageSlot>,
}

/// Builds a single chip's text plus the chip-internal column offset where its
/// image overlay should land (if any). Custom-emoji chips reserve a fixed
/// `EMOJI_REACTION_IMAGE_WIDTH` of spaces in place of the textual `:name:`
/// label so that loading the image later does not reflow the row.
fn build_reaction_chip(reaction: &ReactionInfo) -> (String, Option<usize>, Option<String>) {
    let count = reaction.count;
    match &reaction.emoji {
        ReactionEmoji::Unicode(emoji) => {
            let chip = if reaction.me {
                format!("[● {emoji} {count}]")
            } else {
                format!("[{emoji} {count}]")
            };
            (chip, None, None)
        }
        ReactionEmoji::Custom { .. } => {
            let url = reaction.emoji.custom_image_url();
            let placeholder = " ".repeat(EMOJI_REACTION_IMAGE_WIDTH as usize);
            let prefix = if reaction.me { "[● " } else { "[" };
            let chip = format!("{prefix}{placeholder} {count}]");
            let image_offset = prefix.width();
            (chip, Some(image_offset), url)
        }
    }
}

/// Lays out reaction chips for a message, wrapping at chip boundaries so a
/// chip is never split across rows. Returns both the rendered text rows and
/// the absolute (line, col) position of every custom-emoji image overlay,
/// relative to the first reaction row.
pub(crate) fn lay_out_reaction_chips(
    reactions: &[ReactionInfo],
    width: usize,
) -> ReactionLayout {
    let width = width.max(1);
    let chips: Vec<(String, Option<usize>, Option<String>)> = reactions
        .iter()
        .filter(|reaction| reaction.count > 0)
        .map(build_reaction_chip)
        .collect();
    if chips.is_empty() {
        return ReactionLayout::default();
    }

    let mut lines: Vec<String> = Vec::new();
    let mut slots: Vec<ReactionImageSlot> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for (chip_text, image_offset, url) in chips {
        let chip_width = chip_text.width();
        let separator_width = if current_width == 0 { 0 } else { 2 };
        let projected = current_width + separator_width + chip_width;
        let needs_wrap = current_width > 0 && projected > width;
        if needs_wrap {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }

        let chip_start_col = if current_width == 0 {
            0usize
        } else {
            current.push_str("  ");
            current_width += 2;
            current_width
        };
        current.push_str(&chip_text);
        current_width += chip_width;

        if let (Some(offset), Some(url)) = (image_offset, url) {
            slots.push(ReactionImageSlot {
                line: u16::try_from(lines.len()).unwrap_or(u16::MAX),
                col: u16::try_from(chip_start_col + offset).unwrap_or(u16::MAX),
                url,
            });
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    ReactionLayout { lines, slots }
}

pub(crate) fn wrapped_text_line_count(value: &str, width: usize) -> usize {
    wrap_text_lines(value, width).len()
}

fn wrap_rendered_text_lines(
    rendered: RenderedText,
    width: usize,
    style: Style,
) -> Vec<MessageContentLine> {
    wrap_text_with_highlights(&rendered.text, &rendered.highlights, width)
        .into_iter()
        .map(|(text, mention_highlights)| {
            MessageContentLine::styled_text(text, style, mention_highlights)
        })
        .collect()
}

fn rendered_text_line(rendered: RenderedText, style: Style) -> MessageContentLine {
    MessageContentLine::styled_text(rendered.text, style, rendered.highlights)
}

fn prepend_rendered_text(prefix: String, mut rendered: RenderedText) -> RenderedText {
    let shift = prefix.len();
    for highlight in &mut rendered.highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    rendered.text.insert_str(0, &prefix);
    rendered
}

fn truncate_rendered_text(rendered: RenderedText, limit: usize) -> RenderedText {
    let mut chars = rendered.text.char_indices();
    let cutoff = match chars.nth(limit) {
        Some((index, _)) => index,
        None => return rendered,
    };
    let mut text = rendered.text[..cutoff].to_owned();
    text.push_str("...");
    let highlights = rendered
        .highlights
        .into_iter()
        .filter(|highlight| highlight.start < cutoff)
        .map(|highlight| TextHighlight {
            start: highlight.start,
            end: highlight.end.min(cutoff),
        })
        .collect();
    RenderedText { text, highlights }
}

fn prefix_message_content_line(prefix: &str, mut line: MessageContentLine) -> MessageContentLine {
    let shift = prefix.len();
    for highlight in &mut line.mention_highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    line.text.insert_str(0, prefix);
    line
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

fn wrap_text_with_highlights(
    value: &str,
    highlights: &[TextHighlight],
    width: usize,
) -> Vec<(String, Vec<TextHighlight>)> {
    if value.is_empty() {
        return Vec::new();
    }

    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    for line in value.split('\n') {
        if line.is_empty() {
            lines.push((String::new(), Vec::new()));
            line_start = line_start.saturating_add(1);
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        let mut current_start = line_start;
        let mut current_end = line_start;
        for (relative_start, grapheme) in line.grapheme_indices(true) {
            let grapheme_start = line_start.saturating_add(relative_start);
            let grapheme_end = grapheme_start.saturating_add(grapheme.len());
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width.saturating_add(grapheme_width) > width
            {
                let text = std::mem::take(&mut current);
                lines.push((
                    text,
                    highlights_for_range(highlights, current_start, current_end),
                ));
                current_width = 0;
                current_start = grapheme_start;
            }

            current.push_str(grapheme);
            current_width = current_width.saturating_add(grapheme_width);
            current_end = grapheme_end;
        }
        lines.push((
            current,
            highlights_for_range(highlights, current_start, current_end),
        ));
        line_start = line_start.saturating_add(line.len()).saturating_add(1);
    }
    lines
}

fn highlights_for_range(
    highlights: &[TextHighlight],
    start: usize,
    end: usize,
) -> Vec<TextHighlight> {
    highlights
        .iter()
        .filter_map(|highlight| {
            let highlight_start = highlight.start.max(start);
            let highlight_end = highlight.end.min(end);
            (highlight_start < highlight_end).then(|| TextHighlight {
                start: highlight_start.saturating_sub(start),
                end: highlight_end.saturating_sub(start),
            })
        })
        .collect()
}

fn format_poll_lines(
    poll: &PollInfo,
    content: Option<RenderedText>,
    width: usize,
) -> Vec<MessageContentLine> {
    let inner_width = poll_card_inner_width(width);
    let helper = if poll.allow_multiselect {
        "Select one or more answers"
    } else {
        "Select one answer"
    };
    let mut lines = vec![MessageContentLine::accent(poll_box_border('╭', '╮', width))];
    lines.push(poll_box_line(
        MessageContentLine::plain(truncate_display_width(&poll.question, inner_width)),
        inner_width,
    ));
    if let Some(content) = content {
        lines.extend(
            wrap_rendered_text_lines(content, inner_width, Style::default())
                .into_iter()
                .map(|line| poll_box_line(line, inner_width)),
        );
    }
    lines.push(poll_box_line(
        MessageContentLine::dim(truncate_display_width(helper, inner_width)),
        inner_width,
    ));
    let counted_votes = poll
        .answers
        .iter()
        .filter_map(|answer| answer.vote_count)
        .sum::<u64>();
    let total_votes = poll.total_votes.unwrap_or(counted_votes);
    lines.extend(poll.answers.iter().enumerate().map(|(index, answer)| {
        poll_box_line(
            MessageContentLine::plain(truncate_display_width(
                &format_poll_answer(index, answer, total_votes),
                inner_width,
            )),
            inner_width,
        )
    }));
    lines.push(poll_box_line(
        MessageContentLine::dim(truncate_display_width(
            &format_poll_footer(poll, total_votes),
            inner_width,
        )),
        inner_width,
    ));
    lines.push(MessageContentLine::accent(poll_box_border('╰', '╯', width)));
    lines
}

pub(crate) fn poll_card_inner_width(width: usize) -> usize {
    poll_box_width(width).saturating_sub(4).max(1)
}

fn poll_box_width(width: usize) -> usize {
    width.clamp(4, 72)
}

fn poll_box_border(left: char, right: char, width: usize) -> String {
    let width = poll_box_width(width);
    format!("{left}{}{right}", "─".repeat(width.saturating_sub(2)))
}

fn poll_box_line(mut line: MessageContentLine, inner_width: usize) -> MessageContentLine {
    let prefix = "│ ";
    let suffix = " │";
    let padding = inner_width.saturating_sub(line.text.width());
    let shift = prefix.len();
    for highlight in &mut line.mention_highlights {
        highlight.start = highlight.start.saturating_add(shift);
        highlight.end = highlight.end.saturating_add(shift);
    }
    line.text = format!("{prefix}{}{}{suffix}", line.text, " ".repeat(padding));
    line
}

fn format_poll_result_lines(poll: Option<&PollInfo>, width: usize) -> Vec<MessageContentLine> {
    let Some(poll) = poll else {
        return vec![
            MessageContentLine::accent(truncate_text("Poll results", width)),
            MessageContentLine::dim(truncate_text("Result details unavailable", width)),
        ];
    };
    let mut lines = vec![
        MessageContentLine::accent(truncate_text("Poll results", width)),
        MessageContentLine::plain(truncate_text(&poll.question, width)),
    ];
    if let Some(winner) = poll.answers.first() {
        let votes = winner
            .vote_count
            .map(|count| format!(" with {count} votes"))
            .unwrap_or_default();
        lines.push(MessageContentLine::plain(truncate_text(
            &format!("Winner: {}{votes}", winner.text),
            width,
        )));
    } else {
        lines.push(MessageContentLine::dim(truncate_text(
            "No winning answer recorded",
            width,
        )));
    }
    let counted_votes = poll
        .answers
        .iter()
        .filter_map(|answer| answer.vote_count)
        .sum::<u64>();
    let total_votes = poll
        .total_votes
        .or_else(|| (counted_votes > 0).then_some(counted_votes));
    if let Some(total_votes) = total_votes {
        let vote_label = if total_votes == 1 { "vote" } else { "votes" };
        lines.push(MessageContentLine::dim(truncate_text(
            &format!("{total_votes} total {vote_label} · Final results"),
            width,
        )));
    }
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
        None => "Results not available yet".to_owned(),
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
    let content = state.render_user_mentions_with_highlights(guild_id, &reply.mentions, content);
    let content = prepend_rendered_text(format!("╭─ {} : ", reply.author), content);
    rendered_text_line(
        truncate_rendered_text(content, width),
        Style::default().fg(DIM),
    )
}

fn format_message_kind_line(message_kind: MessageKind) -> Option<MessageContentLine> {
    if message_kind.is_regular() {
        return None;
    }

    let label = match message_kind.code() {
        7 => "joined the server",
        19 => "↳ Reply",
        _ => message_kind
            .known_label()
            .unwrap_or("<unsupported message type>"),
    };

    Some(MessageContentLine::dim(label.to_owned()))
}

fn format_system_message_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Option<Vec<MessageContentLine>> {
    match message.message_kind.code() {
        8 => Some(vec![MessageContentLine::accent(truncate_text(
            &format!("{} boosted the server", message.author),
            width,
        ))]),
        9..=11 => {
            let tier = message.message_kind.code() - 8;
            Some(vec![MessageContentLine::accent(truncate_text(
                &format!("{} boosted the server to Level {tier}", message.author),
                width,
            ))])
        }
        18 => Some(format_thread_created_lines(message, state, width)),
        21 => Some(format_thread_starter_lines(message, state, width)),
        46 => Some(format_poll_result_lines(message.poll.as_ref(), width)),
        _ => None,
    }
}

fn format_thread_created_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let summary = state.thread_summary_for_message(message);
    let thread_name = summary
        .as_ref()
        .map(|summary| summary.name.as_str())
        .or_else(|| message.content.as_deref().filter(|value| !value.is_empty()))
        .unwrap_or("thread");
    let mut lines = vec![
        MessageContentLine::accent(truncate_text(
            &format!("{} started a thread", message.author),
            width,
        )),
        MessageContentLine::plain(truncate_text(&format!("# {thread_name}"), width)),
    ];
    if let Some(summary) = summary {
        lines.push(format_thread_summary_line(&summary, width));
    } else {
        lines.push(MessageContentLine::dim(truncate_text(
            "Thread details unavailable",
            width,
        )));
    }
    lines
}

fn format_thread_summary_line(summary: &ThreadSummary, width: usize) -> MessageContentLine {
    let mut parts = Vec::new();
    if let Some(count) = summary.message_count.or(summary.total_message_sent) {
        let label = if count == 1 { "message" } else { "messages" };
        parts.push(format!("{count} {label}"));
    }
    if summary.archived == Some(true) {
        parts.push("archived".to_owned());
    }
    if summary.locked == Some(true) {
        parts.push("locked".to_owned());
    }
    parts.push("Open thread to view messages".to_owned());
    MessageContentLine::dim(truncate_text(&parts.join(" · "), width))
}

fn format_thread_starter_lines(
    message: &MessageState,
    state: &DashboardState,
    width: usize,
) -> Vec<MessageContentLine> {
    let mut lines = vec![MessageContentLine::accent(truncate_text(
        "Thread starter message",
        width,
    ))];
    if let Some(reply) = message.reply.as_ref() {
        lines.push(format_reply_line(reply, message.guild_id, state, width));
    } else {
        lines.push(MessageContentLine::dim(truncate_text(
            "Started from an unavailable message",
            width,
        )));
    }
    lines
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
        let content = state.render_user_mentions_with_highlights(
            state.forwarded_snapshot_mention_guild_id(snapshot),
            &snapshot.mentions,
            content,
        );
        lines.extend(
            wrap_rendered_text_lines(content, content_width, Style::default())
                .into_iter()
                .map(|line| prefix_message_content_line("│ ", line)),
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
    let prompt = composer_lines(state, area.width);

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

fn composer_lines(state: &DashboardState, width: u16) -> Vec<Line<'static>> {
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

fn composer_text(state: &DashboardState, width: u16) -> String {
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
            let member = *member;
            let is_selected = focused && selected_line == Some(line_index);
            let marker_style = Style::default().fg(presence_color(member.status()));
            let mut name_style = Style::default().fg(presence_color(member.status()));
            if member.is_bot() {
                name_style = name_style.add_modifier(Modifier::ITALIC);
            }
            if is_selected {
                name_style = name_style
                    .bg(Color::Rgb(24, 54, 65))
                    .add_modifier(Modifier::BOLD);
            }

            let display = member_display_label(member, max_name_width);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", presence_marker(member.status())),
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
            group.label.clone(),
            Style::default()
                .fg(discord_role_color(group.color))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", group.entries.len()),
            Style::default().fg(DIM),
        ),
    ])
}

fn member_display_label(member: MemberEntry<'_>, max_width: usize) -> String {
    let display_name = member.display_name();
    if !member.is_bot() {
        return truncate_display_width(&display_name, max_width);
    }

    const BOT_SUFFIX: &str = " [bot]";
    let suffix_width = BOT_SUFFIX.width();
    if max_width <= suffix_width {
        return truncate_display_width(&format!("{}{}", display_name, BOT_SUFFIX), max_width);
    }

    format!(
        "{}{}",
        truncate_display_width(&display_name, max_width.saturating_sub(suffix_width)),
        BOT_SUFFIX
    )
}

fn discord_role_color(color: Option<u32>) -> Color {
    match color {
        Some(value) if value != 0 => {
            let r = ((value >> 16) & 0xFF) as u8;
            let g = ((value >> 8) & 0xFF) as u8;
            let b = (value & 0xFF) as u8;
            Color::Rgb(r, g, b)
        }
        _ => DIM,
    }
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
    if state.is_reaction_users_popup_open() {
        "esc close reacted users"
    } else if state.is_poll_vote_picker_open() {
        "j/k choose answer | space toggle | enter vote | esc close"
    } else if state.is_emoji_reaction_picker_open() {
        "j/k choose emoji | enter/space react | esc close"
    } else if state.is_message_action_menu_open() {
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

fn render_emoji_reaction_picker(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    if !state.is_emoji_reaction_picker_open() {
        return;
    }

    let reactions = state.emoji_reaction_items();
    if reactions.is_empty() {
        return;
    }

    let selected = state.selected_emoji_reaction_index().unwrap_or(0);
    let visible_items = reactions.len().clamp(1, MAX_EMOJI_REACTION_VISIBLE_ITEMS);
    let popup = centered_rect(area, 42, (visible_items as u16).saturating_add(4));
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(emoji_reaction_picker_lines(
            &reactions,
            selected,
            visible_items,
            &ready_urls,
        ))
        .block(block)
        .wrap(Wrap { trim: false }),
        popup,
    );
    render_emoji_reaction_images(
        frame,
        content,
        &reactions,
        selected,
        visible_items,
        emoji_images,
    );
}

fn render_poll_vote_picker(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(answers) = state.poll_vote_picker_items() else {
        return;
    };
    if answers.is_empty() {
        return;
    }

    let selected = state.selected_poll_vote_picker_index().unwrap_or(0);
    let popup = centered_rect(area, 58, (answers.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(poll_vote_picker_lines(answers, selected))
            .block(panel_block("Choose poll votes", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_reaction_users_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    let Some(popup_state) = state.reaction_users_popup() else {
        return;
    };

    // Compute the popup's eventual inner width up front so we can pre-truncate
    // every line to fit. Without this, ratatui's `Wrap` would split a long
    // username across rows and the wrap continuation overlaps neighbouring
    // lines, producing the trailing-fragment artefact reported by users.
    const POPUP_TARGET_WIDTH: u16 = 58;
    let popup_width = POPUP_TARGET_WIDTH
        .min(area.width.saturating_sub(2))
        .max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));

    let max_visible_lines = reaction_users_visible_line_count(area);
    let lines = reaction_users_popup_lines(
        popup_state.reactions(),
        popup_state.scroll(),
        max_visible_lines,
        inner_width,
    );
    let popup = centered_rect(area, POPUP_TARGET_WIDTH, (lines.len() as u16).saturating_add(2));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block("Reacted users", true)),
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

fn reaction_users_visible_line_count(area: Rect) -> usize {
    usize::from(area.height)
        .saturating_sub(5)
        .min(MAX_REACTION_USERS_VISIBLE_LINES)
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

fn poll_vote_picker_lines(answers: &[PollVotePickerItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let marker = if index == selected { "› " } else { "  " };
            let checkbox = if answer.selected { "[x]" } else { "[ ]" };
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{checkbox} {}", answer.label), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Space toggle · Enter vote · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn reaction_users_popup_lines(
    reactions: &[ReactionUsersInfo],
    scroll: usize,
    max_visible_lines: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let data_lines = reaction_users_popup_data_lines(reactions);
    let visible_lines = max_visible_lines.min(data_lines.len());
    let scroll = scroll.min(data_lines.len().saturating_sub(visible_lines));
    let has_hidden_before = scroll > 0;
    let has_hidden_after = scroll.saturating_add(visible_lines) < data_lines.len();
    let mut lines: Vec<Line<'static>> = data_lines
        .into_iter()
        .skip(scroll)
        .take(visible_lines)
        .map(|line| truncate_line_to_display_width(line, inner_width))
        .collect();
    let hint = match (has_hidden_before, has_hidden_after) {
        (true, true) => "j/k scroll · more above/below · Esc close",
        (true, false) => "j/k scroll · more above · Esc close",
        (false, true) => "j/k scroll · more below · Esc close",
        (false, false) => "Esc close",
    };
    lines.push(truncate_line_to_display_width(
        Line::from(Span::styled(hint, Style::default().fg(DIM))),
        inner_width,
    ));
    lines
}

/// Clamps the visible width of a `Line` to `max_width` columns by truncating
/// each contained span, then pads the remainder with explicit spaces so the
/// rendered line covers exactly `max_width` cells.
///
/// Truncation prevents `Paragraph` from wrapping a long line and bleeding the
/// continuation onto adjacent rows. Padding to the full width ensures every
/// cell in the popup row is painted by `Paragraph` — Windows Terminal under
/// WSL does not always clear the right-hand cell of a wide grapheme (Korean,
/// emoji) when ratatui's diff sends a default-style space via `Clear`. Writing
/// an explicit styled space through the paragraph fixes the residue.
fn truncate_line_to_display_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 {
        return Line::default();
    }
    let mut remaining = max_width;
    let mut new_spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 1);
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        if span.content.width() <= remaining {
            remaining = remaining.saturating_sub(span.content.width());
            new_spans.push(span);
            continue;
        }
        let truncated = truncate_display_width(&span.content, remaining);
        remaining = remaining.saturating_sub(truncated.width());
        new_spans.push(Span::styled(truncated, span.style));
    }
    if remaining > 0 {
        new_spans.push(Span::styled(" ".repeat(remaining), line.style));
    }
    let mut truncated = Line::from(new_spans);
    truncated.style = line.style;
    truncated.alignment = line.alignment;
    truncated
}

fn reaction_users_popup_data_lines(reactions: &[ReactionUsersInfo]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if reactions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No reactions found",
            Style::default().fg(DIM),
        )));
    }

    for reaction in reactions {
        let count = reaction.users.len();
        let user_label = if count == 1 { "user" } else { "users" };
        lines.push(Line::from(Span::styled(
            format!("{} · {count} {user_label}", reaction.emoji.status_label()),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        if reaction.users.is_empty() {
            lines.push(Line::from(Span::styled(
                "  no users found",
                Style::default().fg(DIM),
            )));
        } else {
            lines.extend(
                reaction
                    .users
                    .iter()
                    .map(|user| Line::from(Span::raw(format!("  {}", user.display_name)))),
            );
        }
    }
    lines
}

fn emoji_reaction_picker_lines(
    reactions: &[EmojiReactionItem],
    selected: usize,
    max_visible_items: usize,
    thumbnail_urls: &[String],
) -> Vec<Line<'static>> {
    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_items = max_visible_items.max(1).min(reactions.len().max(1));
    let visible_range = emoji_reaction_visible_range(reactions.len(), selected, visible_items);

    let mut lines: Vec<Line<'static>> = reactions[visible_range.clone()]
        .iter()
        .enumerate()
        .map(|(offset, reaction)| {
            let index = visible_range.start + offset;
            let marker = if index == selected { "› " } else { "  " };
            let mut style = Style::default();
            if index == selected {
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            let thumbnail_ready = reaction
                .custom_image_url()
                .is_some_and(|url| thumbnail_urls.iter().any(|ready| ready == &url));
            Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format_emoji_reaction_item(reaction, thumbnail_ready), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter/Space react · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn emoji_reaction_visible_range(
    reactions_len: usize,
    selected: usize,
    visible_items: usize,
) -> std::ops::Range<usize> {
    let start = selected
        .saturating_add(1)
        .saturating_sub(visible_items)
        .min(reactions_len.saturating_sub(visible_items));
    let end = (start + visible_items).min(reactions_len);
    start..end
}

fn render_emoji_reaction_images(
    frame: &mut Frame,
    area: Rect,
    reactions: &[EmojiReactionItem],
    selected: usize,
    visible_items: usize,
    emoji_images: Vec<EmojiReactionImage<'_>>,
) {
    if area.width <= EMOJI_REACTION_IMAGE_WIDTH || area.height == 0 {
        return;
    }

    let selected = selected.min(reactions.len().saturating_sub(1));
    let visible_range = emoji_reaction_visible_range(reactions.len(), selected, visible_items);
    for (offset, reaction) in reactions[visible_range].iter().enumerate() {
        let Some(url) = reaction.custom_image_url() else {
            continue;
        };
        let Some(image) = emoji_images.iter().find(|image| image.url == url) else {
            continue;
        };
        let y = area
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= area.y.saturating_add(area.height.saturating_sub(1)) {
            continue;
        }
        let image_area = Rect::new(
            area.x.saturating_add(2),
            y,
            EMOJI_REACTION_IMAGE_WIDTH.min(area.width.saturating_sub(2)),
            1,
        );
        frame.render_widget(RatatuiImage::new(image.protocol), image_area);
    }
}

fn format_emoji_reaction_item(reaction: &EmojiReactionItem, thumbnail_ready: bool) -> String {
    match &reaction.emoji {
        crate::discord::ReactionEmoji::Unicode(emoji) => format!("{} {}", emoji, reaction.label),
        crate::discord::ReactionEmoji::Custom { .. } if thumbnail_ready => format!(
            "{}{}",
            " ".repeat(usize::from(EMOJI_REACTION_IMAGE_WIDTH.saturating_add(1))),
            reaction.label
        ),
        crate::discord::ReactionEmoji::Custom { name, .. } => name
            .as_deref()
            .map(|name| format!(":{name}: {}", reaction.label))
            .unwrap_or_else(|| format!(":custom: {}", reaction.label)),
    }
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

fn channel_name_style(channel: &ChannelState, active: bool) -> Style {
    match one_to_one_dm_recipient_status(channel) {
        Some(status) if active => Style::default()
            .fg(presence_color(status))
            .add_modifier(Modifier::BOLD),
        Some(status) => Style::default().fg(presence_color(status)),
        None => active_text_style(active, Style::default()),
    }
}

fn one_to_one_dm_recipient_status(channel: &ChannelState) -> Option<PresenceStatus> {
    if !matches!(channel.kind.as_str(), "dm" | "Private") || channel.recipients.len() != 1 {
        return None;
    }

    channel.recipients.first().map(|recipient| recipient.status)
}

fn highlight_style() -> Style {
    Style::default()
        .bg(Color::Rgb(24, 54, 65))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

fn mention_highlight_style() -> Style {
    Style::default()
        .bg(Color::Rgb(92, 76, 35))
        .fg(Color::Yellow)
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
        composer_content_line_count(state, area.width)
    } else {
        1
    };
    MIN_MESSAGE_INPUT_HEIGHT.max(content_lines.saturating_add(1))
}

fn composer_content_line_count(state: &DashboardState, width: u16) -> u16 {
    let mut line_count = composer_prompt_line_count(state.composer_input(), width);
    if state.is_composing() && state.reply_target_message_state().is_some() {
        line_count = line_count.saturating_add(1);
    }
    line_count
}

fn composer_prompt_line_count(input: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let prompt = format!("> {input}");
    wrap_text_lines(&prompt, width).len() as u16
}

#[cfg(test)]
mod tests {
    use ratatui::{
        layout::Rect,
        style::{Color, Modifier, Style},
    };
    use twilight_model::id::Id;
    use unicode_width::UnicodeWidthStr;

    use super::{
        ACCENT, DIM, DISCORD_EPOCH_MILLIS, MemberEntry, MessageContentLine, channel_name_style,
        composer_content_line_count, composer_lines, composer_prompt_line_count, composer_text,
        emoji_reaction_picker_lines, footer_hint, format_message_content,
        format_message_content_lines, format_message_sent_time, format_unix_millis_with_offset,
        highlight_style, inline_image_preview_area, inline_image_preview_row, member_display_label,
        mention_highlight_style, message_action_menu_lines, message_item_lines,
        message_viewport_lines, poll_box_border, poll_card_inner_width, poll_vote_picker_lines,
        reaction_users_popup_lines, reaction_users_visible_line_count, sync_view_heights,
        wrap_text_lines,
    };
    use crate::{
        discord::{
            AppEvent, AttachmentInfo, ChannelInfo, ChannelRecipientState, ChannelState,
            GuildMemberState, MemberInfo, MentionInfo, MessageKind, MessageSnapshotInfo,
            MessageState, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
            ReactionUserInfo, ReactionUsersInfo, ReplyInfo,
        },
        tui::{
            format::truncate_display_width,
            state::{
                DashboardState, EmojiReactionItem, FocusPane, MessageActionItem, MessageActionKind,
                PollVotePickerItem,
            },
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
        assert_eq!(composer_prompt_line_count("漢字仮", 4), 2);
    }

    #[test]
    fn reply_composer_text_uses_original_reply_target_after_selection_changes() {
        let mut state = state_with_message();
        state.open_selected_message_actions();
        state.activate_selected_message_action();
        push_message(&mut state, 2, "newer selected message");

        assert_eq!(
            state
                .selected_message_state()
                .and_then(|message| message.content.as_deref()),
            Some("newer selected message")
        );

        assert_eq!(composer_text(&state, 80), "reply to hello\n> ");
    }

    #[test]
    fn reply_composer_hint_line_is_dim() {
        let mut state = state_with_message();
        state.open_selected_message_actions();
        state.activate_selected_message_action();

        let lines = composer_lines(&state, 80);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["reply to hello", "> "]
        );
        assert_eq!(lines[0].spans[0].style.fg, Some(DIM));
        assert_eq!(lines[1].spans[0].style.fg, None);
    }

    #[test]
    fn one_to_one_dm_channel_name_uses_recipient_status_color() {
        let channel = channel_with_recipients("dm", &[PresenceStatus::DoNotDisturb]);

        let inactive_style = channel_name_style(&channel, false);
        let active_style = channel_name_style(&channel, true);

        assert_eq!(inactive_style.fg, Some(Color::Red));
        assert_eq!(active_style.fg, Some(Color::Red));
        assert!(active_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unknown_dm_status_uses_dim_channel_style() {
        let channel = channel_with_recipients("dm", &[PresenceStatus::Unknown]);

        assert_eq!(
            channel_name_style(&channel, false).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(channel_name_style(&channel, true).fg, Some(Color::DarkGray));
        assert!(
            channel_name_style(&channel, true)
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn group_dm_channel_name_keeps_default_channel_style() {
        let channel = channel_with_recipients(
            "group-dm",
            &[PresenceStatus::Online, PresenceStatus::DoNotDisturb],
        );

        assert_eq!(channel_name_style(&channel, false).fg, None);
        assert_eq!(channel_name_style(&channel, true).fg, Some(Color::Green));
    }

    #[test]
    fn reply_composer_line_count_includes_reply_hint() {
        let mut state = state_with_message();
        state.open_selected_message_actions();
        state.activate_selected_message_action();
        state.push_composer_char('h');
        state.push_composer_char('\n');
        state.push_composer_char('i');

        assert_eq!(composer_content_line_count(&state, 80), 3);
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
            message_with_attachment(Some("漢字仮名交じ".to_owned()), image_attachment());
        message.attachments.clear();

        let lines = format_message_content_lines(&message, &DashboardState::new(), 10);

        assert_eq!(line_texts(&lines), vec!["漢字仮名交", "じ"]);
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
    fn message_content_highlights_current_user_mentions() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "username")];
        let mut state = state_with_member(10, "server alias");
        state.push_event(AppEvent::Ready {
            user: "server alias".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @server alias"]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@server alias");
        assert_eq!(lines[1].spans[2].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn message_content_does_not_highlight_other_user_mentions() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "alice")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @alice"]
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .all(|span| span.style.bg != mention_highlight_style().bg)
        );
    }

    #[test]
    fn message_content_highlights_everyone_mentions_for_current_user() {
        let mut message =
            message_with_attachment(Some("ping @everyone".to_owned()), image_attachment());
        message.attachments.clear();
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   ping @everyone"]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
        assert_eq!(lines[1].spans[2].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn message_content_highlights_mixed_everyone_and_direct_mentions_in_order() {
        let mut message =
            message_with_attachment(Some("@everyone hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "neo")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   @everyone hello @neo"]
        );
        assert_eq!(lines[1].spans[1].content.as_ref(), "@everyone");
        assert_eq!(lines[1].spans[3].content.as_ref(), "@neo");
        assert_eq!(lines[1].spans[1].style.bg, mention_highlight_style().bg);
        assert_eq!(lines[1].spans[3].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn message_content_highlights_here_mentions_for_current_user() {
        let mut message =
            message_with_attachment(Some("ping @here".to_owned()), image_attachment());
        message.attachments.clear();
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   ping @here"]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@here");
        assert_eq!(lines[1].spans[2].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn mention_like_display_name_does_not_duplicate_highlight_spans() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "everyone")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "everyone".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @everyone"]
        );
        assert_eq!(lines[1].spans.len(), 3);
        assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
        assert_eq!(lines[1].spans[2].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn message_content_prefers_cached_member_alias_over_mention_metadata() {
        let mut message =
            message_with_attachment(Some("hello <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "username")];
        let state = state_with_member(10, "server alias");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["hello @server alias"]);
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
    fn user_join_message_type_uses_join_label() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.message_kind = MessageKind::new(7);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["joined the server"]);
        assert_eq!(lines[0].style, Style::default().fg(DIM));
    }

    #[test]
    fn boost_message_types_use_discord_like_copy() {
        for (kind, label) in [
            (8, "neo boosted the server"),
            (9, "neo boosted the server to Level 1"),
            (10, "neo boosted the server to Level 2"),
            (11, "neo boosted the server to Level 3"),
        ] {
            let mut message = message_with_attachment(Some(String::new()), image_attachment());
            message.attachments.clear();
            message.message_kind = MessageKind::new(kind);

            let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

            assert_eq!(line_texts(&lines), vec![label]);
            assert_eq!(lines[0].style, Style::default().fg(ACCENT));
        }
    }

    #[test]
    fn thread_created_message_uses_cached_thread_details() {
        let mut message =
            message_with_attachment(Some("release notes".to_owned()), image_attachment());
        message.attachments.clear();
        message.message_kind = MessageKind::new(18);
        let mut state = DashboardState::new();
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            parent_id: Some(message.channel_id),
            position: None,
            last_message_id: None,
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            recipients: None,
        }));

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(
            line_texts(&lines),
            vec![
                "neo started a thread",
                "# release notes",
                "12 messages · Open thread to view messages"
            ]
        );
    }

    #[test]
    fn thread_starter_message_uses_referenced_message_card() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.message_kind = MessageKind::new(21);
        message.reply = Some(ReplyInfo {
            author: "alice".to_owned(),
            content: Some("original topic".to_owned()),
            mentions: Vec::new(),
        });

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec!["Thread starter message", "╭─ alice : original topic"]
        );
    }

    #[test]
    fn poll_result_message_uses_result_card() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.message_kind = MessageKind::new(46);
        message.poll = Some(PollInfo {
            question: "What should we eat?".to_owned(),
            answers: vec![PollAnswerInfo {
                answer_id: 1,
                text: "Soup".to_owned(),
                vote_count: Some(5),
                me_voted: false,
            }],
            allow_multiselect: false,
            results_finalized: Some(true),
            total_votes: Some(7),
        });

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec![
                "Poll results",
                "What should we eat?",
                "Winner: Soup with 5 votes",
                "7 total votes · Final results"
            ]
        );
    }

    #[test]
    fn reply_message_uses_preview_instead_of_type_label() {
        let mut message =
            message_with_attachment(Some("message body".to_owned()), image_attachment());
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "casey".to_owned(),
            content: Some("looks good".to_owned()),
            mentions: Vec::new(),
        });

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(
            line_texts(&lines),
            vec![
                "╭─ casey : looks good",
                "message body",
                "[image: cat.png] 640x480"
            ]
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
        message.message_kind = MessageKind::new(255);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(lines[0].text, "<unsupported message type>");
    }

    #[test]
    fn poll_message_replaces_empty_message_placeholder() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.poll = Some(poll_info(false));

        let width = 40;
        let lines = format_message_content_lines(&message, &DashboardState::new(), width);
        let texts = line_texts(&lines);

        assert_eq!(texts[0], poll_box_border('╭', '╮', width));
        assert_eq!(texts[1], poll_test_line("What should we eat?", width));
        assert_eq!(texts[2], poll_test_line("Select one answer", width));
        assert_eq!(texts[3], poll_test_line("  ◉ 1. Soup  2 votes  66%", width));
        assert_eq!(
            texts[4],
            poll_test_line("  ◯ 2. Noodles  1 votes  33%", width)
        );
        assert_eq!(
            texts[5],
            poll_test_line("3 votes · Results may still change", width)
        );
        assert_eq!(texts[6], poll_box_border('╰', '╯', width));
    }

    #[test]
    fn poll_message_notes_multiselect() {
        let mut message = message_with_attachment(Some(String::new()), image_attachment());
        message.attachments.clear();
        message.poll = Some(poll_info(true));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert!(lines[2].text.starts_with("│ Select one or more answers"));
        assert_eq!(lines[2].style, Style::default().fg(DIM));
    }

    #[test]
    fn poll_message_places_body_inside_box() {
        let mut message =
            message_with_attachment(Some("Please vote <@10>".to_owned()), image_attachment());
        message.attachments.clear();
        message.poll = Some(poll_info(false));
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 40);

        assert_eq!(lines[1].text, poll_test_line("What should we eat?", 40));
        assert_eq!(lines[2].text, poll_test_line("Please vote @alice", 40));
        assert!(lines[3].text.starts_with("│ Select one answer"));
    }

    #[test]
    fn poll_message_body_highlights_mentions_inside_box() {
        let mut message =
            message_with_attachment(Some("<@10> please vote".to_owned()), image_attachment());
        message.attachments.clear();
        message.mentions = vec![mention_info(10, "server alias")];
        message.poll = Some(poll_info(false));
        let mut state = state_with_member(10, "server alias");
        state.push_event(AppEvent::Ready {
            user: "server alias".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = format_message_content_lines(&message, &state, 40);
        let spans = lines[2].spans();

        assert_eq!(spans[0].content.as_ref(), "│ ");
        assert_eq!(spans[1].content.as_ref(), "@server alias");
        assert_eq!(spans[1].style.bg, mention_highlight_style().bg);
    }

    #[test]
    fn message_content_renders_reaction_chips_below_message() {
        let mut message = message_with_attachment(Some("hello".to_owned()), image_attachment());
        message.attachments.clear();
        message.reactions = vec![ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            count: 3,
            me: true,
        }];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello", "[● 👍 3]"]);
        assert_eq!(lines[1].style, Style::default().fg(ACCENT));
    }

    #[test]
    fn lay_out_reaction_chips_unicode_only_emits_no_image_slots() {
        let reactions = vec![
            ReactionInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                count: 3,
                me: true,
            },
            ReactionInfo {
                emoji: ReactionEmoji::Unicode("❤".to_owned()),
                count: 1,
                me: false,
            },
        ];

        let layout = super::lay_out_reaction_chips(&reactions, 200);

        assert_eq!(layout.lines, vec!["[● 👍 3]  [❤ 1]"]);
        assert!(layout.slots.is_empty());
    }

    #[test]
    fn lay_out_reaction_chips_custom_emoji_reserves_image_slot() {
        let reactions = vec![
            ReactionInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                count: 2,
                me: false,
            },
            ReactionInfo {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(42),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                count: 1,
                me: true,
            },
        ];

        let layout = super::lay_out_reaction_chips(&reactions, 200);

        // First line concatenates both chips with two spaces; the custom-emoji
        // chip reserves two cells of spaces in place of the textual `:name:`.
        assert_eq!(layout.lines, vec!["[👍 2]  [●    1]"]);
        assert_eq!(layout.slots.len(), 1);
        let slot = &layout.slots[0];
        assert_eq!(slot.line, 0);
        // "[👍 2]" is 6 cells, plus "  " separator = 8 cells of preceding text.
        // Inside the chip "[● " is 3 cells, so the image starts at col 8 + 3 = 11.
        assert_eq!(slot.col, 11);
        assert!(slot.url.contains("42.png"));
    }

    #[test]
    fn lay_out_reaction_chips_wraps_at_chip_boundary() {
        let reactions = (0..3)
            .map(|i| ReactionInfo {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(100 + i),
                    name: Some(format!("e{i}")),
                    animated: false,
                },
                count: i + 1,
                me: false,
            })
            .collect::<Vec<_>>();

        // Each chip width: "[" + 2 placeholder spaces + " " + count + "]" = 6.
        // Two chips with separator = 6 + 2 + 6 = 14. Three would be 14 + 2 + 6 = 22.
        let layout = super::lay_out_reaction_chips(&reactions, 14);

        assert_eq!(layout.lines.len(), 2);
        // First two chips on line 0, third chip on line 1.
        assert_eq!(layout.slots.len(), 3);
        assert_eq!(layout.slots[0].line, 0);
        assert_eq!(layout.slots[1].line, 0);
        assert_eq!(layout.slots[2].line, 1);
        // Third chip starts at col 0 of the wrapped second line, image at col 1.
        assert_eq!(layout.slots[2].col, 1);
    }

    #[test]
    fn message_action_menu_marks_selected_and_disabled_actions() {
        let actions = vec![
            MessageActionItem {
                kind: MessageActionKind::Reply,
                label: "Reply".to_owned(),
                enabled: true,
            },
            MessageActionItem {
                kind: MessageActionKind::DownloadImage,
                label: "Download image".to_owned(),
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
    fn emoji_reaction_picker_marks_selected_reaction() {
        let reactions = vec![
            EmojiReactionItem {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                label: "Thumbs up".to_owned(),
            },
            EmojiReactionItem {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(42),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                label: "Party".to_owned(),
            },
        ];

        let lines = emoji_reaction_picker_lines(&reactions, 1, 10, &[]);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "  👍 Thumbs up",
                "› :party: Party",
                "Enter/Space react · Esc close"
            ]
        );
    }

    #[test]
    fn poll_vote_picker_marks_selected_and_checked_answers() {
        let answers = vec![
            PollVotePickerItem {
                answer_id: 1,
                label: "Soup".to_owned(),
                selected: true,
            },
            PollVotePickerItem {
                answer_id: 2,
                label: "Noodles".to_owned(),
                selected: false,
            },
        ];

        let lines = poll_vote_picker_lines(&answers, 1);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "  [x] Soup",
                "› [ ] Noodles",
                "Space toggle · Enter vote · Esc close",
            ]
        );
    }

    #[test]
    fn reaction_users_popup_groups_users_by_reaction() {
        let lines = reaction_users_popup_lines(
            &[
                ReactionUsersInfo {
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    users: vec![
                        ReactionUserInfo {
                            user_id: Id::new(10),
                            display_name: "neo".to_owned(),
                        },
                        ReactionUserInfo {
                            user_id: Id::new(11),
                            display_name: "trinity".to_owned(),
                        },
                    ],
                },
                ReactionUsersInfo {
                    emoji: ReactionEmoji::Custom {
                        id: Id::new(50),
                        name: Some("party".to_owned()),
                        animated: false,
                    },
                    users: Vec::new(),
                },
            ],
            0,
            10,
            56,
        );

        let trimmed = line_texts_from_ratatui(&lines)
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            trimmed,
            vec![
                "👍 · 2 users",
                "  neo",
                "  trinity",
                ":party: · 0 users",
                "  no users found",
                "Esc close",
            ]
        );
    }

    #[test]
    fn reaction_users_popup_scrolls_long_lists() {
        let reactions = vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            users: (1..=6)
                .map(|id| ReactionUserInfo {
                    user_id: Id::new(id),
                    display_name: format!("user-{id}"),
                })
                .collect(),
        }];

        let lines = reaction_users_popup_lines(&reactions, 3, 3, 56);

        let trimmed = line_texts_from_ratatui(&lines)
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            trimmed,
            vec![
                "  user-3",
                "  user-4",
                "  user-5",
                "j/k scroll · more above/below · Esc close",
            ]
        );
    }

    #[test]
    fn reaction_users_popup_buffer_renders_without_wrap_artifacts() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut state = DashboardState::new();
        state.push_event(AppEvent::ReactionUsersLoaded {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![
                ReactionUsersInfo {
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    users: vec![
                        ReactionUserInfo {
                            user_id: Id::new(1),
                            display_name: "갱생케가".to_owned(),
                        },
                        ReactionUserInfo {
                            user_id: Id::new(2),
                            display_name: "하나비".to_owned(),
                        },
                        ReactionUserInfo {
                            user_id: Id::new(3),
                            display_name: "슬기인뎅".to_owned(),
                        },
                        ReactionUserInfo {
                            user_id: Id::new(4),
                            display_name: "won".to_owned(),
                        },
                    ],
                },
                ReactionUsersInfo {
                    emoji: ReactionEmoji::Unicode("❤️".to_owned()),
                    users: vec![ReactionUserInfo {
                        user_id: Id::new(5),
                        display_name: "파닥파닥( 40%..? )".to_owned(),
                    }],
                },
            ],
        });

        // Use a wide terminal so the popup's full POPUP_TARGET_WIDTH (58)
        // applies and line truncation should never trigger.
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");

        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new());
            })
            .expect("first draw");

        // Scroll the popup down past the long username, then back up. The
        // reported bug appeared after the long username was rendered and the
        // user scrolled up through earlier names — that is the diff path the
        // popup must survive without bleeding the wrap continuation onto
        // neighbouring rows.
        for _ in 0..6 {
            state.scroll_reaction_users_popup_down();
        }
        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new());
            })
            .expect("second draw");
        for _ in 0..6 {
            state.scroll_reaction_users_popup_up();
        }
        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new());
            })
            .expect("third draw");

        let buffer = terminal.backend().buffer();
        let dump = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|col| buffer[(col, row)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        // The reported artefact was the trailing fragment "? )" from
        // "파닥파닥( 40%..? )" appearing on rows that should hold a different
        // (shorter) name. After scrolling, count the number of rows whose
        // popup-content section ends with the long username's tail. Only the
        // single row that actually renders that user should match — any other
        // matches indicate wrap continuation has bled across rows.
        let trailing_matches = dump
            .iter()
            .filter(|line| line.contains("? )"))
            .count();
        assert!(
            trailing_matches <= 1,
            "popup buffer contained '? )' fragment on {trailing_matches} rows; expected at most 1.\nDump:\n{}",
            dump.join("\n")
        );
    }

    #[test]
    fn reaction_users_popup_buffer_stays_clean_in_narrow_terminal() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut state = DashboardState::new();
        state.push_event(AppEvent::ReactionUsersLoaded {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                users: vec![
                    ReactionUserInfo {
                        user_id: Id::new(1),
                        display_name: "won".to_owned(),
                    },
                    ReactionUserInfo {
                        user_id: Id::new(2),
                        display_name: "파닥파닥( 40%..? )".to_owned(),
                    },
                ],
            }],
        });

        // Narrow terminal that would force the popup down to a width where
        // the long name no longer fits without wrapping. Pre-truncation must
        // turn the long name into an ellipsis, never split it across rows.
        let backend = TestBackend::new(40, 25);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new());
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let dump = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|col| buffer[(col, row)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        let trailing_matches = dump.iter().filter(|line| line.contains("? )")).count();
        assert!(
            trailing_matches <= 1,
            "popup buffer contained '? )' fragment on {trailing_matches} rows; expected at most 1.\nDump:\n{}",
            dump.join("\n")
        );
    }

    #[test]
    fn reaction_users_popup_truncates_long_lines_to_fit_width() {
        let reactions = vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
            users: vec![
                ReactionUserInfo {
                    user_id: Id::new(1),
                    display_name: "won".to_owned(),
                },
                ReactionUserInfo {
                    user_id: Id::new(2),
                    display_name: "파닥파닥( 40%..? )".to_owned(),
                },
            ],
        }];

        // Inner width that is narrower than the long Korean+ASCII display name
        // forces the popup logic to truncate. Without truncation, ratatui's
        // wrap would split the long name and the wrap continuation would bleed
        // onto adjacent rows.
        let lines = reaction_users_popup_lines(&reactions, 0, 4, 12);

        for line in &lines {
            assert!(
                line.width() <= 12,
                "line {:?} exceeded inner width",
                line_texts_from_ratatui(std::slice::from_ref(line))
            );
        }
    }

    #[test]
    fn reaction_users_popup_reserves_footer_space_in_short_areas() {
        assert_eq!(reaction_users_visible_line_count(Rect::new(0, 0, 20, 5)), 0);
        assert_eq!(reaction_users_visible_line_count(Rect::new(0, 0, 20, 6)), 1);
        assert_eq!(
            reaction_users_visible_line_count(Rect::new(0, 0, 20, 40)),
            14
        );
    }

    #[test]
    fn emoji_reaction_picker_reserves_space_for_loaded_custom_image() {
        let reactions = vec![EmojiReactionItem {
            emoji: ReactionEmoji::Custom {
                id: Id::new(42),
                name: Some("party".to_owned()),
                animated: false,
            },
            label: "Party".to_owned(),
        }];

        let lines = emoji_reaction_picker_lines(
            &reactions,
            0,
            10,
            &["https://cdn.discordapp.com/emojis/42.png".to_owned()],
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["›    Party", "Enter/Space react · Esc close"]
        );
    }

    #[test]
    fn emoji_reaction_picker_windows_long_lists_around_selection() {
        let reactions = (0..15)
            .map(|index| EmojiReactionItem {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(100 + index),
                    name: Some(format!("emoji_{index}")),
                    animated: false,
                },
                label: format!("Emoji {index}"),
            })
            .collect::<Vec<_>>();

        let lines = emoji_reaction_picker_lines(&reactions, 12, 5, &[]);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "  :emoji_8: Emoji 8",
                "  :emoji_9: Emoji 9",
                "  :emoji_10: Emoji 10",
                "  :emoji_11: Emoji 11",
                "› :emoji_12: Emoji 12",
                "Enter/Space react · Esc close"
            ]
        );
    }

    #[test]
    fn footer_hint_switches_for_emoji_picker() {
        let mut state = state_with_message();
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        assert_eq!(
            footer_hint(&state),
            "j/k choose emoji | enter/space react | esc close"
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
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
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
            message_with_forwarded_snapshot(forwarded_snapshot(Some("漢字仮名交じ"), Vec::new()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 12);

        assert_eq!(
            line_texts(&lines),
            vec!["↱ Forwarded", "│ 漢字仮名交", "│ じ"]
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
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
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
    fn message_item_header_uses_display_width_for_wide_author() {
        let ascii = message_item_lines(
            "bruised8".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("plain text".to_owned())],
            14,
            0,
            0,
        );
        let wide = message_item_lines(
            "漢字名".to_owned(),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("plain text".to_owned())],
            14,
            0,
            0,
        );

        assert_eq!(line_texts_from_ratatui(&ascii)[0], "oo bruised8 00:00");
        assert_eq!(line_texts_from_ratatui(&wide)[0], "oo 漢字名 00:00");
    }

    #[test]
    fn shared_truncation_uses_display_width_for_wide_characters() {
        let author = truncate_display_width("漢字仮名交じり", 8);

        assert_eq!(author, "漢字...");
        assert_eq!(author.width(), 7);
    }

    #[test]
    fn member_label_truncates_by_display_width() {
        let member = GuildMemberState {
            user_id: Id::new(10),
            display_name: "漢字仮名交じり文章".to_owned(),
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
            status: PresenceStatus::Online,
        };

        let label = member_display_label(MemberEntry::Guild(&member), 12);

        assert_eq!(label, "漢字仮名...");
        assert!(label.width() <= 12);
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
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content,
            mentions: Vec::new(),
            attachments: vec![attachment],
            forwarded_snapshots: Vec::new(),
        }
    }

    fn state_with_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.focus_pane(FocusPane::Messages);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn push_message(state: &mut DashboardState, message_id: u64, content: &str) {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
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
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![snapshot],
        }
    }

    fn poll_info(allow_multiselect: bool) -> PollInfo {
        PollInfo {
            question: "What should we eat?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "Soup".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "Noodles".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect,
            results_finalized: Some(false),
            total_votes: Some(3),
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
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state
    }

    fn member_info(user_id: u64, display_name: &str) -> MemberInfo {
        MemberInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
        }
    }

    fn channel_with_recipients(kind: &str, statuses: &[PresenceStatus]) -> ChannelState {
        ChannelState {
            id: Id::new(10),
            guild_id: None,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: kind.to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: statuses
                .iter()
                .enumerate()
                .map(|(index, status)| ChannelRecipientState {
                    user_id: Id::new(100 + u64::try_from(index).expect("index should fit u64")),
                    display_name: format!("recipient {index}"),
                    is_bot: false,
                    avatar_url: None,
                    status: *status,
                })
                .collect(),
        }
    }

    fn line_texts(lines: &[MessageContentLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    fn poll_test_line(text: &str, width: usize) -> String {
        let inner_width = poll_card_inner_width(width);
        let padding = inner_width.saturating_sub(text.width());
        format!("│ {text}{} │", " ".repeat(padding))
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
