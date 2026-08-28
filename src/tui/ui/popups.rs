use super::activity::{ActivityLeading, build_activity_render};
use super::message::list::render_image_preview;
use super::*;
use crate::discord::ActivityKind;
use crate::tui::selection;
use crate::tui::state::SCROLL_OFF;
use crate::tui::text::format_byte_size;
use ratatui::layout::Position;

mod action_menu;
mod attachment_viewer;
mod channel_switcher;
mod confirmation;
mod debug_log;
mod downloads;
mod folder_settings;
mod forum_post;
mod keymap;
mod notification_inbox;
mod options;
mod polls;
mod profile;
mod reactions;
mod search;
mod stream_info;
mod thread_edit;
mod toast;
mod url_picker;
mod voice_participant_audio;

const POPUP_GAUGE_WIDTH: usize = 28;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PopupScrollMetrics {
    position: usize,
    viewport_len: usize,
    content_len: usize,
}

/// The rendered item rows for the active selectable popup. State owns the
/// selected item, while this UI plan owns terminal geometry and variable row
/// heights. Paging, rendering, scrollbars, and hit testing all derive from the
/// same plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SelectablePopupLayout {
    pub(super) target: SelectablePopupTarget,
    pub(super) popup: Rect,
    pub(super) list: Rect,
    pub(super) scroll: usize,
    row_items: Vec<Option<usize>>,
}

impl SelectablePopupLayout {
    fn new(
        target: SelectablePopupTarget,
        popup: Rect,
        list: Rect,
        snapshot: SelectablePopupSnapshot,
        rows_from_start: impl Fn(usize, usize) -> Vec<Option<usize>>,
    ) -> Self {
        let max_rows = usize::from(list.height).max(1);
        let (scroll, row_items) = popup_rows_with_visible_selection(
            snapshot.item_count,
            snapshot.selected,
            snapshot.scroll,
            max_rows,
            rows_from_start,
        );
        Self {
            target,
            popup,
            list,
            scroll,
            row_items,
        }
    }

    fn fixed(
        target: SelectablePopupTarget,
        popup: Rect,
        snapshot: SelectablePopupSnapshot,
    ) -> Self {
        let inner = panel_block("", false).inner(popup);
        let list = Rect {
            width: inner.width.saturating_sub(1).max(1),
            ..inner
        };
        Self::new(target, popup, list, snapshot, |start, max_rows| {
            (start..snapshot.item_count.min(start.saturating_add(max_rows)))
                .map(Some)
                .collect()
        })
    }

    pub(super) fn visible_items(&self) -> usize {
        self.row_items
            .iter()
            .flatten()
            .copied()
            .fold((None, 0usize), |(previous, count), item| {
                if previous == Some(item) {
                    (previous, count)
                } else {
                    (Some(item), count + 1)
                }
            })
            .1
            .max(1)
    }

    pub(super) fn item_at(&self, column: u16, row: u16) -> Option<usize> {
        let inside = column >= self.list.x
            && column < self.list.x.saturating_add(self.list.width)
            && row >= self.list.y
            && row < self.list.y.saturating_add(self.list.height);
        if !inside {
            return None;
        }
        self.row_items
            .get(usize::from(row.saturating_sub(self.list.y)))
            .copied()
            .flatten()
    }
}

fn popup_rows_with_visible_selection(
    item_count: usize,
    selected: usize,
    preferred_start: usize,
    max_rows: usize,
    rows_from_start: impl Fn(usize, usize) -> Vec<Option<usize>>,
) -> (usize, Vec<Option<usize>>) {
    if item_count == 0 {
        return (0, Vec::new());
    }

    let selected = selected.min(item_count - 1);
    let mut start = preferred_start.min(item_count - 1).min(selected);
    let mut rows = rows_from_start(start, max_rows);
    while !rows.iter().flatten().any(|item| *item == selected) && start < selected {
        start += 1;
        rows = rows_from_start(start, max_rows);
    }

    // At the end of a list, fill otherwise empty rows without hiding the
    // selection. This keeps the window stable and avoids a blank tail.
    while start > 0
        && rows.len() < max_rows
        && rows.iter().flatten().next_back().copied() == Some(item_count - 1)
    {
        let candidate = rows_from_start(start - 1, max_rows);
        if !candidate.iter().flatten().any(|item| *item == selected) {
            break;
        }
        start -= 1;
        rows = candidate;
    }

    // Match pane navigation's scrolloff, but count logical items rather than
    // terminal rows. This keeps the same breathing room for grouped and
    // variable-height lists without making their geometry a state concern.
    while let Some((before, after, visible)) = popup_visible_item_counts(&rows, selected) {
        let scrolloff = SCROLL_OFF.min(visible.saturating_sub(1) / 2);
        if before < scrolloff && start > 0 {
            let candidate = rows_from_start(start - 1, max_rows);
            if popup_visible_item_counts(&candidate, selected)
                .is_some_and(|(candidate_before, _, _)| candidate_before > before)
            {
                start -= 1;
                rows = candidate;
                continue;
            }
        }
        if after < scrolloff && start < selected {
            let candidate = rows_from_start(start + 1, max_rows);
            if popup_visible_item_counts(&candidate, selected)
                .is_some_and(|(_, candidate_after, _)| candidate_after > after)
            {
                start += 1;
                rows = candidate;
                continue;
            }
        }
        break;
    }

    (start, rows)
}

