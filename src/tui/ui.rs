use crate::discord::ids::{Id, marker::MessageMarker};
use chrono::{DateTime, Local, NaiveDate};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};
use ratatui_image::{Image as RatatuiImage, Resize, StatefulImage};
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
use super::state::MemberEntry;
use super::{
    format::truncate_display_width,
    message_format::{
        EMOJI_REACTION_IMAGE_WIDTH, MessageContentLine, ReactionLayout, embed_color,
        format_message_content_lines_with_loaded_custom_emoji_urls,
        format_message_content_sections_with_loaded_custom_emoji_urls, format_message_relative_age,
        lay_out_reaction_chips_with_custom_emoji_images, reaction_line_spans, wrap_text_lines,
    },
    state::{
        ChannelSwitcherItem, ChannelThreadItem, DashboardState, DisplayOptionItem,
        EmojiReactionItem, FORUM_POST_CARD_HEIGHT, FocusPane, ImageViewerItem, MessageActionItem,
        PollVotePickerItem, channel_action_shortcut, discord_color, guild_action_shortcut,
        indexed_shortcut, member_action_shortcut, message_action_shortcut, presence_color,
        presence_marker,
    },
};
use crate::discord::{
    ActivityInfo, ChannelState, ChannelUnreadState, ChannelVisibilityStats, FriendStatus,
    MessageState, PresenceStatus, ReactionInfo, ReactionUsersInfo, UserProfileInfo,
};

/// Discord's "you were mentioned" orange, `#FFA500`.
const MENTION_ORANGE: Color = Color::Rgb(255, 165, 0);

/// Explicit RGB instead of `Modifier::DIM` so CJK wide characters dim
/// uniformly with ASCII (most terminals ignore SGR dim on wide glyphs).
const READ_DIM: Color = Color::Rgb(130, 130, 130);

/// Explicit RGB instead of relying on `Modifier::BOLD` alone, which most
/// monospace fonts can't apply to CJK glyphs.
const UNREAD_BRIGHT: Color = Color::Rgb(255, 255, 255);

mod activity;
mod forum;
mod interaction;
mod layout;
mod message_list;
mod panes;
mod popups;
mod types;

pub(crate) use self::interaction::{focus_pane_at, mouse_target_at, user_profile_popup_contains};
use self::layout::{
    centered_rect, dashboard_areas, image_viewer_image_area, image_viewer_popup,
    inline_image_preview_area, inline_image_preview_height, inline_image_preview_width,
    message_areas, message_list_area, panel_scrollbar_area, reaction_users_visible_line_count,
    vertical_scrollbar_visible,
};
#[cfg(test)]
use self::layout::{composer_content_line_count, composer_prompt_line_count};
pub(crate) use self::message_list::message_starts_new_day;
use self::message_list::render_messages;
#[cfg(test)]
use self::panes::{
    composer_cursor_position, composer_lines, composer_lines_with_loaded_custom_emoji_urls,
    composer_text, emoji_picker_lines, member_display_label, member_name_style,
    primary_activity_summary,
};
use self::panes::{render_channels, render_guilds, render_header, render_members};
use self::popups::{
    render_channel_switcher_popup, render_debug_log_popup, render_emoji_reaction_picker,
    render_image_viewer, render_leader_popup, render_message_action_menu, render_options_popup,
    render_poll_vote_picker, render_reaction_users_popup, render_user_profile_popup,
    user_profile_popup_has_avatar, user_profile_popup_text_geometry,
    user_profile_popup_total_lines,
};
use self::types::{
    ACCENT, DIM, DISCORD_EPOCH_MILLIS, EMBED_PREVIEW_GUTTER_PREFIX, MESSAGE_AVATAR_OFFSET,
    MESSAGE_AVATAR_PLACEHOLDER, MessageViewportLayout, SCROLLBAR_THUMB, SELECTED_FORUM_POST_BORDER,
    SELECTED_MESSAGE_BORDER, SELECTED_MESSAGE_CONTENT_OFFSET, SNOWFLAKE_TIMESTAMP_SHIFT,
    UserProfilePopupText,
};
pub(crate) use self::types::{ActionMenuTarget, MESSAGE_ROW_GAP, MouseTarget};
pub use self::types::{
    AvatarImage, EmojiImage, ImagePreview, ImagePreviewLayout, ImagePreviewState,
};
#[cfg(test)]
use self::{
    forum::{
        forum_post_reaction_summary, forum_post_scrollbar_visible_count, forum_post_viewport_lines,
    },
    message_list::{
        date_separator_line, format_message_sent_time, format_unix_millis_with_offset,
        inline_image_preview_row, message_author_style, message_item_lines,
        message_viewport_layout, message_viewport_lines, new_messages_notice_line,
        selected_avatar_x_offset, selected_message_card_width, selected_message_content_x_offset,
    },
    popups::{
        channel_switcher_cursor_position, channel_switcher_lines, debug_log_popup_lines,
        emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
        filtered_emoji_reaction_picker_lines, message_action_menu_lines, options_popup_lines,
        poll_vote_picker_lines, reaction_users_popup_lines, user_profile_popup_lines,
        user_profile_popup_lines_with_activities,
    },
};

