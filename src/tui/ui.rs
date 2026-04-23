use crate::discord::ids::{Id, marker::MessageMarker};
use chrono::{DateTime, Local, NaiveDate};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use ratatui_image::{Image as RatatuiImage, Resize, StatefulImage, protocol::StatefulProtocol};
use unicode_width::UnicodeWidthStr;

use super::{
    format::{truncate_display_width, truncate_text},
    message_format::{
        EMOJI_REACTION_IMAGE_WIDTH, MessageContentLine, embed_color, format_attachment_summary,
        format_message_content_lines, lay_out_reaction_chips, wrap_text_lines,
    },
    state::{
        ChannelActionItem, ChannelPaneEntry, ChannelThreadItem, DashboardState, EmojiReactionItem,
        FocusPane, GuildPaneEntry, MAX_MENTION_PICKER_VISIBLE, MemberActionItem, MemberEntry,
        MemberGroup, MentionPickerEntry, MessageActionItem, PollVotePickerItem, discord_color,
        folder_color, presence_color, presence_marker,
    },
};
use crate::discord::{
    ChannelState, ChannelVisibilityStats, FriendStatus, MessageState, PresenceStatus,
    ReactionUsersInfo, UserProfileInfo,
};

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const SCROLLBAR_THUMB: Color = Color::Rgb(170, 170, 170);
const MIN_MESSAGE_INPUT_HEIGHT: u16 = 3;
const IMAGE_PREVIEW_HEIGHT: u16 = 10;
const IMAGE_PREVIEW_WIDTH: u16 = 72;
const MESSAGE_AVATAR_PLACEHOLDER: &str = "oo";
const MESSAGE_AVATAR_OFFSET: u16 = 3;
pub(crate) const MESSAGE_ROW_GAP: usize = 1;
const EMBED_PREVIEW_GUTTER_PREFIX: &str = "  ▎ ";
const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
const MAX_REACTION_USERS_VISIBLE_LINES: usize = 14;

pub struct ImagePreview<'a> {
    pub message_index: usize,
    pub preview_height: u16,
    pub accent_color: Option<u32>,
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
    /// One-row strip rendered between the message list and the composer when
    /// somebody else is typing in the selected channel. Width is zero when
    /// nobody is typing so the message list reclaims the row.
    typing: Rect,
    composer: Rect,
}

struct UserProfilePopupText {
    lines: Vec<Line<'static>>,
    selected_line: Option<usize>,
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
    profile_avatar: Option<AvatarImage>,
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
    render_channel_action_menu(frame, areas.messages, state);
    render_member_action_menu(frame, areas.messages, state);
    render_poll_vote_picker(frame, areas.messages, state);
    render_emoji_reaction_picker(frame, areas.messages, state, emoji_images);
    render_reaction_users_popup(frame, areas.messages, state);
    render_user_profile_popup(frame, areas.messages, state, profile_avatar);
    render_debug_log_popup(frame, areas.messages, state);
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
                            truncate_display_width(entry.label(), max_width),
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
                        let label_width = max_width.saturating_sub(arrow.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(color)),
                            Span::styled(
                                truncate_display_width(&title, label_width),
                                Style::default().fg(color).add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    GuildPaneEntry::Guild { state, branch } => {
                        let prefix = branch.prefix();
                        let label_width = max_width.saturating_sub(prefix.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(prefix, Style::default().fg(DIM)),
                            Span::styled(
                                truncate_display_width(state.name.as_str(), label_width),
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
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.guild_scroll(),
        panel_content_height(area, "Servers"),
        state.guild_pane_entries().len(),
    );
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
                        let label_width = max_width.saturating_sub(arrow.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(arrow, Style::default().fg(ACCENT)),
                            Span::styled(
                                truncate_display_width(&state.name, label_width),
                                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                            ),
                        ]))
                    }
                    ChannelPaneEntry::Channel { state, branch } => {
                        let branch_prefix = branch.prefix();
                        let channel_prefix = channel_prefix(&state.kind);
                        let label_width = max_width
                            .saturating_sub(branch_prefix.width())
                            .saturating_sub(channel_prefix.width());
                        ListItem::new(Line::from(vec![
                            selection_marker(is_selected),
                            Span::styled(branch_prefix, Style::default().fg(DIM)),
                            Span::styled(channel_prefix, Style::default().fg(DIM)),
                            Span::styled(
                                truncate_display_width(&state.name, label_width),
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
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.channel_scroll(),
        panel_content_height(area, "Channels"),
        state.channel_pane_entries().len(),
    );
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

    let lines = message_viewport_lines(
        &messages,
        selected,
        state,
        content_width,
        message_areas.list.width as usize,
        &image_previews,
    );

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
        if let Some(preview_area) = inline_image_preview_area(
            message_areas.list,
            row,
            image_preview.preview_height,
            image_preview.accent_color,
        ) {
            render_image_preview(frame, preview_area, image_preview.state);
        }
        previous_preview_rows =
            previous_preview_rows.saturating_add(image_preview.preview_height as usize);
    }
    let preview_width = inline_image_preview_width(message_areas.list);
    let max_preview_height = inline_image_preview_height(message_areas.list, true);
    render_vertical_scrollbar(
        frame,
        message_areas.list,
        state.message_scroll_row_position(content_width, preview_width, max_preview_height),
        message_areas.list.height as usize,
        state.message_total_rendered_rows(content_width, preview_width, max_preview_height),
    );
    render_typing_footer(frame, message_areas.typing, state);
    render_composer(frame, message_areas.composer, state);
    render_composer_mention_picker(frame, message_areas, state);
}

fn render_typing_footer(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if area.height == 0 {
        return;
    }
    // The text might already be `None` if the only typer was the local user
    // and `message_areas` reserved the strip on a stale read. Render the
    // footer if and only if we still have a label to show.
    let Some(text) = state.typing_footer_for_selected_channel() else {
        return;
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, Style::default().fg(DIM)))),
        area,
    );
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
        let global_index = state.message_scroll().saturating_add(index);
        let separator_lines = state.message_extra_top_lines(global_index) as isize;
        let body_base_rows =
            state.message_base_line_count_for_width(message, content_width) as isize;
        let block_rows = body_base_rows + separator_lines;
        let preview_height = preview_for_message(image_previews, index)
            .map(|preview| preview.preview_height)
            .unwrap_or(0) as isize;

        let layout = lay_out_reaction_chips(&message.reactions, content_width);
        if !layout.slots.is_empty() {
            // Reactions live in the last `layout.lines.len()` rows of the
            // message's base content (header + body), before the preview
            // spacer. The body starts after the optional date separator,
            // so the reaction strip begins at:
            //     body_top + (body_base_rows - reaction_lines)
            let message_top = rendered_rows - line_offset;
            let body_top = message_top + separator_lines;
            let reaction_strip_top =
                body_top + body_base_rows.saturating_sub(layout.lines.len() as isize);

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

        rendered_rows = rendered_rows
            .saturating_add((block_rows + preview_height + MESSAGE_ROW_GAP as isize) - line_offset);
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

fn preview_for_message<'a>(
    image_previews: &'a [ImagePreview<'a>],
    message_index: usize,
) -> Option<&'a ImagePreview<'a>> {
    image_previews
        .iter()
        .find(|preview| preview.message_index == message_index)
}

