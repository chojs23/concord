use super::activity::{ActivityLeading, build_activity_render};
use super::message::list::render_image_preview;
use super::*;
use crate::discord::ActivityKind;
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
mod thread_edit;
mod toast;
mod url_picker;

pub(super) use action_menu::{
    action_menu_area, leader_popup_area_for_state, render_channel_action_menu,
    render_guild_action_menu, render_leader_popup, render_member_action_menu,
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
    channel_switcher_item_index_at, channel_switcher_popup_area, channel_switcher_visible_items,
    render_channel_switcher_popup,
};
pub(super) use confirmation::{
    guild_leave_confirmation_popup_area_for_state, message_confirmation_popup_area_for_state,
    quit_confirmation_popup_area, render_guild_leave_confirmation, render_message_confirmation,
    render_notification_inbox_mark_all_confirmation, render_quit_confirmation,
    render_thread_delete_confirmation, thread_delete_confirmation_popup_area_for_state,
};
#[cfg(test)]
pub(super) use confirmation::{
    message_delete_confirmation_lines, message_pin_confirmation_lines,
    message_remove_embeds_confirmation_lines, quit_confirmation_lines,
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
    forum_post_composer_metrics, forum_post_composer_popup_area,
    forum_post_tag_picker_visible_items, render_forum_post_composer, render_forum_post_tag_picker,
};
#[cfg(test)]
pub(super) use keymap::keymap_help_popup_lines;
pub(super) use keymap::{
    keymap_popup_area, keymap_popup_text_area, keymap_popup_total_lines, render_keymap_help_popup,
};
pub(super) use notification_inbox::{
    notification_inbox_popup_area, render_notification_inbox_popup,
};
#[cfg(test)]
pub(super) use options::options_popup_lines;
pub(super) use options::{options_popup_area, options_popup_visible_items, render_options_popup};
#[cfg(test)]
pub(super) use polls::poll_vote_picker_lines;
pub(super) use polls::{poll_vote_picker_popup_area, render_poll_vote_picker};
pub(in crate::tui) use profile::user_profile_popup_area;
pub(super) use profile::{
    render_user_profile_popup, user_profile_popup_has_avatar, user_profile_popup_text_geometry,
    user_profile_popup_total_lines,
};
#[cfg(test)]
pub(super) use profile::{user_profile_popup_lines, user_profile_popup_lines_with_activities};
#[cfg(test)]
pub(super) use reactions::{
    emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
    emoji_reaction_picker_lines_with_own_reactions, filtered_emoji_reaction_picker_lines,
    reaction_list_lines_with_ready_urls, reaction_users_popup_lines,
};
pub(super) use reactions::{
    emoji_reaction_picker_popup_area_for_state, emoji_reaction_picker_visible_items_for_area,
    reaction_users_popup_area_for_state, render_emoji_reaction_picker, render_reaction_users_popup,
};
pub(super) use search::{
    render_search_popup, search_popup_area_for_state, search_popup_visible_items,
};
pub(super) use thread_edit::{
    render_thread_edit, render_thread_edit_tag_picker, thread_edit_metrics, thread_edit_popup_area,
    thread_edit_tag_picker_visible_items,
};
#[cfg(test)]
pub(super) use toast::toast_line;
pub(super) use toast::{render_toast, toast_area};
#[cfg(test)]
pub(super) use url_picker::message_url_picker_lines_for_width;
pub(super) use url_picker::{message_url_picker_popup_area, render_message_url_picker};

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
        ActiveModalPopupKind::Leader => Some(leader_popup_area_for_state(frame_area, state)),
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

fn render_modal_paragraph(
    frame: &mut Frame,
    popup: Rect,
    title: impl Into<String>,
    lines: Vec<Line<'static>>,
) {
    let inner = render_modal_frame(frame, popup, title);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn popup_shortcut_help_text(items: &[(&str, &str)]) -> String {
    items
        .iter()
        .map(|(shortcut, description)| format!("[{shortcut}] {description}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn popup_button_line(shortcut: &'static str, label: &'static str, active: bool) -> Line<'static> {
    popup_button_line_with_style(shortcut, label, active, Style::default())
}

fn popup_danger_button_line(
    shortcut: &'static str,
    label: &'static str,
    active: bool,
) -> Line<'static> {
    popup_button_line_with_style(
        shortcut,
        label,
        active,
        theme::current().style(theme::HighlightGroup::Error),
    )
}

fn popup_button_line_with_style(
    shortcut: &'static str,
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
    selection_marker_with("› ", selected)
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

fn editable_tags_section_line(active: bool, required: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{}tags:", editable_field_marker(active)),
        editable_field_label_style(active, false),
    )];
    if required {
        spans.push(Span::styled(
            " required",
            theme::current().style(theme::HighlightGroup::Error),
        ));
    }
    Line::from(spans)
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
