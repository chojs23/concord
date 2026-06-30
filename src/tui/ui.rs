use crate::discord::ids::{Id, marker::MessageMarker};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use ratatui_image::{Image as RatatuiImage, Resize, StatefulImage};
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
use super::state::MemberEntry;
use super::{
    format::truncate_display_width,
    message::format::{
        EMOJI_REACTION_IMAGE_WIDTH, MessageContentLine, ReactionLayout, embed_color,
        format_message_content_lines_with_loaded_custom_emoji_urls,
        format_message_content_sections_with_loaded_custom_emoji_urls, format_message_relative_age,
        lay_out_reaction_chips_with_custom_emoji_images, reaction_line_spans, wrap_text_lines,
    },
    message::layout::MessageViewportPlan,
    state::{
        ActiveModalPopupKind, AppliedForumTag, AttachmentDownloadProgressView,
        AttachmentViewerItem, ChannelSwitcherItem, ChannelThreadItem, DashboardState,
        DisplayOptionItem, EmojiReactionItem, FocusPane, MessageActionItem, MessageUrlItem,
        PollVotePickerItem, SearchFieldView, SearchPopupMode, SearchPopupView, SearchResultItem,
        ThreadActionItem, discord_color, presence_color, presence_marker,
    },
};
use crate::discord::{
    ActivityInfo, ChannelState, ChannelUnreadState, ChannelVisibilityStats, FriendStatus,
    MessageState, PresenceStatus, ReactionInfo, ReactionUsersInfo, UserProfileInfo, is_thread_kind,
};

/// Discord's "you were mentioned" orange, `#FFA500`.
const MENTION_ORANGE: Color = Color::Rgb(255, 165, 0);

/// Explicit RGB instead of `Modifier::DIM` so CJK wide characters dim
/// uniformly with ASCII (most terminals ignore SGR dim on wide glyphs).
const READ_DIM: Color = Color::Rgb(130, 130, 130);

/// Explicit RGB instead of relying on `Modifier::BOLD` alone, which most
/// monospace fonts can't apply to CJK glyphs.
const UNREAD_BRIGHT: Color = Color::Reset;

pub(in crate::tui) const LOCAL_UPLOAD_PREVIEW_HEIGHT: u16 = 6;
pub(in crate::tui) const LOCAL_UPLOAD_PREVIEW_WIDTH: u16 = 32;

mod activity;
pub(in crate::tui) mod forum;
mod hit_test;
mod layout;
mod message;
mod panes;
mod popups;
mod types;