fn popup_visible_item_counts(
    rows: &[Option<usize>],
    selected: usize,
) -> Option<(usize, usize, usize)> {
    let mut items = Vec::new();
    for item in rows.iter().flatten().copied() {
        if items.last().copied() != Some(item) {
            items.push(item);
        }
    }
    let position = items.iter().position(|item| *item == selected)?;
    Some((
        position,
        items.len().saturating_sub(position + 1),
        items.len(),
    ))
}

pub(super) use action_menu::{
    action_menu_area, key_sequence_hint_area_for_state, render_channel_action_menu,
    render_guild_action_menu, render_key_sequence_hint, render_member_action_menu,
    render_message_action_menu, render_thread_action_menu,
};
#[cfg(test)]
pub(super) use action_menu::{
    channel_action_menu_lines_for_test, message_action_menu_lines,
    message_action_menu_lines_with_keymap_options,
};
#[cfg(test)]
pub(super) use attachment_viewer::centered_viewer_preview_area;
pub(super) use attachment_viewer::render_attachment_viewer;
#[cfg(test)]
pub(super) use channel_switcher::{channel_switcher_cursor_position, channel_switcher_lines};
pub(super) use channel_switcher::{
    channel_switcher_list_layout, channel_switcher_popup_area, render_channel_switcher_popup,
};
pub(super) use confirmation::{
    guild_leave_confirmation_popup_area_for_state, long_message_confirmation_popup_area_for_state,
    message_confirmation_popup_area_for_state, quit_confirmation_popup_area,
    render_guild_leave_confirmation, render_long_message_confirmation, render_message_confirmation,
    render_notification_inbox_mark_all_confirmation, render_quit_confirmation,
    render_thread_delete_confirmation, thread_delete_confirmation_popup_area_for_state,
};
#[cfg(test)]
pub(super) use confirmation::{
    long_message_confirmation_lines_for_test, message_delete_confirmation_lines,
    message_pin_confirmation_lines, message_remove_embeds_confirmation_lines,
    quit_confirmation_lines,
};
#[cfg(test)]
pub(super) use debug_log::debug_log_popup_lines;
pub(super) use debug_log::{debug_log_popup_area_for_state, render_debug_log_popup};
#[cfg(test)]
pub(super) use downloads::downloads_popup_lines;
pub(super) use downloads::{
    downloads_popup_area, downloads_popup_line_count, render_downloads_popup,
};
#[cfg(test)]
pub(super) use folder_settings::folder_settings_input_line_for_test;
pub(super) use folder_settings::{folder_settings_popup_area, render_folder_settings_popup};
pub(super) use forum_post::{
    forum_post_composer_metrics, forum_post_composer_popup_area, forum_post_tag_picker_list_layout,
    render_forum_post_composer, render_forum_post_tag_picker,
};
#[cfg(test)]
pub(super) use keymap::keymap_help_popup_lines;
pub(super) use keymap::{
    keymap_popup_area, keymap_popup_text_area, keymap_popup_total_lines, render_keymap_help_popup,
};
pub(super) use notification_inbox::{
    notification_inbox_list_layout, notification_inbox_popup_area, render_notification_inbox_popup,
};
#[cfg(test)]
pub(super) use options::options_popup_lines;
pub(super) use options::{options_popup_area, options_popup_list_layout, render_options_popup};
#[cfg(test)]
pub(super) use polls::poll_vote_picker_lines;
pub(super) use polls::{poll_vote_picker_popup_area, render_poll_vote_picker};
pub(super) use profile::{
    render_user_profile_popup, user_profile_picker_list_layout, user_profile_popup_has_avatar,
    user_profile_popup_metrics, user_profile_popup_text_geometry,
};
pub(in crate::tui) use profile::{user_profile_popup_area, user_profile_popup_avatar_viewport};
#[cfg(test)]
pub(super) use profile::{
    user_profile_popup_lines, user_profile_popup_lines_with_activities, user_profile_popup_text,
};
#[cfg(test)]
pub(super) use reactions::{
    emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
    emoji_reaction_picker_lines_with_own_reactions, filtered_emoji_reaction_picker_lines,
    reaction_list_lines_with_ready_urls, reaction_users_popup_lines,
};
pub(super) use reactions::{
    emoji_reaction_picker_list_layout, emoji_reaction_picker_popup_area_for_state,
    reaction_users_list_layout, reaction_users_popup_area_for_state, render_emoji_reaction_picker,
    render_reaction_users_popup,
};
pub(super) use search::{
    render_search_popup, search_popup_area_for_state, search_popup_list_layout,
};
pub(super) use stream_info::{render_stream_info, stream_info_area, stream_info_lines_for_area};
#[cfg(test)]
pub(super) use stream_info::{stream_info_lines, stream_info_lines_for_width};
pub(super) use thread_edit::{
    render_thread_edit, render_thread_edit_tag_picker, thread_edit_metrics, thread_edit_popup_area,
    thread_edit_tag_picker_list_layout,
};
#[cfg(test)]
pub(super) use toast::toast_line;
pub(super) use toast::{render_toast, toast_area};
#[cfg(test)]
pub(super) use url_picker::message_url_picker_lines_for_width;
pub(super) use url_picker::{message_url_picker_popup_area, render_message_url_picker};
pub(super) use voice_participant_audio::{
    render_voice_participant_audio_popup, voice_participant_audio_list_layout,
    voice_participant_audio_popup_area,
};

