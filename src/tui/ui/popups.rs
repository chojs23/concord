use super::activity::{ActivityLeading, build_activity_render};
use super::message::list::render_image_preview;
use super::*;
use crate::discord::ActivityKind;
use crate::tui::format::format_byte_size;
use ratatui::layout::Position;

mod action_menu;
mod attachment_viewer;
mod channel_switcher;
mod confirmation;
mod debug_log;
mod keymap;
mod options;
mod polls;
mod profile;
mod reactions;
mod search;
mod toast;
mod url_picker;

#[cfg(test)]
pub(super) use action_menu::{leader_action_lines_for_test, message_action_menu_lines};
pub(super) use action_menu::{render_leader_popup, render_message_action_menu};
#[cfg(test)]
pub(super) use attachment_viewer::centered_viewer_preview_area;
pub(super) use attachment_viewer::render_attachment_viewer;
#[cfg(test)]
pub(super) use channel_switcher::{channel_switcher_cursor_position, channel_switcher_lines};
pub(super) use channel_switcher::{
    channel_switcher_item_index_at, channel_switcher_popup_area, render_channel_switcher_popup,
};
#[cfg(test)]
pub(super) use confirmation::{
    message_delete_confirmation_lines, message_pin_confirmation_lines, quit_confirmation_lines,
};
pub(super) use confirmation::{
    render_guild_leave_confirmation, render_message_delete_confirmation,
    render_message_pin_confirmation, render_quit_confirmation,
};
#[cfg(test)]
pub(super) use debug_log::debug_log_popup_lines;
pub(super) use debug_log::render_debug_log_popup;
#[cfg(test)]
pub(super) use keymap::keymap_help_popup_lines;
pub(super) use keymap::{
    keymap_popup_text_area, keymap_popup_total_lines, render_keymap_help_popup,
};
#[cfg(test)]
pub(super) use options::options_popup_lines;
pub(super) use options::render_options_popup;
#[cfg(test)]
pub(super) use polls::poll_vote_picker_lines;
pub(super) use polls::render_poll_vote_picker;
pub(super) use profile::{
    render_user_profile_popup, user_profile_popup_area, user_profile_popup_has_avatar,
    user_profile_popup_text_geometry, user_profile_popup_total_lines,
};
#[cfg(test)]
pub(super) use profile::{user_profile_popup_lines, user_profile_popup_lines_with_activities};
#[cfg(test)]
pub(super) use reactions::{
    emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
    emoji_reaction_picker_lines_with_existing, emoji_reaction_picker_lines_with_own_reactions,
    filtered_emoji_reaction_picker_lines, reaction_users_popup_lines,
};
pub(super) use reactions::{render_emoji_reaction_picker, render_reaction_users_popup};
pub(super) use search::render_search_popup;
pub(super) use toast::render_toast;
#[cfg(test)]
pub(super) use toast::{toast_area, toast_line};
#[cfg(test)]
pub(super) use url_picker::message_url_picker_lines_for_width;
pub(super) use url_picker::render_message_url_picker;

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

fn selectable_popup_marker(selected: bool) -> Span<'static> {
    let marker = if selected { "› " } else { "  " };
    Span::styled(marker, Style::default().fg(ACCENT))
}

fn selectable_popup_shortcut_span(shortcut: impl Into<String>) -> Span<'static> {
    Span::styled(shortcut.into(), Style::default().fg(DIM))
}

fn selectable_popup_label_style(selected: bool, enabled: bool) -> Style {
    let mut style = if enabled {
        Style::default()
    } else {
        Style::default().fg(DIM)
    };
    if selected {
        style = style
            .bg(Color::Rgb(40, 45, 90))
            .add_modifier(Modifier::BOLD);
    }
    style
}

fn shortcut_prefix(shortcut: Option<char>) -> String {
    shortcut
        .map(|shortcut| format!("[{shortcut}] "))
        .unwrap_or_else(|| "    ".to_owned())
}