pub(crate) use self::hit_test::{focus_pane_at, mouse_target_at, user_profile_popup_contains};
use self::layout::{
    attachment_viewer_image_area, attachment_viewer_popup, centered_rect, dashboard_areas,
    inline_image_preview_area, inline_image_preview_height, inline_image_preview_width,
    message_areas, message_list_area, panel_scrollbar_area, reaction_users_visible_line_count,
    vertical_scrollbar_visible,
};
#[cfg(test)]
use self::layout::{composer_content_line_count, composer_prompt_line_count};
use self::message::list::{MessageMedia, render_messages};
#[cfg(test)]
use self::panes::{
    composer_cursor_position, composer_lines, composer_lines_with_loaded_custom_emoji_urls,
    composer_text, emoji_picker_lines, member_display_label, member_name_style,
    primary_activity_summary,
};
use self::panes::{render_channels, render_guilds, render_header, render_members};
use self::popups::{
    channel_switcher_visible_items, emoji_reaction_picker_visible_items_for_area,
    forum_post_composer_metrics, forum_post_composer_popup_area,
    forum_post_tag_picker_visible_items, keymap_popup_text_area, keymap_popup_total_lines,
    options_popup_visible_items, render_attachment_viewer, render_channel_switcher_popup,
    render_debug_log_popup, render_downloads_popup, render_emoji_reaction_picker,
    render_folder_settings_popup, render_forum_post_composer, render_forum_post_tag_picker,
    render_guild_leave_confirmation, render_keymap_help_popup, render_leader_popup,
    render_message_action_menu, render_message_confirmation, render_message_url_picker,
    render_notification_inbox_popup, render_options_popup, render_poll_vote_picker,
    render_quit_confirmation, render_reaction_users_popup, render_search_popup,
    render_thread_action_menu, render_thread_delete_confirmation, render_thread_edit,
    render_thread_edit_tag_picker, render_toast, render_user_profile_popup,
    search_popup_visible_items, thread_edit_metrics, thread_edit_popup_area,
    thread_edit_tag_picker_visible_items, user_profile_popup_has_avatar,
    user_profile_popup_text_geometry, user_profile_popup_total_lines,
};
use self::types::{
    ACCENT, DIM, EMBED_PREVIEW_GUTTER_PREFIX, MESSAGE_AVATAR_OFFSET, MESSAGE_AVATAR_PLACEHOLDER,
    MESSAGE_SELECTION_PREFIX_WIDTH, MessageViewportLayout, SCROLLBAR_THUMB,
    SELECTED_FORUM_POST_BORDER, SELECTED_MESSAGE_BORDER, UserProfilePopupText,
};
pub use self::types::{
    AvatarImage, EmojiImage, ImagePreview, ImagePreviewLayout, ImagePreviewState,
};
pub(crate) use self::types::{MouseTarget, PopupListTarget};
#[cfg(test)]
use self::{
    forum::{
        forum_post_reaction_summary, forum_post_scrollbar_visible_count,
        forum_post_tag_rows_for_test, forum_post_viewport_lines,
    },
    message::list::{
        date_separator_line, format_message_sent_time, inline_image_preview_row,
        message_author_style, message_body_custom_emoji_rows, message_item_lines,
        message_viewport_layout, message_viewport_lines, new_messages_notice_line,
        selected_avatar_x_offset, selected_message_card_width, selected_message_content_x_offset,
    },
    popups::{
        centered_viewer_preview_area, channel_switcher_cursor_position, channel_switcher_lines,
        debug_log_popup_lines, emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
        emoji_reaction_picker_lines_with_own_reactions, filtered_emoji_reaction_picker_lines,
        keymap_help_popup_lines, leader_action_lines_for_test, message_action_menu_lines,
        message_action_menu_lines_with_keymap_options, message_delete_confirmation_lines,
        message_pin_confirmation_lines, message_remove_embeds_confirmation_lines,
        message_url_picker_lines_for_width, options_popup_lines, poll_vote_picker_lines,
        quit_confirmation_lines, reaction_users_popup_lines, toast_area, toast_line,
        user_profile_popup_lines, user_profile_popup_lines_with_activities,
    },
};