pub fn sync_view_heights(area: Rect, state: &mut DashboardState) {
    let areas = dashboard_areas(area, state);
    state.set_guild_view_height(visible_panel_content_height(
        areas.guilds,
        "Servers",
        state.is_pane_visible(FocusPane::Guilds),
    ));
    state.set_channel_view_height(visible_panel_content_height(
        areas.channels,
        "Channels",
        state.is_pane_visible(FocusPane::Channels),
    ));
    state.set_message_view_height(message_list_area(areas.messages, state).height as usize);
    state.set_member_view_height(visible_panel_content_height(
        areas.members,
        "Members",
        state.is_pane_visible(FocusPane::Members),
    ));
    state.set_reaction_users_popup_view_height(reaction_users_visible_line_count(areas.messages));
    if state.is_user_profile_popup_open() {
        // The popup body shrinks when the avatar slot is in use, so use
        // the same has-avatar predicate the renderer uses to keep the
        // total-line / view-height pair consistent with what gets drawn.
        let has_avatar = user_profile_popup_has_avatar(
            areas.messages,
            state.show_avatars() && state.user_profile_popup_avatar_url().is_some(),
        );
        let (text_width, text_height) =
            user_profile_popup_text_geometry(areas.messages, has_avatar);
        let total_lines = user_profile_popup_total_lines(state, text_width);
        state.set_user_profile_popup_view_height(text_height as usize);
        state.set_user_profile_popup_total_lines(total_lines);
    }
}

pub fn image_preview_layout(area: Rect, state: &DashboardState) -> ImagePreviewLayout {
    let areas = dashboard_areas(area, state);
    let list = message_list_area(areas.messages, state);
    let viewer_image_area = image_viewer_image_area(areas.messages);
    ImagePreviewLayout {
        list_height: list.height as usize,
        content_width: message_content_width(list),
        preview_width: inline_image_preview_width(list),
        max_preview_height: inline_image_preview_height(list, true),
        viewer_preview_width: viewer_image_area.width,
        viewer_max_preview_height: viewer_image_area.height,
    }
}

pub fn render(
    frame: &mut Frame,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
    avatar_images: Vec<AvatarImage>,
    emoji_images: Vec<EmojiImage<'_>>,
    profile_avatar: Option<AvatarImage>,
) {
    let areas = dashboard_areas(frame.area(), state);
    let mut inline_image_previews = Vec::new();
    let mut viewer_image_preview = None;
    for image_preview in image_previews {
        if image_preview.viewer {
            viewer_image_preview = Some(image_preview);
        } else {
            inline_image_previews.push(image_preview);
        }
    }

    render_header(frame, areas.header, state);
    if state.is_pane_visible(FocusPane::Guilds) {
        render_guilds(frame, areas.guilds, state);
    }
    if state.is_pane_visible(FocusPane::Channels) {
        render_channels(frame, areas.channels, state);
    }
    render_messages(
        frame,
        areas.messages,
        state,
        inline_image_previews,
        avatar_images,
        &emoji_images,
    );
    if state.is_pane_visible(FocusPane::Members) {
        render_members(frame, areas.members, state, &emoji_images);
    }
    render_leader_popup(frame, areas.messages, state);
    render_channel_switcher_popup(frame, areas.messages, state);
    if !state.is_leader_action_mode() {
        render_message_action_menu(frame, areas.messages, state);
    }
    render_options_popup(frame, areas.messages, state);
    render_poll_vote_picker(frame, areas.messages, state);
    render_user_profile_popup(frame, areas.messages, state, profile_avatar, &emoji_images);
    render_emoji_reaction_picker(frame, areas.messages, state, emoji_images);
    render_reaction_users_popup(frame, areas.messages, state);
    render_image_viewer(frame, areas.messages, state, viewer_image_preview);
    render_debug_log_popup(frame, areas.messages, state);
}