fn message_viewport_lines(
    messages: &[&MessageState],
    selected: Option<usize>,
    state: &DashboardState,
    content_width: usize,
    list_width: usize,
    image_previews: &[ImagePreview<'_>],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let author = message.author.clone();
        let author_style = message_author_style(state.message_author_role_color(message));
        let content = format_message_content_lines(message, state, content_width.max(8));
        let preview = preview_for_message(image_previews, index);
        let preview_height = preview.map(|preview| preview.preview_height).unwrap_or(0);

        let global_index = state.message_scroll().saturating_add(index);
        let separator_line = state
            .message_starts_new_day_at(global_index)
            .then(|| date_separator_line(message.id, list_width));
        let separator_lines = usize::from(separator_line.is_some());
        let line_offset = usize::from(index == 0) * state.message_line_scroll();
        let body_skip = line_offset.saturating_sub(separator_lines);

        if let Some(line) = separator_line.filter(|_| line_offset == 0) {
            lines.push(line);
        }

        let item_lines = message_item_lines(
            author,
            author_style,
            format_message_sent_time(message.id),
            content,
            content_width,
            preview_height,
            preview.and_then(|preview| preview.accent_color),
            body_skip,
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
    author_style: Style,
    sent_time: String,
    content: Vec<MessageContentLine>,
    content_width: usize,
    preview_height: u16,
    preview_accent_color: Option<u32>,
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
        Span::styled(author, author_style),
        Span::raw(" "),
        Span::styled(sent_time, Style::default().fg(DIM)),
    ])];
    lines.extend(content.into_iter().map(|line| {
        let mut spans = vec![message_avatar_spacer_span()];
        spans.extend(line.spans());
        Line::from(spans)
    }));
    lines.extend(image_preview_spacer_lines(
        preview_height,
        preview_accent_color,
    ));
    lines.push(Line::from(""));
    lines.into_iter().skip(line_offset).collect()
}

fn message_author_style(role_color: Option<u32>) -> Style {
    Style::default()
        .fg(discord_color(role_color, Color::White))
        .bold()
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

fn message_local_date(message_id: Id<MessageMarker>) -> NaiveDate {
    let unix_millis = (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS;
    i64::try_from(unix_millis)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map(|dt| dt.with_timezone(&Local).date_naive())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2015, 1, 1).expect("static date is valid"))
}

/// Returns true when a message at `current` should be preceded by a date
/// separator because its local-date differs from the previous visible message.
/// Returns false when there is no previous message so the channel's earliest
/// visible message does not gain a separator on its own.
pub(crate) fn message_starts_new_day(
    current: Id<MessageMarker>,
    previous: Option<Id<MessageMarker>>,
) -> bool {
    match previous {
        None => false,
        Some(prev) => message_local_date(current) != message_local_date(prev),
    }
}

fn date_separator_line(message_id: Id<MessageMarker>, width: usize) -> Line<'static> {
    let date = message_local_date(message_id);
    let label = format!(" {} ", date.format("%Y-%m-%d"));
    let label_width = label.as_str().width();
    let total = width.max(label_width.saturating_add(2));
    let dashes = total.saturating_sub(label_width);
    let left = dashes / 2;
    let right = dashes.saturating_sub(left);
    Line::from(Span::styled(
        format!("{}{}{}", "─".repeat(left), label, "─".repeat(right)),
        Style::default().fg(DIM),
    ))
}

#[cfg(test)]
fn format_unix_millis_with_offset(unix_millis: u64, offset: chrono::FixedOffset) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&offset).format("%H:%M").to_string())
}

fn image_preview_spacer_lines(height: u16, accent_color: Option<u32>) -> Vec<Line<'static>> {
    (0..height)
        .map(|_| match accent_color {
            Some(color) => Line::from(vec![
                message_avatar_spacer_span(),
                Span::styled(
                    EMBED_PREVIEW_GUTTER_PREFIX,
                    Style::default().fg(embed_color(color)),
                ),
            ]),
            None => Line::from(""),
        })
        .collect()
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
    let inner_width = composer_inner_width(area.width);
    let prompt = composer_lines(state, inner_width);
    let border_color = if state.is_composing() { ACCENT } else { DIM };

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
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border_color))
                    .title_style(Style::default().fg(Color::White).bold()),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_composer_mention_picker(
    frame: &mut Frame,
    message_areas: MessageAreas,
    state: &DashboardState,
) {
    if state.composer_mention_query().is_none() {
        return;
    }
    let candidates = state.composer_mention_candidates();
    if candidates.is_empty() {
        return;
    }
    let Some(area) = mention_picker_area(message_areas, candidates.len()) else {
        return;
    };
    frame.render_widget(Clear, area);
    let inner_width = area.width.saturating_sub(2) as usize;
    let lines = mention_picker_lines(&candidates, state.composer_mention_selected(), inner_width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DIM))
        .title(" mention ")
        .title_style(Style::default().fg(Color::White).bold());
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Picks a rectangle directly above the composer for the picker. Returns
/// `None` when there isn't enough room (very short terminal) so the caller
/// can silently skip drawing.
fn mention_picker_area(message_areas: MessageAreas, candidate_count: usize) -> Option<Rect> {
    let composer = message_areas.composer;
    let messages = message_areas.list;
    if composer.x < messages.x || composer.width == 0 {
        return None;
    }
    // 1 row per candidate + 2 for the bordered block.
    let desired_height = (candidate_count.min(MAX_MENTION_PICKER_VISIBLE) as u16).saturating_add(2);
    let available_above = composer.y.saturating_sub(messages.y);
    let height = desired_height.min(available_above);
    if height < 3 {
        return None;
    }
    let width = composer.width.clamp(20, 48).min(messages.width);
    let x = composer.x;
    let y = composer.y.saturating_sub(height);
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn mention_picker_lines(
    candidates: &[MentionPickerEntry],
    selected: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let max_label_width = width.saturating_sub(4).max(1);
    candidates
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let cursor = if index == selected { "› " } else { "  " };
            let bot_marker = if entry.is_bot { " [BOT]" } else { "" };
            // Show the raw username next to the alias when they differ so the
            // user can see which row matched their query when they typed
            // against the username instead of the alias.
            let username_hint = entry
                .username
                .as_deref()
                .filter(|name| !name.eq_ignore_ascii_case(&entry.display_name))
                .map(|name| format!(" @{name}"))
                .unwrap_or_default();
            let label = format!("{}{bot_marker}{username_hint}", entry.display_name);
            let label = truncate_display_width(&label, max_label_width);
            let mut row_style = Style::default().fg(presence_color(entry.status));
            if index == selected {
                row_style = row_style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            Line::from(vec![
                Span::styled(cursor, Style::default().fg(ACCENT)),
                Span::styled(presence_marker(entry.status).to_string(), row_style),
                Span::styled(" ", row_style),
                Span::styled(label, row_style),
            ])
        })
        .collect()
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
        // Tell the user up-front if the keymap won't open the composer here,
        // so they don't repeatedly press `i` and wonder why nothing happens.
        if !state.can_send_in_selected_channel() {
            return format!("read-only · cannot send messages in {label}");
        }
        // SEND is allowed but ATTACH isn't — flag it so a future attachment
        // picker has a coherent UX, and the user knows uploads will be
        // refused before they try.
        if !state.can_attach_in_selected_channel() {
            return format!("press i to write in {label} (attachments disabled)");
        }
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
            let name_style =
                member_name_style(member, state.member_role_color(member), is_selected);

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
            .block(panel_block_owned(state.member_panel_title(), focused))
            .wrap(Wrap { trim: false }),
        area,
    );
    render_vertical_scrollbar(
        frame,
        panel_scrollbar_area(area),
        state.member_scroll(),
        state.member_content_height(),
        state.member_line_count(),
    );
}

fn member_group_header(group: &MemberGroup<'_>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            group.label.clone(),
            Style::default()
                .fg(discord_color(group.color, DIM))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", group.entries.len()),
            Style::default().fg(DIM),
        ),
    ])
}