pub(super) fn active_selectable_popup_layout(
    area: Rect,
    state: &DashboardState,
) -> Option<SelectablePopupLayout> {
    let snapshot = state.active_selectable_popup_snapshot()?;
    Some(match snapshot.target {
        SelectablePopupTarget::MessageActions
        | SelectablePopupTarget::GuildActions
        | SelectablePopupTarget::ChannelActions
        | SelectablePopupTarget::MemberActions
        | SelectablePopupTarget::ThreadActions => SelectablePopupLayout::fixed(
            snapshot.target,
            action_menu_area(area, snapshot.item_count),
            snapshot,
        ),
        SelectablePopupTarget::MessageUrls => SelectablePopupLayout::fixed(
            snapshot.target,
            message_url_picker_popup_area(area, snapshot.item_count),
            snapshot,
        ),
        SelectablePopupTarget::Options => options_popup_list_layout(area, state, snapshot),
        SelectablePopupTarget::UserProfileStatus | SelectablePopupTarget::UserProfileActivity => {
            user_profile_picker_list_layout(area, state, snapshot)
        }
        SelectablePopupTarget::EmojiReactions => {
            emoji_reaction_picker_list_layout(area, state, snapshot)
        }
        SelectablePopupTarget::PollVotes => SelectablePopupLayout::fixed(
            snapshot.target,
            poll_vote_picker_popup_area(area, snapshot.item_count),
            snapshot,
        ),
        SelectablePopupTarget::ReactionList => reaction_users_list_layout(area, snapshot),
        SelectablePopupTarget::ChannelSwitcher => {
            channel_switcher_list_layout(area, state, snapshot)
        }
        SelectablePopupTarget::NotificationInbox => {
            notification_inbox_list_layout(area, state, snapshot)
        }
        SelectablePopupTarget::ForumPostTags => forum_post_tag_picker_list_layout(area, snapshot),
        SelectablePopupTarget::ThreadEditTags => thread_edit_tag_picker_list_layout(area, snapshot),
        SelectablePopupTarget::VoiceParticipantAudio => {
            voice_participant_audio_list_layout(area, snapshot)
        }
        SelectablePopupTarget::SearchResults | SelectablePopupTarget::SearchSuggestions => {
            search_popup_list_layout(area, state, snapshot)
        }
    })
}