pub(in crate::tui) use self::popups::user_profile_popup_area;
#[cfg(test)]
pub(in crate::tui::ui) use self::popups::{downloads_popup_area, downloads_popup_lines};
pub fn sync_view_heights(area: Rect, state: &mut DashboardState) {
    let areas = dashboard_areas(area, state);
    let guild_filter_row = usize::from(
        state.is_guild_pane_filter_active() && state.is_pane_visible(FocusPane::Guilds),
    );
    state.set_guild_view_height(
        visible_panel_content_height(
            areas.guilds,
            "Servers",
            state.is_pane_visible(FocusPane::Guilds),
        )
        .saturating_sub(guild_filter_row),
    );
    let channel_filter_row = usize::from(
        state.is_channel_pane_filter_active() && state.is_pane_visible(FocusPane::Channels),
    );
    state.set_channel_view_height(
        visible_panel_content_height(
            areas.channels,
            "Channels",
            state.is_pane_visible(FocusPane::Channels),
        )
        .saturating_sub(usize::from(state.is_pane_visible(FocusPane::Channels)))
        .saturating_sub(channel_filter_row),
    );
    state.set_message_view_height(message_list_area(areas.messages, state).height as usize);
    state.set_member_view_height(visible_panel_content_height(
        areas.members,
        "Members",
        state.is_pane_visible(FocusPane::Members),
    ));
    state.set_reaction_users_popup_view_height(reaction_users_visible_line_count(area));
    if state.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker) {
        let reaction_count = state
            .filtered_emoji_reaction_items_slice()
            .map(<[_]>::len)
            .unwrap_or(0);
        let has_filter = state.emoji_reaction_filter().is_some();
        let visible_items =
            emoji_reaction_picker_visible_items_for_area(area, reaction_count, has_filter);
        state.set_emoji_reaction_picker_view_height(visible_items);
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::Options) {
        let visible_items = options_popup_visible_items(area, state);
        state.set_options_popup_view_height(visible_items);
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::ChannelSwitcher) {
        state.set_channel_switcher_view_height(channel_switcher_visible_items(area));
    }
    if let Some(view) = state.search_popup_view() {
        let visible_items = search_popup_visible_items(area, &view);
        state.set_search_popup_view_height(visible_items);
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::UserProfile) {
        // The popup body shrinks when the avatar slot is in use, so use
        // the same has-avatar predicate the renderer uses to keep the
        // total-line / view-height pair consistent with what gets drawn.
        let has_avatar = user_profile_popup_has_avatar(
            area,
            state.show_avatars() && state.user_profile_popup_has_avatar_preview(),
        );
        let (text_width, text_height) = user_profile_popup_text_geometry(area, has_avatar);
        let total_lines = user_profile_popup_total_lines(state, text_width);
        state.set_user_profile_popup_view_height(text_height as usize);
        state.set_user_profile_popup_total_lines(total_lines);
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::KeymapHelp) {
        let inner = keymap_popup_text_area(area);
        let total_lines = keymap_popup_total_lines(state);
        state.set_keymap_popup_view_height(inner.height as usize);
        state.set_keymap_popup_total_lines(total_lines);
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::ForumPostComposer)
        && let Some(view) = state.forum_post_composer_view()
    {
        // Mirror the renderer's geometry: a 1-cell border plus a reserved
        // scrollbar column. Keep the laid-out height and the focus/cursor reveal
        // in lockstep with what `render_forum_post_composer` draws.
        let popup = forum_post_composer_popup_area(area);
        let viewport = usize::from(popup.height.saturating_sub(2));
        let content_width = usize::from(popup.width.saturating_sub(3)).max(1);
        let preview_count = state.forum_post_attachment_previews().len();
        let metrics = forum_post_composer_metrics(&view, content_width, preview_count);
        state.set_forum_post_composer_metrics(viewport, metrics.total_lines);
        state.reveal_forum_post_composer_rows(metrics.reveal_start, metrics.reveal_end);
        if state.is_forum_post_tag_picker_active() && !view.tags.is_empty() {
            let visible_items = forum_post_tag_picker_visible_items(area, view.tags.len());
            state.set_forum_post_tag_picker_view_height(visible_items);
        }
    }
    if state.is_active_modal_popup(ActiveModalPopupKind::ThreadEdit)
        && let Some(view) = state.thread_edit_view()
    {
        // Mirror the renderer's geometry: a 1-cell border plus a reserved
        // scrollbar column. Keep the laid-out height and the focus/cursor reveal
        // in lockstep with what `render_thread_edit` draws.
        let popup = thread_edit_popup_area(area);
        let viewport = usize::from(popup.height.saturating_sub(2));
        let content_width = usize::from(popup.width.saturating_sub(3)).max(1);
        let metrics = thread_edit_metrics(&view, content_width);
        state.set_thread_edit_metrics(viewport, metrics.total_lines);
        state.reveal_thread_edit_rows(metrics.reveal_start, metrics.reveal_end);
        if state.is_thread_edit_tag_picker_active() && !view.tags.is_empty() {
            let visible_items = thread_edit_tag_picker_visible_items(area, view.tags.len());
            state.set_thread_edit_tag_picker_view_height(visible_items);
        }
    }
}

pub fn image_preview_layout(area: Rect, state: &DashboardState) -> ImagePreviewLayout {
    let areas = dashboard_areas(area, state);
    let list = message_list_area(areas.messages, state);
    let viewer_image_area = attachment_viewer_image_area(area, state.attachment_viewer_zoom());
    ImagePreviewLayout {
        list_height: list.height as usize,
        content_width: message_content_width(list),
        preview_width: inline_image_preview_width(list),
        max_preview_height: inline_image_preview_height(list, true),
        viewer_preview_width: viewer_image_area.width,
        viewer_max_preview_height: viewer_image_area.height,
        font_size: None,
    }
}

#[cfg(test)]
pub fn render(
    frame: &mut Frame,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
    avatar_images: Vec<AvatarImage>,
    emoji_images: Vec<EmojiImage<'_>>,
    profile_avatar: Option<AvatarImage>,
) {
    render_with_message_viewport_plan(
        frame,
        state,
        image_previews,
        avatar_images,
        emoji_images,
        profile_avatar,
        None,
    );
}