fn member_name_style(member: MemberEntry<'_>, role_color: Option<u32>, is_selected: bool) -> Style {
    let mut style = Style::default().fg(discord_color(role_color, Color::White));
    if matches!(
        member.status(),
        PresenceStatus::Offline | PresenceStatus::Unknown
    ) {
        style = style.add_modifier(Modifier::DIM);
    }
    if member.is_bot() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if is_selected {
        style = style
            .bg(Color::Rgb(24, 54, 65))
            .add_modifier(Modifier::BOLD);
    }
    style
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
    if state.is_debug_log_popup_open() {
        "`/esc close debug logs"
    } else if state.is_reaction_users_popup_open() {
        "esc close reacted users"
    } else if state.is_poll_vote_picker_open() {
        "j/k choose answer | space toggle | enter vote | esc close"
    } else if state.is_emoji_reaction_picker_open() {
        "j/k choose emoji | enter/space react | esc close"
    } else if state.is_user_profile_popup_open() {
        "j/k pick mutual server | enter open server | esc close"
    } else if state.is_message_action_menu_open() {
        "j/k choose action | enter select | esc close | q quit"
    } else if state.is_channel_action_menu_open() {
        if state.is_channel_action_threads_phase() {
            "j/k choose thread | enter open | esc/← back | q quit"
        } else {
            "j/k choose action | enter select | esc close | q quit"
        }
    } else if state.is_member_action_menu_open() {
        "j/k choose action | enter select | esc close | q quit"
    } else if state.focus() == FocusPane::Members {
        "tab/1-4 focus | j/k move | enter/space profile | a actions | i write | q quit"
    } else if state.focus() == FocusPane::Channels {
        "tab/1-4 focus | j/k move | enter/space open | ←/→ category | a actions | ` logs | i write | q quit"
    } else {
        "tab/1-4 focus | j/k move | J/K scroll | enter/space action/tree | ←/→ close/open | ` logs | i write | esc cancel | q quit"
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

fn render_channel_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_channel_action_menu_open() {
        return;
    }

    let title_suffix = state
        .channel_action_menu_title()
        .map(|name| format!(" — {name}"))
        .unwrap_or_default();

    if state.is_channel_action_threads_phase() {
        let threads = state.channel_action_thread_items();
        let selected = state.selected_channel_action_index().unwrap_or(0);
        let row_count = threads.len().max(1) as u16;
        let popup = centered_rect(area, 54, row_count.saturating_add(4));
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(channel_thread_menu_lines(&threads, selected))
                .block(panel_block_owned(format!("Threads{title_suffix}"), true))
                .wrap(Wrap { trim: false }),
            popup,
        );
        return;
    }

    let actions = state.selected_channel_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_channel_action_index().unwrap_or(0);
    let popup = centered_rect(area, 54, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(channel_action_menu_lines(&actions, selected))
            .block(panel_block_owned(
                format!("Channel actions{title_suffix}"),
                true,
            ))
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
    let desired_visible_items = reactions
        .len()
        .clamp(1, super::selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
    let popup = centered_rect(area, 42, (desired_visible_items as u16).saturating_add(4));
    let ready_urls = emoji_images
        .iter()
        .map(|image| image.url.clone())
        .collect::<Vec<_>>();
    let block = panel_block("Choose reaction", true);
    let content = block.inner(popup);
    let visible_items = usize::from(content.height.saturating_sub(1)).min(desired_visible_items);
    let visible_range =
        super::selection::visible_item_range(reactions.len(), selected, visible_items);
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
    render_vertical_scrollbar(
        frame,
        Rect {
            height: visible_items as u16,
            ..content
        },
        visible_range.start,
        visible_items,
        reactions.len(),
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

fn render_member_action_menu(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_member_action_menu_open() {
        return;
    }
    let actions = state.selected_member_action_items();
    if actions.is_empty() {
        return;
    }
    let selected = state.selected_member_action_index().unwrap_or(0);
    let title = state
        .member_action_menu_title()
        .map(|name| format!("Member actions — {name}"))
        .unwrap_or_else(|| "Member actions".to_owned());
    let popup = centered_rect(area, 48, (actions.len() as u16).saturating_add(4));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(member_action_menu_lines(&actions, selected))
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn member_action_menu_lines(actions: &[MemberActionItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
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
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn render_user_profile_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
    avatar: Option<AvatarImage>,
) {
    if !state.is_user_profile_popup_open() {
        return;
    }

    const POPUP_WIDTH: u16 = 60;
    const POPUP_HEIGHT: u16 = 24;
    const AVATAR_CELL_WIDTH: u16 = 8;
    const AVATAR_CELL_HEIGHT: u16 = 4;
    let width = POPUP_WIDTH.min(area.width.saturating_sub(2)).max(8);
    let height = POPUP_HEIGHT.min(area.height.saturating_sub(2)).max(6);
    let popup = centered_rect(area, width, height);
    frame.render_widget(Clear, popup);

    let block = panel_block("Profile", true);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // The avatar sits inside the inner area; reserve a fixed column gutter
    // so the text section starts cleanly to its right.
    let has_avatar = avatar.is_some()
        && state.user_profile_popup_avatar_url().is_some()
        && inner.width > AVATAR_CELL_WIDTH + 2;
    let text_area = if has_avatar {
        let gutter = AVATAR_CELL_WIDTH + 2;
        Rect {
            x: inner.x + gutter,
            y: inner.y,
            width: inner.width.saturating_sub(gutter),
            height: inner.height,
        }
    } else {
        inner
    };

    let popup_text = if let Some(profile) = state.user_profile_popup_data() {
        user_profile_popup_text(
            profile,
            state,
            text_area.width.saturating_sub(0),
            state.user_profile_popup_status(),
            state.user_profile_popup_mutual_cursor(),
        )
    } else if let Some(message) = state.user_profile_popup_load_error() {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                truncate_display_width(
                    &format!("Failed to load profile: {message}"),
                    text_area.width.into(),
                ),
                Style::default().fg(Color::Red),
            ))],
            selected_line: None,
        }
    } else {
        UserProfilePopupText {
            lines: vec![Line::from(Span::styled(
                "Loading profile...",
                Style::default().fg(DIM),
            ))],
            selected_line: None,
        }
    };
    let total_lines = popup_text.lines.len();
    let selected_line = popup_text.selected_line;
    let scroll_position =
        user_profile_popup_scroll_position(total_lines, selected_line, text_area.height as usize);
    let lines = user_profile_popup_visible_lines(
        popup_text.lines,
        selected_line,
        text_area.height as usize,
    );
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    render_vertical_scrollbar(
        frame,
        text_area,
        scroll_position,
        text_area.height as usize,
        total_lines,
    );

    if let Some(avatar) = avatar.filter(|_| has_avatar) {
        let avatar_area = Rect {
            x: inner.x,
            y: inner.y,
            width: AVATAR_CELL_WIDTH.min(inner.width),
            height: AVATAR_CELL_HEIGHT.min(inner.height),
        };
        frame.render_widget(RatatuiImage::new(&avatar.protocol), avatar_area);
    }
}

#[cfg(test)]
fn user_profile_popup_lines(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    mutual_cursor: Option<usize>,
) -> Vec<Line<'static>> {
    user_profile_popup_text(profile, state, width, status, mutual_cursor).lines
}

fn user_profile_popup_text(
    profile: &UserProfileInfo,
    state: &DashboardState,
    width: u16,
    status: PresenceStatus,
    mutual_cursor: Option<usize>,
) -> UserProfilePopupText {
    let inner_width = usize::from(width.max(8));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut selected_line = None;

    let display_name = profile.display_name().to_owned();
    lines.push(Line::from(Span::styled(
        truncate_display_width(&display_name, inner_width),
        user_profile_display_name_style(status),
    )));
    lines.push(Line::from(Span::styled(
        truncate_display_width(&format!("@{}", profile.username), inner_width),
        Style::default().fg(DIM),
    )));

    if let Some(pronouns) = profile.pronouns.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate_display_width(pronouns, inner_width),
            Style::default().fg(DIM),
        )));
    }

    let (badge_label, badge_color) = friend_status_badge(profile.friend_status);
    lines.push(Line::from(Span::styled(
        badge_label,
        Style::default()
            .fg(badge_color)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "ABOUT ME");
    push_wrapped_paragraph(
        &mut lines,
        profile
            .bio
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no bio)"),
        inner_width,
    );

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(&mut lines, "NOTE");
    push_wrapped_paragraph(
        &mut lines,
        profile
            .note
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(no note)"),
        inner_width,
    );

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(
        &mut lines,
        &format!("MUTUAL SERVERS ({})", profile.mutual_guilds.len()),
    );
    if profile.mutual_guilds.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (none)".to_owned(),
            Style::default().fg(DIM),
        )));
    } else {
        for (index, entry) in profile.mutual_guilds.iter().enumerate() {
            let name = state
                .guild_name(entry.guild_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("guild-{}", entry.guild_id.get()));
            let selected = mutual_cursor == Some(index);
            let marker = if selected { "› " } else { "  " };
            let body = match entry.nick.as_deref() {
                Some(nick) => format!("• {name} — {nick}"),
                None => format!("• {name}"),
            };
            let mut style = Style::default();
            if selected {
                selected_line = Some(lines.len());
                style = style
                    .bg(Color::Rgb(40, 45, 90))
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(vec![
                Span::styled(marker.to_owned(), Style::default().fg(ACCENT)),
                Span::styled(
                    truncate_display_width(&body, inner_width.saturating_sub(2)),
                    style,
                ),
            ]));
        }
    }

    lines.push(Line::from(Span::raw(String::new())));
    push_section_header(
        &mut lines,
        &format!("MUTUAL FRIENDS ({})", profile.mutual_friends_count),
    );

    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "j/k pick · Enter open · Esc close",
        Style::default().fg(DIM),
    )));
    UserProfilePopupText {
        lines,
        selected_line,
    }
}