fn popup_gauge_spacer() -> Span<'static> {
    Span::raw(" ".repeat(POPUP_GAUGE_WIDTH))
}

fn popup_gauge_line(
    x_offset: u16,
    min_label: &str,
    max_label: String,
    style: Style,
) -> Line<'static> {
    let leading_space = usize::from(x_offset).saturating_sub(min_label.len().saturating_add(1));
    Line::from(vec![
        Span::styled(format!("{}{min_label} ", " ".repeat(leading_space)), style),
        popup_gauge_spacer(),
        Span::styled(format!(" {max_label}"), style),
    ])
}

struct PopupGauge {
    x_offset: u16,
    width_margin: u16,
    y: u16,
    value: u16,
    maximum: u16,
    style: Style,
}

fn render_popup_gauge(frame: &mut Frame, inner: Rect, gauge: PopupGauge) {
    let gauge_width = inner
        .width
        .saturating_sub(gauge.width_margin)
        .min(POPUP_GAUGE_WIDTH as u16);
    if gauge_width == 0 {
        return;
    }
    let ratio = if gauge.maximum == 0 {
        0.0
    } else {
        f64::from(gauge.value) / f64::from(gauge.maximum)
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .label("")
            .gauge_style(gauge.style),
        Rect::new(
            inner.x.saturating_add(gauge.x_offset),
            gauge.y,
            gauge_width,
            1,
        ),
    );
}

pub(super) fn background_media_occlusion_areas(
    frame_area: Rect,
    state: &DashboardState,
) -> Vec<Rect> {
    let mut areas = Vec::new();

    if state.is_folder_settings_open() {
        areas.push(folder_settings_popup_area(frame_area));
    }
    if let Some(area) = active_modal_popup_area(frame_area, state) {
        areas.push(area);
    }
    if state.is_key_sequence_active() {
        areas.push(key_sequence_hint_area_for_state(frame_area, state));
    }

    let downloads = state.attachment_downloads();
    if !downloads.is_empty() {
        areas.push(downloads_popup_area(
            frame_area,
            downloads_popup_line_count(downloads.len()),
        ));
    }
    if let Some(toast) = state.toast_message() {
        areas.push(toast_area(frame_area, toast.text));
    }
    let members = dashboard_areas(frame_area, state).members;
    let stream_lines = stream_info_lines_for_area(state, members);
    if !stream_lines.is_empty() {
        areas.push(stream_info_area(members, &stream_lines));
    }

    areas.into_iter().filter(|area| !area.is_empty()).collect()
}

