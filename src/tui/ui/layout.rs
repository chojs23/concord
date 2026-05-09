use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    widgets::{Block, Borders},
};
use unicode_width::UnicodeWidthStr;

use super::super::{message_format::wrap_text_lines, state::DashboardState};
use super::types::{
    DashboardAreas, EMBED_PREVIEW_GUTTER_PREFIX, IMAGE_PREVIEW_HEIGHT, IMAGE_PREVIEW_WIDTH,
    IMAGE_VIEWER_POPUP_HEIGHT, IMAGE_VIEWER_POPUP_WIDTH, MAX_REACTION_USERS_VISIBLE_LINES,
    MESSAGE_AVATAR_OFFSET, MIN_MESSAGE_INPUT_HEIGHT, MessageAreas,
};

pub(super) fn dashboard_areas(area: Rect) -> DashboardAreas {
    let [header, main, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let [guilds, channels, center, members] = Layout::horizontal([
        Constraint::Length(20),
        Constraint::Length(24),
        Constraint::Min(40),
        Constraint::Length(26),
    ])
    .areas(main);

    DashboardAreas {
        header,
        guilds,
        channels,
        messages: center,
        members,
        footer,
    }
}

pub(super) fn image_viewer_popup(area: Rect) -> Rect {
    centered_rect(area, IMAGE_VIEWER_POPUP_WIDTH, IMAGE_VIEWER_POPUP_HEIGHT)
}

pub(super) fn image_viewer_image_area(area: Rect) -> Rect {
    let inner = image_viewer_popup(area).inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let [image_area, _] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    image_area
}

pub(super) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub(super) fn panel_scrollbar_area(area: Rect) -> Rect {
    area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    })
}

pub(super) fn vertical_scrollbar_visible(
    area: Rect,
    viewport_len: usize,
    content_len: usize,
) -> bool {
    area.height > 0 && viewport_len > 0 && content_len > viewport_len
}

pub(super) fn reaction_users_visible_line_count(area: Rect) -> usize {
    usize::from(area.height)
        .saturating_sub(5)
        .min(MAX_REACTION_USERS_VISIBLE_LINES)
}

pub(super) fn message_list_area(area: Rect, state: &DashboardState) -> Rect {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    message_areas(inner, state).list
}

pub(super) fn message_areas(area: Rect, state: &DashboardState) -> MessageAreas {
    let composer_height = composer_height(area, state);
    let typing_height: u16 = state.typing_footer_for_selected_channel().is_some().into();
    let banner_height: u16 = state.unread_banner().is_some().into();
    let [unread_banner, list, typing, composer] = Layout::vertical([
        Constraint::Length(banner_height),
        Constraint::Min(0),
        Constraint::Length(typing_height),
        Constraint::Length(composer_height),
    ])
    .areas(area);
    MessageAreas {
        unread_banner,
        list,
        typing,
        composer,
    }
}

pub(super) fn inline_image_preview_height(area: Rect, visible: bool) -> u16 {
    if !visible || area.height < 5 {
        0
    } else {
        IMAGE_PREVIEW_HEIGHT
            .min(area.height.saturating_sub(1))
            .max(3)
    }
}

pub(super) fn inline_image_preview_width(area: Rect) -> u16 {
    area.width
        .saturating_sub(inline_image_content_offset(area))
        .min(IMAGE_PREVIEW_WIDTH)
}

pub(super) fn inline_image_content_offset(area: Rect) -> u16 {
    MESSAGE_AVATAR_OFFSET.min(area.width.saturating_sub(1))
}

pub(super) fn inline_image_preview_area(
    list: Rect,
    row: isize,
    preview_x_offset_columns: u16,
    preview_width: u16,
    preview_height: u16,
    accent_color: Option<u32>,
) -> Option<Rect> {
    if preview_width == 0 || preview_height == 0 {
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
        .saturating_add(preview_x_offset_columns)
        .saturating_add(gutter_width);
    let available_width = list
        .width
        .saturating_sub(content_offset)
        .saturating_sub(preview_x_offset_columns)
        .saturating_sub(gutter_width);

    Some(Rect {
        x,
        y: u16::try_from(visible_top).ok()?,
        width: preview_width.min(available_width),
        height: u16::try_from(visible_bottom - visible_top).ok()?,
    })
}

pub(super) fn composer_height(area: Rect, state: &DashboardState) -> u16 {
    let content_lines = if state.is_composing()
        || !state.composer_input().is_empty()
        || !state.pending_composer_attachments().is_empty()
    {
        composer_content_line_count(state, composer_inner_width(area.width))
    } else {
        1
    };
    MIN_MESSAGE_INPUT_HEIGHT.max(content_lines.saturating_add(2))
}

pub(super) fn composer_inner_width(width: u16) -> u16 {
    width.saturating_sub(2).max(1)
}

pub(super) fn composer_content_line_count(state: &DashboardState, width: u16) -> u16 {
    let mut line_count = composer_prompt_line_count(state.composer_input(), width);
    line_count = line_count.saturating_add(state.pending_composer_attachments().len() as u16);
    if state.is_composing() && state.reply_target_message_state().is_some() {
        line_count = line_count.saturating_add(1);
    }
    line_count
}

pub(super) fn composer_prompt_line_count(input: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let prompt = format!("> {input}");
    wrap_text_lines(&prompt, width).len() as u16
}