fn user_profile_popup_visible_lines(
    lines: Vec<Line<'static>>,
    selected_line: Option<usize>,
    visible_height: usize,
) -> Vec<Line<'static>> {
    if visible_height == 0 {
        return Vec::new();
    }

    let scroll = user_profile_popup_scroll_position(lines.len(), selected_line, visible_height);

    lines
        .into_iter()
        .skip(scroll)
        .take(visible_height)
        .collect()
}

fn user_profile_popup_scroll_position(
    total_lines: usize,
    selected_line: Option<usize>,
    visible_height: usize,
) -> usize {
    let max_scroll = total_lines.saturating_sub(visible_height);
    selected_line
        .map(|line| line.saturating_add(1).saturating_sub(visible_height))
        .unwrap_or(0)
        .min(max_scroll)
}

fn user_profile_display_name_style(status: PresenceStatus) -> Style {
    Style::default()
        .fg(presence_color(status))
        .add_modifier(Modifier::BOLD)
}

fn friend_status_badge(status: FriendStatus) -> (String, Color) {
    match status {
        FriendStatus::Friend => ("● Friend".to_owned(), Color::Green),
        FriendStatus::IncomingRequest => ("● Incoming friend request".to_owned(), Color::Yellow),
        FriendStatus::OutgoingRequest => ("● Outgoing friend request".to_owned(), Color::Yellow),
        FriendStatus::Blocked => ("● Blocked".to_owned(), Color::Red),
        FriendStatus::None => ("● Not friends".to_owned(), DIM),
    }
}

fn push_section_header(lines: &mut Vec<Line<'static>>, label: &str) {
    lines.push(Line::from(Span::styled(
        label.to_owned(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )));
}

fn push_wrapped_paragraph(lines: &mut Vec<Line<'static>>, text: &str, width: usize) {
    for line in text.split('\n') {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            lines.push(Line::from(Span::raw(String::new())));
        } else {
            lines.push(Line::from(Span::raw(truncate_display_width(
                trimmed, width,
            ))));
        }
    }
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
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let inner_width = usize::from(popup_width.saturating_sub(2));

    let max_visible_lines = reaction_users_visible_line_count(area);
    let lines = reaction_users_popup_lines(
        popup_state.reactions(),
        popup_state.scroll(),
        max_visible_lines,
        inner_width,
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(panel_block("Reacted users", true)),
        popup,
    );
    render_vertical_scrollbar(
        frame,
        Rect {
            height: max_visible_lines as u16,
            ..panel_scrollbar_area(popup)
        },
        popup_state.scroll(),
        max_visible_lines,
        popup_state.data_line_count(),
    );
}

fn render_debug_log_popup(frame: &mut Frame, area: Rect, state: &DashboardState) {
    if !state.is_debug_log_popup_open() {
        return;
    }

    const POPUP_TARGET_WIDTH: u16 = 78;
    let popup_width = POPUP_TARGET_WIDTH.min(area.width.saturating_sub(2)).max(1);
    let visible_log_lines = usize::from(area.height).saturating_sub(6).max(1);
    let lines = debug_log_popup_lines(
        state.debug_log_lines(),
        state.debug_channel_visibility(),
        visible_log_lines,
        usize::from(popup_width.saturating_sub(2)),
    );
    let popup = centered_rect(
        area,
        POPUP_TARGET_WIDTH,
        (lines.len() as u16).saturating_add(2),
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Debug logs", true))
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

fn panel_scrollbar_area(area: Rect) -> Rect {
    area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    })
}

fn render_vertical_scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    viewport_len: usize,
    content_len: usize,
) {
    if area.height == 0 || viewport_len == 0 || content_len <= viewport_len {
        return;
    }

    let max_position = content_len.saturating_sub(viewport_len);
    let position = position.min(max_position);
    let scrollbar_content_len = max_position.saturating_add(1);
    let mut state = ScrollbarState::new(scrollbar_content_len)
        .position(position)
        .viewport_content_length(viewport_len);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("┃")
        .thumb_style(Style::default().fg(SCROLLBAR_THUMB))
        .track_style(Style::default().fg(DIM));

    frame.render_stateful_widget(scrollbar, area, &mut state);
}

fn reaction_users_visible_line_count(area: Rect) -> usize {
    usize::from(area.height)
        .saturating_sub(5)
        .min(MAX_REACTION_USERS_VISIBLE_LINES)
}

fn debug_log_popup_lines(
    entries: Vec<String>,
    channel_visibility: ChannelVisibilityStats,
    visible_log_lines: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let visible_log_lines = visible_log_lines.max(1);
    let mut lines = Vec::new();

    // Header line: visible vs. permission-hidden channels for the active
    // scope. Helps the user diagnose "why is this channel missing" without
    // diving into the logs.
    let visibility_text = format!(
        "Channels: {} visible · {} hidden by permissions",
        channel_visibility.visible, channel_visibility.hidden,
    );
    lines.push(Line::from(Span::styled(
        visibility_text,
        Style::default().fg(ACCENT),
    )));
    lines.push(Line::from(Span::raw(String::new())));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "No errors recorded in this process.",
            Style::default().fg(DIM),
        )));
    } else {
        let wrapped = entries
            .into_iter()
            .flat_map(|entry| wrap_text_lines(&entry, width))
            .collect::<Vec<_>>();
        let start = wrapped.len().saturating_sub(visible_log_lines);
        for entry in wrapped.into_iter().skip(start) {
            lines.push(Line::from(Span::styled(
                entry,
                Style::default().fg(Color::Red),
            )));
        }
    }
    lines.push(Line::from(Span::raw(String::new())));
    lines.push(Line::from(Span::styled(
        "Showing current-process ERROR logs only · ` / Esc close",
        Style::default().fg(DIM),
    )));
    lines
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

fn channel_action_menu_lines(actions: &[ChannelActionItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let marker = if index == selected { "› " } else { "  " };
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
                Span::styled(action.label.clone(), style),
            ])
        })
        .collect();
    lines.push(Line::from(Span::styled(
        "Enter select · Esc close",
        Style::default().fg(DIM),
    )));
    lines
}