fn active_modal_popup_area(frame_area: Rect, state: &DashboardState) -> Option<Rect> {
    let kind = state.active_modal_popup_kind()?;
    match kind {
        ActiveModalPopupKind::MessageActionMenu => {
            let actions = state.selected_message_action_items();
            (!actions.is_empty()).then(|| action_menu_area(frame_area, actions.len()))
        }
        ActiveModalPopupKind::GuildActionMenu => {
            let count = state.guild_action_row_count();
            (count > 0).then(|| action_menu_area(frame_area, count))
        }
        ActiveModalPopupKind::ChannelActionMenu => {
            let count = state.channel_action_row_count();
            (count > 0).then(|| action_menu_area(frame_area, count))
        }
        ActiveModalPopupKind::MemberActionMenu => {
            let count = state.selected_member_action_items().len();
            (count > 0).then(|| action_menu_area(frame_area, count))
        }
        ActiveModalPopupKind::MessageUrlPicker => {
            let urls = state.selected_message_url_items();
            (!urls.is_empty()).then(|| message_url_picker_popup_area(frame_area, urls.len()))
        }
        ActiveModalPopupKind::MessageConfirmation => {
            message_confirmation_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::LongMessageConfirmation => {
            long_message_confirmation_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::QuitConfirmation => Some(quit_confirmation_popup_area(frame_area)),
        ActiveModalPopupKind::GuildLeaveConfirmation => {
            guild_leave_confirmation_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::ThreadDeleteConfirmation => {
            thread_delete_confirmation_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::Options => Some(options_popup_area(frame_area, state)),
        ActiveModalPopupKind::AttachmentViewer => Some(attachment_viewer_popup(
            frame_area,
            state.attachment_viewer_zoom(),
        )),
        ActiveModalPopupKind::UserProfile => Some(user_profile_popup_area(frame_area)),
        ActiveModalPopupKind::EmojiReactionPicker => {
            emoji_reaction_picker_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::PollVotePicker => state
            .poll_vote_picker_items()
            .filter(|answers| !answers.is_empty())
            .map(|answers| poll_vote_picker_popup_area(frame_area, answers.len())),
        ActiveModalPopupKind::ReactionUsers => {
            reaction_users_popup_area_for_state(frame_area, state)
        }
        ActiveModalPopupKind::DebugLog => Some(debug_log_popup_area_for_state(frame_area, state)),
        ActiveModalPopupKind::KeymapHelp => Some(keymap_popup_area(frame_area)),
        ActiveModalPopupKind::ChannelSwitcher => Some(channel_switcher_popup_area(frame_area)),
        ActiveModalPopupKind::NotificationInbox => Some(notification_inbox_popup_area(frame_area)),
        ActiveModalPopupKind::Search => search_popup_area_for_state(frame_area, state),
        ActiveModalPopupKind::ForumPostComposer => Some(forum_post_composer_popup_area(frame_area)),
        ActiveModalPopupKind::ThreadEdit => Some(thread_edit_popup_area(frame_area)),
        ActiveModalPopupKind::ThreadActionMenu => {
            let count = state.thread_action_row_count();
            (count > 0).then(|| action_menu_area(frame_area, count))
        }
        ActiveModalPopupKind::VoiceParticipantAudio => {
            Some(voice_participant_audio_popup_area(frame_area))
        }
    }
}

/// Clears the popup area, draws the standard focused panel border, and
/// returns the inner content rect. Every modal popup opens with this
/// sequence and then renders its content into the returned rect.
fn render_modal_frame(frame: &mut Frame, popup: Rect, title: impl Into<String>) -> Rect {
    clear_area(frame, popup);
    let block = modal_block_owned(title.into());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
}

/// Shared geometry for modal forms whose actions stay visible while the form
/// body scrolls. Keeping this calculation in one place prevents rendering and
/// scroll metrics from disagreeing about the usable viewport.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PopupFormAreas {
    pub(super) content: Rect,
    pub(super) footer: Rect,
}

pub(super) fn popup_form_areas(popup: Rect) -> PopupFormAreas {
    popup_form_areas_with_footer_height(popup, 3)
}

pub(super) fn popup_form_areas_with_footer_height(
    popup: Rect,
    footer_height: u16,
) -> PopupFormAreas {
    let inner = Block::default().borders(Borders::ALL).inner(popup);
    let footer_height = inner.height.min(footer_height);
    PopupFormAreas {
        content: Rect {
            height: inner.height.saturating_sub(footer_height),
            ..inner
        },
        footer: Rect {
            y: inner
                .y
                .saturating_add(inner.height.saturating_sub(footer_height)),
            height: footer_height,
            ..inner
        },
    }
}

/// Draws the common form frame with an optional destination or item label on
/// the right. The context is omitted on narrow terminals instead of colliding
/// with the main title.
fn render_popup_form_frame(
    frame: &mut Frame,
    popup: Rect,
    title: &str,
    context: &str,
) -> PopupFormAreas {
    render_popup_form_frame_with_footer_height(frame, popup, title, context, 3)
}

fn render_popup_form_frame_with_footer_height(
    frame: &mut Frame,
    popup: Rect,
    title: &str,
    context: &str,
    footer_height: u16,
) -> PopupFormAreas {
    clear_area(frame, popup);
    let mut block = modal_block_owned(title.to_owned());
    let required_width = title
        .width()
        .saturating_add(context.width())
        .saturating_add(8);
    if !context.is_empty() && usize::from(popup.width) >= required_width {
        block = block.title(
            Line::from(Span::styled(
                format!(" {context} "),
                theme::current().style(theme::HighlightGroup::MessageSecondary),
            ))
            .right_aligned(),
        );
    }
    frame.render_widget(block, popup);
    popup_form_areas_with_footer_height(popup, footer_height)
}

/// Renders the always-visible actions shared by create and edit forms. Each
/// action gets its own row so focus and hierarchy stay clear at every width.
struct PopupFormActions {
    cancel_active: bool,
    primary_shortcut: &'static str,
    primary_label: &'static str,
    primary_active: bool,
}

fn render_popup_form_footer(frame: &mut Frame, area: Rect, actions: PopupFormActions) {
    if area.is_empty() {
        return;
    }

    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::TOP)
        .border_type(theme.border_type(theme::BorderSurface::Modal))
        .border_style(theme.style(theme::HighlightGroup::ModalBorder));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    let lines = vec![
        popup_button_line_with_style(
            actions.primary_shortcut,
            actions.primary_label,
            actions.primary_active,
            theme::current().style(theme::HighlightGroup::Strong),
        ),
        popup_button_line("c", "Cancel", actions.cancel_active),
    ];
    let lines = truncate_popup_lines(lines, usize::from(inner.width));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_modal_paragraph(
    frame: &mut Frame,
    popup: Rect,
    title: impl Into<String>,
    lines: Vec<Line<'static>>,
) {
    let inner = render_modal_frame(frame, popup, title);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Renders a one-row-per-item popup list with the shared selection viewport.
/// Keeping clipping and the scrollbar here prevents individual list popups from
/// showing page navigation while leaving the selected row off-screen.
fn render_selectable_popup_list(
    frame: &mut Frame,
    popup: Rect,
    title: impl Into<String>,
    lines: Vec<Line<'static>>,
    scroll: usize,
) {
    let inner = render_modal_frame(frame, popup, title);
    let viewport_len = usize::from(inner.height).max(1);
    let range = selection::visible_window(scroll, viewport_len, lines.len());
    let content = Rect {
        width: inner.width.saturating_sub(1).max(1),
        ..inner
    };
    let width = usize::from(content.width).max(1);
    let visible_lines = lines[range.clone()]
        .iter()
        .cloned()
        .map(|line| truncate_line_to_display_width(line, width))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(visible_lines).wrap(Wrap { trim: false }),
        content,
    );
    render_vertical_scrollbar(frame, inner, range.start, viewport_len, lines.len());
}

fn popup_shortcut_help_text(items: &[(&str, &str)]) -> String {
    items
        .iter()
        .map(|(shortcut, description)| format!("[{shortcut}] {description}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn popup_button_line(shortcut: &str, label: &'static str, active: bool) -> Line<'static> {
    popup_button_line_with_style(shortcut, label, active, Style::default())
}

fn popup_danger_button_line(shortcut: &str, label: &'static str, active: bool) -> Line<'static> {
    popup_button_line_with_style(
        shortcut,
        label,
        active,
        theme::current().style(theme::HighlightGroup::Error),
    )
}

fn popup_button_line_with_style(
    shortcut: &str,
    label: &'static str,
    active: bool,
    label_style: Style,
) -> Line<'static> {
    let active_style = |style| {
        if active {
            theme::current().apply(theme::HighlightGroup::ActiveField, style)
        } else {
            style
        }
    };
    Line::from(vec![
        Span::styled(
            editable_field_marker(active),
            active_style(Style::default()),
        ),
        Span::styled(
            format!("[{shortcut}] "),
            active_style(theme::current().style(theme::HighlightGroup::Shortcut)),
        ),
        Span::styled(label, active_style(label_style)),
    ])
}

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

fn truncate_popup_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| truncate_line_to_display_width(line, width))
        .collect()
}

fn wrapped_styled_popup_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from(Span::styled(String::new(), style))];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };

        if candidate.width() <= width {
            current = candidate;
            continue;
        }

        if !current.is_empty() {
            lines.push(Line::from(Span::styled(current, style)));
        }
        current = truncate_display_width(word, width);
    }

    if !current.is_empty() {
        lines.push(Line::from(Span::styled(current, style)));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(String::new(), style)));
    }
    lines
}