fn message_content_width(list: Rect) -> usize {
    let padding = 4usize;
    (list.width as usize)
        .saturating_sub(padding)
        .saturating_sub(MESSAGE_AVATAR_OFFSET as usize)
        .max(8)
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

fn panel_content_height(area: Rect, title: &'static str) -> usize {
    panel_block(title, false).inner(area).height.max(1) as usize
}

fn visible_panel_content_height(area: Rect, title: &'static str, visible: bool) -> usize {
    if visible {
        panel_content_height(area, title)
    } else {
        0
    }
}

fn render_vertical_scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    viewport_len: usize,
    content_len: usize,
) {
    if !vertical_scrollbar_visible(area, viewport_len, content_len) {
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

/// Clamps the visible width of a `Line` to `max_width` columns by truncating
/// each contained span, then pads the remainder with explicit spaces so the
/// rendered line covers exactly `max_width` cells.
///
/// Truncation prevents `Paragraph` from wrapping a long line and bleeding the
/// continuation onto adjacent rows. Padding to the full width ensures every
/// cell in the popup row is painted by `Paragraph`. Windows Terminal under WSL
/// does not always clear the right-hand cell of a wide grapheme such as Korean
/// text or emoji when ratatui's diff sends a default-style space via `Clear`.
/// Writing an explicit styled space through the paragraph fixes the residue.
fn channel_prefix(kind: &str) -> &'static str {
    match kind {
        "dm" | "Private" => "@ ",
        "group-dm" | "Group" => "● ",
        "voice" | "GuildVoice" => "🔊 ",
        "category" | "GuildCategory" => "▾ ",
        "forum" | "GuildForum" => "▣ ",
        "thread" | "GuildPublicThread" | "GuildPrivateThread" | "GuildNewsThread" => "» ",
        _ => "# ",
    }
}

fn dm_presence_dot_span(channel: &ChannelState) -> Option<Span<'static>> {
    let status = one_to_one_dm_recipient_status(channel)?;
    Some(Span::styled(
        format!("{} ", presence_marker(status)),
        Style::default().fg(presence_color(status)),
    ))
}

/// Active channels skip decoration because the highlight bar handles them and
/// the activate-time ack clears their unread state anyway.
fn channel_unread_decoration(
    unread: ChannelUnreadState,
    base: Style,
    active: bool,
) -> (Option<Span<'static>>, Style) {
    if active {
        return (None, base);
    }
    match unread {
        ChannelUnreadState::Mentioned(count) => {
            let style = base.fg(MENTION_ORANGE).add_modifier(Modifier::BOLD);
            (Some(Span::styled(format!("({count}) "), style)), style)
        }
        ChannelUnreadState::Notified(count) => {
            let style = base.fg(UNREAD_BRIGHT).add_modifier(Modifier::BOLD);
            (Some(Span::styled(format!("({count}) "), style)), style)
        }
        ChannelUnreadState::Unread => (None, base.fg(UNREAD_BRIGHT).add_modifier(Modifier::BOLD)),
        ChannelUnreadState::Seen => (None, base.fg(READ_DIM)),
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

pub(super) fn panel_block_line(title: Line<'static>, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { Color::DarkGray };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border))
        .title_style(Style::default().fg(Color::White).bold())
}

#[cfg(test)]
mod tests;