fn channel_thread_menu_lines(threads: &[ChannelThreadItem], selected: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = if threads.is_empty() {
        vec![Line::from(Span::styled(
            "  (no threads)".to_owned(),
            Style::default().fg(DIM),
        ))]
    } else {
        threads
            .iter()
            .enumerate()
            .map(|(index, thread)| {
                let marker = if index == selected { "› " } else { "  " };
                let mut suffix = String::new();
                if thread.archived {
                    suffix.push_str(" [archived]");
                }
                if thread.locked {
                    suffix.push_str(" [locked]");
                }
                let mut style = Style::default();
                if index == selected {
                    style = style
                        .bg(Color::Rgb(40, 45, 90))
                        .add_modifier(Modifier::BOLD);
                }
                Line::from(vec![
                    Span::styled(marker, Style::default().fg(ACCENT)),
                    Span::styled(format!("» {}", thread.label), style),
                    Span::styled(suffix, Style::default().fg(DIM)),
                ])
            })
            .collect()
    };
    lines.push(Line::from(Span::styled(
        "Enter open · Esc back",
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
    let visible_range =
        super::selection::visible_item_range(reactions.len(), selected, visible_items);

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
    let visible_range =
        super::selection::visible_item_range(reactions.len(), selected, visible_items);
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
    let typing_height: u16 = state.typing_footer_for_selected_channel().is_some().into();
    let [list, typing, composer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(typing_height),
        Constraint::Length(composer_height),
    ])
    .areas(area);
    MessageAreas {
        list,
        typing,
        composer,
    }
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
        .enumerate()
        .take(message_index.saturating_add(1))
        .map(|(local_idx, message)| {
            let global = state.message_scroll().saturating_add(local_idx);
            state.message_base_line_count_for_width(message, content_width)
                + state.message_extra_top_lines(global)
        })
        .sum::<usize>()
        .saturating_add(previous_preview_rows)
        .saturating_add(message_index.saturating_mul(MESSAGE_ROW_GAP))
        .saturating_sub(1);
    row as isize - line_offset as isize
}

fn inline_image_preview_area(
    list: Rect,
    row: isize,
    preview_height: u16,
    accent_color: Option<u32>,
) -> Option<Rect> {
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

    let gutter_width = accent_color
        .map(|_| EMBED_PREVIEW_GUTTER_PREFIX.width() as u16)
        .unwrap_or(0);
    let x = list
        .x
        .saturating_add(content_offset)
        .saturating_add(gutter_width);

    Some(Rect {
        x,
        y: u16::try_from(visible_top).ok()?,
        width: list
            .width
            .saturating_sub(content_offset)
            .saturating_sub(gutter_width),
        height: u16::try_from(visible_bottom - visible_top).ok()?,
    })
}

fn composer_height(area: Rect, state: &DashboardState) -> u16 {
    let content_lines = if state.is_composing() || !state.composer_input().is_empty() {
        composer_content_line_count(state, composer_inner_width(area.width))
    } else {
        1
    };
    MIN_MESSAGE_INPUT_HEIGHT.max(content_lines.saturating_add(2))
}

fn composer_inner_width(width: u16) -> u16 {
    width.saturating_sub(2).max(1)
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::discord::ids::{Id, marker::MessageMarker};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier, Style},
    };
    use unicode_width::UnicodeWidthStr;

    use super::{
        ACCENT, DIM, DISCORD_EPOCH_MILLIS, MemberEntry, SNOWFLAKE_TIMESTAMP_SHIFT,
        channel_name_style, composer_content_line_count, composer_lines,
        composer_prompt_line_count, composer_text, date_separator_line, debug_log_popup_lines,
        emoji_reaction_picker_lines, footer_hint, format_message_sent_time,
        format_unix_millis_with_offset, highlight_style, inline_image_preview_area,
        inline_image_preview_row, member_display_label, member_name_style,
        message_action_menu_lines, message_author_style, message_item_lines,
        message_starts_new_day, message_viewport_lines, poll_vote_picker_lines,
        reaction_users_popup_lines, reaction_users_visible_line_count, sync_view_heights,
        user_profile_display_name_style, user_profile_popup_lines, user_profile_popup_text,
        user_profile_popup_visible_lines,
    };
    use crate::{
        discord::{
            AppEvent, AttachmentInfo, ChannelInfo, ChannelRecipientState, ChannelState,
            ChannelVisibilityStats, EmbedInfo, FriendStatus, GuildMemberState, MemberInfo,
            MentionInfo, MessageInfo, MessageKind, MessageSnapshotInfo, MessageState,
            MutualGuildInfo, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
            ReactionUserInfo, ReactionUsersInfo, ReplyInfo, RoleInfo, UserProfileInfo,
        },
        tui::{
            format::{TextHighlightKind, truncate_display_width},
            message_format::{
                MessageContentLine, format_message_content, format_message_content_lines,
                lay_out_reaction_chips, mention_highlight_style, poll_box_border,
                poll_card_inner_width, wrap_text_lines,
            },
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

        assert_eq!(state.message_view_height(), 12);
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
    fn message_author_style_is_bold_white() {
        let style = message_author_style(None);

        assert_eq!(style.fg, Some(Color::White));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn message_author_style_uses_role_color_when_available() {
        let style = message_author_style(Some(0x3366CC));

        assert_eq!(style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn message_viewport_author_uses_resolved_role_color() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(99);
        let role_id = Id::new(100);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
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
                permission_overwrites: Vec::new(),
            }],
            members: vec![MemberInfo {
                user_id: author_id,
                display_name: "neo".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![role_id],
            }],
            presences: vec![(author_id, PresenceStatus::Online)],
            roles: vec![RoleInfo {
                id: role_id,
                name: "Blue".to_owned(),
                color: Some(0x3366CC),
                position: 10,
                hoist: false,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(1),
            author_id,
            author: "fallback".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages();
        let lines = message_viewport_lines(&messages, None, &state, 40, 80, &[]);

        assert_eq!(
            lines[0].spans[1].style.fg,
            Some(Color::Rgb(0x33, 0x66, 0xCC))
        );
    }

    #[test]
    fn history_message_author_uses_channel_guild_for_role_color() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(99);
        let role_id = Id::new(100);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
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
                permission_overwrites: Vec::new(),
            }],
            members: vec![MemberInfo {
                user_id: author_id,
                display_name: "neo".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![role_id],
            }],
            presences: vec![(author_id, PresenceStatus::Online)],
            roles: vec![RoleInfo {
                id: role_id,
                name: "Blue".to_owned(),
                color: Some(0x3366CC),
                position: 10,
                hoist: false,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                guild_id: None,
                channel_id,
                message_id: Id::new(1),
                author_id,
                author: "fallback".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                pinned: false,
                reactions: Vec::new(),
                content: Some("hello".to_owned()),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            }],
        });

        let messages = state.messages();
        let lines = message_viewport_lines(&messages, None, &state, 40, 80, &[]);

        assert_eq!(
            lines[0].spans[1].style.fg,
            Some(Color::Rgb(0x33, 0x66, 0xCC))
        );
    }

    #[test]
    fn user_profile_name_style_uses_presence_color() {
        let style = user_profile_display_name_style(PresenceStatus::DoNotDisturb);

        assert_eq!(style.fg, Some(Color::Red));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn user_profile_popup_styles_name_by_status() {
        let profile = user_profile_info(10, "neo");
        let state = DashboardState::new();

        let lines = user_profile_popup_lines(&profile, &state, 40, PresenceStatus::Idle, None);

        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(180, 140, 0)));
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn user_profile_popup_visible_lines_follow_selected_mutual_server() {
        let mut profile = user_profile_info(10, "neo");
        profile.mutual_guilds = (1_u64..=12)
            .map(|id| MutualGuildInfo {
                guild_id: Id::new(id),
                nick: None,
            })
            .collect();
        let state = DashboardState::new();
        let popup_text =
            user_profile_popup_text(&profile, &state, 40, PresenceStatus::Online, Some(10));

        let visible =
            user_profile_popup_visible_lines(popup_text.lines, popup_text.selected_line, 6);
        let texts = line_texts_from_ratatui(&visible);

        assert!(texts.iter().any(|line| line == "› • guild-11"));
        assert!(!texts.iter().any(|line| line == "  • guild-1"));
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
    fn message_content_lines_render_discord_embed_preview() {
        let mut message = message_with_content(Some(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
        ));
        message.embeds = vec![youtube_embed()];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

        assert_eq!(
            line_texts(&lines),
            vec![
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "  ▎ YouTube",
                "  ▎ Example Video",
            ]
        );
        assert_eq!(lines[1].style.fg, Some(DIM));
        assert!(lines[2].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[2].style.fg, Some(Color::Blue));
        let marker_spans = lines[1].spans();
        assert_eq!(marker_spans[0].content.as_ref(), "  ▎ ");
        assert_eq!(marker_spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert!(
            !marker_spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn message_embed_hides_media_and_player_urls() {
        let mut message = message_with_content(Some("watch this".to_owned()));
        let mut embed = youtube_embed();
        embed.video_url = Some("https://www.youtube.com/embed/dQw4w9WgXcQ".to_owned());
        message.embeds = vec![embed];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

        assert_eq!(
            line_texts(&lines),
            vec![
                "watch this",
                "  ▎ YouTube",
                "  ▎ Example Video",
                "  ▎ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ]
        );
    }

    #[test]
    fn message_embed_url_underline_skips_marker() {
        let mut message = message_with_content(Some("watch this".to_owned()));
        let mut embed = youtube_embed();
        embed.description = None;
        embed.image_url = None;
        message.embeds = vec![embed];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 80);
        let url_spans = lines[3].spans();

        assert_eq!(
            line_texts(&lines),
            vec![
                "watch this",
                "  ▎ YouTube",
                "  ▎ Example Video",
                "  ▎ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ]
        );
        assert_eq!(url_spans[0].content.as_ref(), "  ▎ ");
        assert_eq!(url_spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert!(
            !url_spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(
            url_spans[1].content.as_ref(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert!(
            url_spans[1]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn message_embed_does_not_repeat_body_url() {
        let mut message = message_with_content(Some(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned(),
        ));
        let mut embed = youtube_embed();
        embed.title = None;
        embed.description = None;
        embed.image_url = None;
        message.embeds = vec![embed];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

        assert_eq!(
            line_texts(&lines),
            vec!["https://www.youtube.com/watch?v=dQw4w9WgXcQ", "  ▎ YouTube"]
        );
    }

    #[test]
    fn message_content_preserves_explicit_newlines() {
        let message = message_with_content(Some("hello\nworld".to_owned()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello", "world"]);
    }

    #[test]
    fn message_content_wraps_long_lines_to_content_width() {
        let message = message_with_content(Some("abcdefghijkl".to_owned()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 5);

        assert_eq!(line_texts(&lines), vec!["abcde", "fghij", "kl"]);
    }

    #[test]
    fn message_content_wraps_wide_characters_by_terminal_width() {
        let message = message_with_content(Some("漢字仮名交じ".to_owned()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 10);

        assert_eq!(line_texts(&lines), vec!["漢字仮名交", "じ"]);
    }

    #[test]
    fn message_content_renders_known_user_mentions() {
        let message = message_with_content(Some("hello <@10>".to_owned()));
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["hello @alice"]);
    }

    #[test]
    fn message_content_keeps_unknown_user_mentions_raw() {
        let message = message_with_content(Some("hello <@10>".to_owned()));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello <@10>"]);
    }

    #[test]
    fn message_content_renders_mentions_from_message_metadata() {
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "alice")];

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert_eq!(line_texts(&lines), vec!["hello @alice"]);
    }

    #[test]
    fn message_content_highlights_current_user_mentions() {
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "username")];
        let mut state = state_with_member(10, "server alias");
        state.push_event(AppEvent::Ready {
            user: "server alias".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @server alias", ""]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@server alias");
        assert_eq!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn message_content_highlights_other_user_mentions_with_softer_color() {
        // Discord still paints non-self mentions, just with a calmer tint than
        // the gold "you" highlight, so the user can tell whether they were the
        // one being pinged at a glance.
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "alice")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @alice", ""]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@alice");
        assert_eq!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::OtherMention).bg
        );
        assert_ne!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg,
            "other-user mentions must not look like a self-mention notification"
        );
    }

    #[test]
    fn message_content_highlights_everyone_mentions_for_current_user() {
        let message = message_with_content(Some("ping @everyone".to_owned()));
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   ping @everyone", ""]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
        assert_eq!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn message_content_highlights_mixed_everyone_and_direct_mentions_in_order() {
        let mut message = message_with_content(Some("@everyone hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "neo")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   @everyone hello @neo", ""]
        );
        assert_eq!(lines[1].spans[1].content.as_ref(), "@everyone");
        assert_eq!(lines[1].spans[3].content.as_ref(), "@neo");
        assert_eq!(
            lines[1].spans[1].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
        assert_eq!(
            lines[1].spans[3].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn message_content_highlights_here_mentions_for_current_user() {
        let message = message_with_content(Some("ping @here".to_owned()));
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   ping @here", ""]
        );
        assert_eq!(lines[1].spans[2].content.as_ref(), "@here");
        assert_eq!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn mention_like_display_name_does_not_duplicate_highlight_spans() {
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "everyone")];
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "everyone".to_owned(),
            user_id: Some(Id::new(10)),
        });

        let lines = message_item_lines(
            message.author.clone(),
            message_author_style(None),
            "00:00".to_owned(),
            format_message_content_lines(&message, &state, 200),
            40,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   hello @everyone", ""]
        );
        assert_eq!(lines[1].spans.len(), 3);
        assert_eq!(lines[1].spans[2].content.as_ref(), "@everyone");
        assert_eq!(
            lines[1].spans[2].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn message_content_prefers_cached_member_alias_over_mention_metadata() {
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info(10, "username")];
        let state = state_with_member(10, "server alias");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["hello @server alias"]);
    }

    #[test]
    fn message_content_prefers_message_mention_nick_over_cached_member_name() {
        let mut message = message_with_content(Some("hello <@10>".to_owned()));
        message.mentions = vec![mention_info_with_nick(10, "server alias")];
        let state = state_with_member(10, "username");

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
        let message = message_with_content(Some("one\n\nthree".to_owned()));

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
        let mut message = message_with_content(Some(String::new()));
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
            let mut message = message_with_content(Some(String::new()));
            message.message_kind = MessageKind::new(kind);

            let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

            assert_eq!(line_texts(&lines), vec![label]);
            assert_eq!(lines[0].style, Style::default().fg(ACCENT));
        }
    }

    #[test]
    fn thread_created_message_uses_cached_thread_details() {
        let mut message = message_with_content(Some("release notes".to_owned()));
        message.message_kind = MessageKind::new(18);
        message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(10 * 60 * 1000));
        let latest_thread_message_id =
            snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
        let mut state = DashboardState::new();
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            parent_id: Some(message.channel_id),
            position: None,
            last_message_id: Some(latest_thread_message_id),
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let lines = format_message_content_lines(&message, &state, 200);
        let texts = line_texts(&lines);

        assert_eq!(texts[0], "neo started release notes thread.");
        assert!(texts[1].starts_with("  ╭"));
        assert!(texts[2].starts_with("  │ release notes"));
        assert!(texts[2].contains("12 messages"));
        assert!(texts[3].contains("2 minutes ago"));
        assert!(texts[4].starts_with("  ╰"));
        assert_eq!(lines[0].style, Style::default().fg(Color::White));
        assert_eq!(lines[3].style, Style::default().fg(DIM));
    }

    #[test]
    fn thread_created_message_uses_cached_thread_message_when_last_id_missing() {
        let now = current_unix_millis();
        let mut message = message_with_content(Some("release notes".to_owned()));
        message.message_kind = MessageKind::new(18);
        message.id = snowflake_for_unix_ms(now.saturating_sub(10 * 60 * 1000));
        let latest_thread_message_id = snowflake_for_unix_ms(now.saturating_sub(2 * 60 * 1000));
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
            permission_overwrites: Vec::new(),
        }));
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            message_id: latest_thread_message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("latest reply".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let lines = format_message_content_lines(&message, &state, 200);
        let texts = line_texts(&lines);

        assert!(texts[2].contains("13 messages"));
        assert!(texts[3].contains("neo latest reply 2 minutes ago"));
    }

    #[test]
    fn thread_created_message_falls_back_to_system_message_time() {
        let mut message = message_with_content(Some("release notes".to_owned()));
        message.message_kind = MessageKind::new(18);
        message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
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
            permission_overwrites: Vec::new(),
        }));

        let lines = format_message_content_lines(&message, &state, 200);
        let texts = line_texts(&lines);

        assert!(texts[2].contains("12 messages"));
        assert!(texts[3].contains("2 minutes ago"));
    }

    #[test]
    fn thread_created_message_keeps_archived_and_locked_metadata() {
        let mut message = message_with_content(Some("release notes".to_owned()));
        message.message_kind = MessageKind::new(18);
        message.id = snowflake_for_unix_ms(current_unix_millis().saturating_sub(2 * 60 * 1000));
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
            thread_archived: Some(true),
            thread_locked: Some(true),
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let lines = format_message_content_lines(&message, &state, 200);

        assert!(line_texts(&lines)[3].contains("archived · locked"));
    }

    #[test]
    fn thread_starter_message_uses_referenced_message_card() {
        let mut message = message_with_content(Some(String::new()));
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
        let mut message = message_with_content(Some(String::new()));
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
        let mut message = message_with_content(Some("asdf".to_owned()));
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "neo".to_owned(),
            content: Some("hello <@10>".to_owned()),
            mentions: Vec::new(),
        });
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 200);

        assert_eq!(line_texts(&lines), vec!["╭─ neo : hello @alice", "asdf"]);
    }

    #[test]
    fn reply_preview_renders_mentions_from_reply_metadata() {
        let mut message = message_with_content(Some("asdf".to_owned()));
        message.message_kind = MessageKind::new(19);
        message.reply = Some(ReplyInfo {
            author: "neo".to_owned(),
            content: Some("hello <@10>".to_owned()),
            mentions: vec![mention_info(10, "alice")],
        });

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
        let mut message = message_with_content(Some(String::new()));
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
        let mut message = message_with_content(Some(String::new()));
        message.poll = Some(poll_info(true));

        let lines = format_message_content_lines(&message, &DashboardState::new(), 200);

        assert!(lines[2].text.starts_with("│ Select one or more answers"));
        assert_eq!(lines[2].style, Style::default().fg(DIM));
    }

    #[test]
    fn poll_message_places_body_inside_box() {
        let mut message = message_with_content(Some("Please vote <@10>".to_owned()));
        message.poll = Some(poll_info(false));
        let state = state_with_member(10, "alice");

        let lines = format_message_content_lines(&message, &state, 40);

        assert_eq!(lines[1].text, poll_test_line("What should we eat?", 40));
        assert_eq!(lines[2].text, poll_test_line("Please vote @alice", 40));
        assert!(lines[3].text.starts_with("│ Select one answer"));
    }

    #[test]
    fn poll_message_body_highlights_mentions_inside_box() {
        let mut message = message_with_content(Some("<@10> please vote".to_owned()));
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
        assert_eq!(
            spans[1].style.bg,
            mention_highlight_style(TextHighlightKind::SelfMention).bg
        );
    }

    #[test]
    fn message_content_renders_reaction_chips_below_message() {
        let mut message = message_with_content(Some("hello".to_owned()));
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

        let layout = lay_out_reaction_chips(&reactions, 200);

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

        let layout = lay_out_reaction_chips(&reactions, 200);

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
        let layout = lay_out_reaction_chips(&reactions, 14);

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
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
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
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
            })
            .expect("second draw");
        for _ in 0..6 {
            state.scroll_reaction_users_popup_up();
        }
        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), &mut state);
                super::render(frame, &state, Vec::new(), Vec::new(), Vec::new(), None);
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
        let trailing_matches = dump.iter().filter(|line| line.contains("? )")).count();
        assert!(
            trailing_matches <= 1,
            "popup buffer contained '? )' fragment on {trailing_matches} rows; expected at most 1.\nDump:\n{}",
            dump.join("\n")
        );
    }

    #[test]
    fn reaction_users_popup_buffer_stays_clean_in_narrow_terminal() {
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
        let dump = render_dashboard_dump(40, 25, &mut state);

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
    fn footer_hint_switches_for_debug_log_popup() {
        let mut state = DashboardState::new();
        state.toggle_debug_log_popup();

        assert_eq!(footer_hint(&state), "`/esc close debug logs");
    }

    #[test]
    fn debug_log_popup_shows_recent_errors() {
        let lines = debug_log_popup_lines(
            vec![
                "1 [ERROR] first: old".to_owned(),
                "2 [ERROR] second: recent".to_owned(),
            ],
            ChannelVisibilityStats {
                visible: 12,
                hidden: 3,
            },
            1,
            80,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "Channels: 12 visible · 3 hidden by permissions",
                "",
                "2 [ERROR] second: recent",
                "",
                "Showing current-process ERROR logs only · ` / Esc close"
            ]
        );
    }

    #[test]
    fn debug_log_popup_has_empty_state() {
        let lines = debug_log_popup_lines(Vec::new(), ChannelVisibilityStats::default(), 5, 80);

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                "Channels: 0 visible · 0 hidden by permissions",
                "",
                "No errors recorded in this process.",
                "",
                "Showing current-process ERROR logs only · ` / Esc close"
            ]
        );
    }

    #[test]
    fn debug_log_popup_wraps_long_detail_lines() {
        let lines = debug_log_popup_lines(
            vec!["42 [ERROR] history: load message history failed: Discord HTTP request failed; detail=Discord returned HTTP 403; api_error=Missing Access; response_body_bytes=99".to_owned()],
            ChannelVisibilityStats::default(),
            4,
            44,
        );
        let texts = line_texts_from_ratatui(&lines);
        let joined = texts.join("");

        assert!(
            joined.contains("detail=Discord returned HTTP 403"),
            "expected wrapped debug popup line to preserve HTTP detail: {texts:?}"
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
            permission_overwrites: Vec::new(),
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
                permission_overwrites: Vec::new(),
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
    fn forwarded_snapshot_renders_discord_embed_preview() {
        let mut snapshot = forwarded_snapshot(
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Vec::new(),
        );
        snapshot.embeds = vec![youtube_embed()];
        let message = message_with_forwarded_snapshot(snapshot);

        let lines = format_message_content_lines(&message, &DashboardState::new(), 80);

        assert_eq!(
            line_texts(&lines),
            vec![
                "↱ Forwarded",
                "│ https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "│   ▎ YouTube",
                "│   ▎ Example Video",
            ]
        );
        let url_spans = lines[2].spans();
        assert_eq!(url_spans[0].content.as_ref(), "│ ");
        assert!(
            !url_spans[0]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(url_spans[1].content.as_ref(), "  ▎ ");
        assert_eq!(url_spans[1].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert!(
            !url_spans[1]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn image_preview_rows_are_part_of_the_message_item() {
        let lines = message_item_lines(
            "neo".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("look".to_owned())],
            14,
            3,
            None,
            0,
        );

        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn embed_image_preview_rows_continue_embed_gutter() {
        let lines = message_item_lines(
            "neo".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("look".to_owned())],
            14,
            2,
            Some(0xff0000),
            0,
        );

        assert_eq!(line_texts_from_ratatui(&lines)[2], "     ▎ ");
        assert_eq!(lines[2].spans[1].style.fg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn text_only_message_item_has_header_and_content_rows() {
        let lines = message_item_lines(
            "neo".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("look".to_owned())],
            14,
            0,
            None,
            0,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["oo neo 00:00", "   look", ""]
        );
    }

    #[test]
    fn message_item_lines_can_start_after_line_offset() {
        let lines = message_item_lines(
            "neo".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![
                MessageContentLine::plain("first".to_owned()),
                MessageContentLine::plain("second".to_owned()),
                MessageContentLine::plain("third".to_owned()),
            ],
            14,
            0,
            None,
            2,
        );

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec!["   second", "   third", ""]
        );
    }

    #[test]
    fn message_item_header_uses_display_width_for_wide_author() {
        let ascii = message_item_lines(
            "bruised8".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("plain text".to_owned())],
            14,
            0,
            None,
            0,
        );
        let wide = message_item_lines(
            "漢字名".to_owned(),
            message_author_style(None),
            "00:00".to_owned(),
            vec![MessageContentLine::plain("plain text".to_owned())],
            14,
            0,
            None,
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
            username: None,
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
    fn server_label_truncates_by_display_width() {
        let label = truncate_display_width("漢字仮名交じりサーバー", 12);

        assert_eq!(label, "漢字仮名...");
        assert!(label.width() <= 12);
    }

    #[test]
    fn channel_label_truncates_by_display_width_after_prefixes() {
        let branch_prefix = "├ ";
        let channel_prefix = "# ";
        let max_width = 14usize;
        let label_width = max_width
            .saturating_sub(branch_prefix.width())
            .saturating_sub(channel_prefix.width());
        let label = truncate_display_width("漢字仮名交じり", label_width);

        assert_eq!(label, "漢字仮...");
        assert!(branch_prefix.width() + channel_prefix.width() + label.width() <= max_width);
    }

    #[test]
    fn offline_member_name_keeps_role_color_and_dims() {
        let member = GuildMemberState {
            user_id: Id::new(10),
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
            status: PresenceStatus::Offline,
        };

        let style = member_name_style(MemberEntry::Guild(&member), Some(0x3366CC), false);

        assert_eq!(style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn no_role_member_name_stays_white_for_online_like_statuses() {
        for status in [
            PresenceStatus::Online,
            PresenceStatus::Idle,
            PresenceStatus::DoNotDisturb,
        ] {
            let member = GuildMemberState {
                user_id: Id::new(10),
                display_name: "neo".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
                status,
            };

            let style = member_name_style(MemberEntry::Guild(&member), None, false);

            assert_eq!(style.fg, Some(Color::White));
            assert!(!style.add_modifier.contains(Modifier::DIM));
        }
    }

    #[test]
    fn no_role_offline_member_name_is_white_and_dimmed() {
        let member = GuildMemberState {
            user_id: Id::new(10),
            display_name: "neo".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
            status: PresenceStatus::Offline,
        };

        let style = member_name_style(MemberEntry::Guild(&member), None, false);

        assert_eq!(style.fg, Some(Color::White));
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn selected_bot_member_name_preserves_role_color_and_selection_style() {
        let member = GuildMemberState {
            user_id: Id::new(10),
            display_name: "bot".to_owned(),
            username: None,
            is_bot: true,
            avatar_url: None,
            role_ids: Vec::new(),
            status: PresenceStatus::Online,
        };

        let style = member_name_style(MemberEntry::Guild(&member), Some(0x3366CC), true);

        assert_eq!(style.fg, Some(Color::Rgb(0x33, 0x66, 0xCC)));
        assert_eq!(style.bg, Some(Color::Rgb(24, 54, 65)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn message_sent_time_formats_with_timezone_offset() {
        let kst = chrono::FixedOffset::east_opt(9 * 60 * 60).expect("KST offset should be valid");

        assert_eq!(
            format_unix_millis_with_offset(DISCORD_EPOCH_MILLIS, kst),
            Some("09:00".to_owned())
        );
    }

    fn snowflake_for_unix_ms(unix_ms: u64) -> Id<MessageMarker> {
        let raw = (unix_ms - DISCORD_EPOCH_MILLIS) << SNOWFLAKE_TIMESTAMP_SHIFT;
        Id::new(raw.max(1))
    }

    fn current_unix_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_millis()
            .try_into()
            .expect("current unix millis should fit in u64")
    }

    #[test]
    fn date_separator_appears_when_local_date_changes() {
        // 24h apart at noon UTC guarantees different local dates regardless of
        // the test runner's timezone.
        let day_one = snowflake_for_unix_ms(1_743_465_600_000); // 2026-04-01 00:00:00 UTC + 12h ≈ noon
        let day_two = snowflake_for_unix_ms(1_743_465_600_000 + 24 * 60 * 60 * 1000);

        assert!(!message_starts_new_day(day_one, None));
        assert!(!message_starts_new_day(day_one, Some(day_one)));
        assert!(message_starts_new_day(day_two, Some(day_one)));
    }

    #[test]
    fn date_separator_line_centers_label_within_full_width() {
        let id = snowflake_for_unix_ms(1_743_508_800_000); // arbitrary timestamp
        let line = date_separator_line(id, 30);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text.width(), 30);
        assert!(text.contains(' '));
        assert!(text.starts_with('─'));
        assert!(text.ends_with('─'));
        // The label is "YYYY-MM-DD" wrapped in spaces, so 12 chars.
        let label_chars = text.matches(char::is_numeric).count();
        assert_eq!(label_chars, 8);
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
            message_viewport_lines(&messages, Some(0), &DashboardState::new(), 5, 80, &[])
                .into_iter()
                .take(5)
                .collect::<Vec<_>>();
        let visible_text = line_texts_from_ratatui(&visible_rows);
        let sent_time = format_message_sent_time(Id::new(1));

        assert!(visible_text[0].starts_with("oo "));
        assert!(visible_text[0].ends_with(&sent_time));
        assert!(visible_text[1].ends_with("selected"));
        assert_eq!(visible_text[2], "");
        assert!(visible_text[3].starts_with("oo "));
        assert!(visible_text[3].ends_with(&sent_time));
        assert!(visible_text[4].ends_with("abcdefgh"));
    }

    #[test]
    fn selected_message_highlight_skips_avatar_column() {
        let message = message_with_content(Some("abcdefghijkl".to_owned()));
        let messages = [&message];

        let lines = message_viewport_lines(&messages, Some(0), &DashboardState::new(), 5, 80, &[]);
        let sent_time = format_message_sent_time(Id::new(1));

        assert_eq!(
            line_texts_from_ratatui(&lines),
            vec![
                format!("oo . {sent_time}"),
                "   abcdefgh".to_owned(),
                "   ijkl".to_owned(),
                "".to_owned(),
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
            inline_image_preview_area(area, 2, 4, None),
            Some(Rect::new(13, 8, 77, 4))
        );
    }

    #[test]
    fn embed_image_preview_area_leaves_room_for_gutter() {
        let area = Rect::new(10, 5, 80, 12);

        assert_eq!(
            inline_image_preview_area(area, 2, 4, Some(0xff0000)),
            Some(Rect::new(17, 8, 73, 4))
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

        assert_eq!(row, 14);
        assert_eq!(
            inline_image_preview_area(area, row, 4, None),
            Some(Rect::new(13, 20, 77, 3))
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
            inline_image_preview_area(area, 3, 4, None),
            Some(Rect::new(13, 9, 77, 2))
        );
    }

    #[test]
    fn inline_image_preview_area_clips_preview_at_list_top() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(
            inline_image_preview_area(area, -2, 4, None),
            Some(Rect::new(13, 5, 77, 3))
        );
    }

    #[test]
    fn inline_image_preview_area_returns_none_when_preview_starts_below_list() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(inline_image_preview_area(area, 5, 4, None), None);
    }

    #[test]
    fn inline_image_preview_area_returns_none_when_preview_ends_above_list() {
        let area = Rect::new(10, 5, 80, 6);

        assert_eq!(inline_image_preview_area(area, -5, 4, None), None);
    }

    fn render_dashboard_dump(width: u16, height: u16, state: &mut DashboardState) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| {
                sync_view_heights(frame.area(), state);
                super::render(frame, state, Vec::new(), Vec::new(), Vec::new(), None);
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|col| buffer[(col, row)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
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
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }

    fn message_with_content(content: Option<String>) -> MessageState {
        let mut message = message_with_attachment(content, image_attachment());
        message.attachments.clear();
        message
    }

    fn youtube_embed() -> EmbedInfo {
        EmbedInfo {
            color: Some(0xff0000),
            provider_name: Some("YouTube".to_owned()),
            author_name: None,
            title: Some("Example Video".to_owned()),
            description: Some("A video description".to_owned()),
            fields: Vec::new(),
            footer_text: None,
            url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
            thumbnail_width: Some(480),
            thumbnail_height: Some(360),
            image_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
            image_width: Some(480),
            image_height: Some(360),
            video_url: None,
        }
    }

    fn state_with_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
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
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
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
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
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
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
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
            embeds: Vec::new(),
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
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn state_with_member(user_id: u64, display_name: &str) -> DashboardState {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GuildCreate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: vec![member_info(user_id, display_name)],
            presences: vec![(Id::new(user_id), PresenceStatus::Online)],
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state
    }

    fn member_info(user_id: u64, display_name: &str) -> MemberInfo {
        MemberInfo {
            user_id: Id::new(user_id),
            display_name: display_name.to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        }
    }

    fn user_profile_info(user_id: u64, username: &str) -> UserProfileInfo {
        UserProfileInfo {
            user_id: Id::new(user_id),
            username: username.to_owned(),
            global_name: None,
            guild_nick: None,
            role_ids: Vec::new(),
            avatar_url: None,
            bio: None,
            pronouns: None,
            mutual_guilds: Vec::<MutualGuildInfo>::new(),
            mutual_friends_count: 0,
            friend_status: FriendStatus::None,
            note: None,
        }
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: None,
            display_name: display_name.to_owned(),
        }
    }

    fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: Some(nick.to_owned()),
            display_name: nick.to_owned(),
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
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    status: *status,
                })
                .collect(),
            permission_overwrites: Vec::new(),
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