fn push_wrapped_styled_popup_text(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    style: Style,
) {
    lines.extend(wrapped_styled_popup_lines(text, width, style));
}

fn selectable_popup_marker(selected: bool) -> Span<'static> {
    selection_marker(selected)
}

fn editable_field_marker(active: bool) -> &'static str {
    if active { "› " } else { "  " }
}

fn editable_field_label_style(active: bool, editing: bool) -> Style {
    if editing {
        theme::current().apply(
            theme::HighlightGroup::Strong,
            theme::current().style(theme::HighlightGroup::Editing),
        )
    } else if active {
        theme::current().style(theme::HighlightGroup::ActiveField)
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    }
}

fn editable_field_value_style(active: bool, editing: bool) -> Style {
    if editing {
        theme::current().style(theme::HighlightGroup::Editing)
    } else if active {
        theme::current().style(theme::HighlightGroup::ActiveField)
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    }
}

fn popup_form_section_heading(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {label}"),
        theme::current().style(theme::HighlightGroup::Heading),
    ))
}

fn popup_form_field_label_style(active: bool, editing: bool) -> Style {
    if editing {
        theme::current().apply(
            theme::HighlightGroup::Strong,
            theme::current().style(theme::HighlightGroup::Editing),
        )
    } else if active {
        theme::current().style(theme::HighlightGroup::ActiveField)
    } else {
        theme::current().style(theme::HighlightGroup::MessageSecondary)
    }
}