pub(in crate::tui) fn render_with_message_viewport_plan(
    frame: &mut Frame,
    state: &DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
    avatar_images: Vec<AvatarImage>,
    emoji_images: Vec<EmojiImage<'_>>,
    profile_avatar: Option<AvatarImage>,
    message_viewport_plan: Option<&MessageViewportPlan<'_>>,
) {
    let areas = dashboard_areas(frame.area(), state);
    // Modal popups and menus center on the whole terminal rather than the
    // message pane, so they are not clipped to the chat column.
    let popup_area = frame.area();
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
    let media_occlusion_areas = background_media_occlusion_areas(frame.area(), state);
    render_messages(
        frame,
        areas.messages,
        state,
        MessageMedia {
            image_previews: inline_image_previews,
            avatar_images,
            emoji_images: &emoji_images,
            occlusion_areas: &media_occlusion_areas,
        },
        message_viewport_plan,
    );
    if state.is_pane_visible(FocusPane::Members) {
        render_members(frame, areas.members, state, &emoji_images);
    }
    render_leader_popup(frame, popup_area, state);
    render_channel_switcher_popup(frame, popup_area, state);
    render_notification_inbox_popup(frame, popup_area, state);
    render_message_action_menu(frame, popup_area, state);
    render_thread_action_menu(frame, popup_area, state);
    render_message_url_picker(frame, popup_area, state);
    render_message_confirmation(frame, popup_area, state);
    render_quit_confirmation(frame, popup_area, state);
    render_guild_leave_confirmation(frame, popup_area, state);
    render_thread_delete_confirmation(frame, popup_area, state);
    render_folder_settings_popup(frame, popup_area, state);
    render_options_popup(frame, popup_area, state);
    render_poll_vote_picker(frame, popup_area, state);
    render_user_profile_popup(frame, popup_area, state, profile_avatar, &emoji_images);
    render_emoji_reaction_picker(frame, popup_area, state, &emoji_images);
    render_reaction_users_popup(frame, popup_area, state);
    render_attachment_viewer(frame, frame.area(), state, viewer_image_preview);
    render_debug_log_popup(frame, popup_area, state);
    render_keymap_help_popup(frame, popup_area, state);
    render_search_popup(frame, popup_area, state);
    render_forum_post_composer(frame, popup_area, state);
    render_forum_post_tag_picker(frame, popup_area, state, &emoji_images);
    render_thread_edit(frame, popup_area, state);
    render_thread_edit_tag_picker(frame, popup_area, state, &emoji_images);
    render_downloads_popup(frame, frame.area(), state);
    render_toast(frame, frame.area(), state);
}

pub(in crate::tui) fn background_media_occlusion_areas(
    frame_area: Rect,
    state: &DashboardState,
) -> Vec<Rect> {
    self::popups::background_media_occlusion_areas(frame_area, state)
}

pub(in crate::tui) fn image_preview_list_area(area: Rect, state: &DashboardState) -> Rect {
    let areas = dashboard_areas(area, state);
    message_list_area(areas.messages, state)
}

pub(in crate::tui) fn inline_image_preview_screen_area(
    list: Rect,
    row: isize,
    preview_x_offset_columns: u16,
    preview_width: u16,
    preview_height: u16,
    accent_color: Option<u32>,
) -> Option<Rect> {
    inline_image_preview_area(
        list,
        row,
        preview_x_offset_columns,
        preview_width,
        preview_height,
        accent_color,
    )
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

fn channel_prefix(kind: &str) -> &'static str {
    match kind {
        "dm" | "Private" => "@ ",
        "group-dm" | "Group" => "● ",
        "voice" | "GuildVoice" => "🔈 ",
        "category" | "GuildCategory" => "▾ ",
        "forum" | "GuildForum" => "📝 ",
        kind if is_thread_kind(kind) => "» ",
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
        .title_style(Style::default().fg(Color::Reset).bold())
}

pub(super) fn panel_block_line(title: Line<'static>, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { Color::DarkGray };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border))
        .title_style(Style::default().fg(Color::Reset).bold())
}

#[cfg(test)]
mod tests;