fn popup_form_field_value_style(active: bool, editing: bool) -> Style {
    if editing {
        theme::current().style(theme::HighlightGroup::Editing)
    } else if active {
        theme::current().style(theme::HighlightGroup::ActiveField)
    } else {
        Style::default()
    }
}

fn popup_form_field_label(label: &str, required: bool) -> String {
    if required {
        format!("{label} *")
    } else {
        label.to_owned()
    }
}

fn popup_form_text_field_line(
    label: &str,
    required: bool,
    value: &str,
    active: bool,
    editing: bool,
    width: usize,
) -> Line<'static> {
    let label = popup_form_field_label(label, required);
    let prefix = format!("{}{label}  ", editable_field_marker(active));
    let available = width.saturating_sub(prefix.width()).max(1);
    let content = Span::styled(
        truncate_display_width(value, available),
        popup_form_field_value_style(active, editing),
    );
    Line::from(vec![
        Span::styled(prefix, popup_form_field_label_style(active, editing)),
        content,
    ])
}

fn popup_form_text_value_column(label: &str, required: bool, active: bool) -> usize {
    let label = popup_form_field_label(label, required);
    format!("{}{label}  ", editable_field_marker(active)).width()
}

fn popup_form_summary_line(
    label: &str,
    required: bool,
    value: &str,
    affordance: Option<&str>,
    active: bool,
    enabled: bool,
    width: usize,
) -> Line<'static> {
    let label = popup_form_field_label(label, required);
    let prefix = format!("{}{label}  ", editable_field_marker(active));
    let affordance = affordance.unwrap_or_default();
    let affordance_width = affordance.width();
    let value_width = width
        .saturating_sub(prefix.width())
        .saturating_sub(affordance_width)
        .saturating_sub(usize::from(!affordance.is_empty()))
        .max(1);
    let value = truncate_display_width(value, value_width);
    let padding = width
        .saturating_sub(prefix.width())
        .saturating_sub(value.width())
        .saturating_sub(affordance_width);
    let value_style = if enabled {
        popup_form_field_value_style(active, false)
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    };

    let mut spans = vec![
        Span::styled(prefix, popup_form_field_label_style(active, false)),
        Span::styled(value, value_style),
    ];
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    if !affordance.is_empty() {
        spans.push(Span::styled(
            affordance.to_owned(),
            if active && enabled {
                theme::current().style(theme::HighlightGroup::Shortcut)
            } else {
                theme::current().style(theme::HighlightGroup::Hint)
            },
        ));
    }
    truncate_line_to_display_width(Line::from(spans), width)
}

fn push_popup_form_inline_status(lines: &mut Vec<Line<'static>>, status: &str, width: usize) {
    push_wrapped_styled_popup_text(
        lines,
        &format!("  {status}"),
        width,
        theme::current().style(theme::HighlightGroup::Error),
    );
}

fn selectable_popup_shortcut_span(shortcut: impl Into<String>) -> Span<'static> {
    Span::styled(
        shortcut.into(),
        theme::current().style(theme::HighlightGroup::Shortcut),
    )
}

fn selectable_popup_label_style(selected: bool, enabled: bool) -> Style {
    let mut style = if enabled {
        Style::default()
    } else {
        theme::current().style(theme::HighlightGroup::Disabled)
    };
    if selected {
        style = theme::current().apply(theme::HighlightGroup::SelectedRow, style);
    }
    style
}

fn shortcut_prefix(shortcut: Option<char>) -> String {
    shortcut
        .map(|shortcut| format!("[{shortcut}] "))
        .unwrap_or_else(|| "    ".to_owned())
}
