use ratatui::layout::Rect;

use super::super::state::{DashboardState, FocusPane};
use super::{
    channel_pane_header_height,
    layout::{dashboard_areas, message_areas},
    panel_block, panel_block_owned,
    popups::active_selectable_popup_layout,
    types::MouseTarget,
};

pub(crate) fn focus_pane_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<FocusPane> {
    let areas = dashboard_areas(area, state);
    [
        (areas.guilds, FocusPane::Guilds),
        (areas.channels, FocusPane::Channels),
        (areas.messages, FocusPane::Messages),
        (areas.members, FocusPane::Members),
    ]
    .into_iter()
    .filter(|(_, pane)| state.is_pane_visible(*pane))
    .find_map(|(area, pane)| rect_contains(area, column, row).then_some(pane))
}

pub(crate) fn mouse_target_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    let areas = dashboard_areas(area, state);
    if let Some(target) = selectable_popup_mouse_target(area, state, column, row) {
        return Some(target);
    }
    if state.active_modal_popup_kind().is_some() || state.is_folder_settings_open() {
        return Some(MouseTarget::ModalBackdrop);
    }
    if state.is_pane_visible(FocusPane::Guilds)
        && let Some(target) = pane_row_mouse_target(
            areas.guilds,
            FocusPane::Guilds,
            column,
            row,
            state.guild_pane_filter_query().is_some(),
            0,
        )
    {
        return Some(target);
    }
    if state.is_pane_visible(FocusPane::Channels)
        && let Some(target) = pane_row_mouse_target(
            areas.channels,
            FocusPane::Channels,
            column,
            row,
            state.channel_pane_filter_query().is_some(),
            channel_pane_header_height(state),
        )
    {
        return Some(target);
    }
    if let Some(target) = message_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if state.is_pane_visible(FocusPane::Members)
        && let Some(target) =
            pane_row_mouse_target(areas.members, FocusPane::Members, column, row, false, 0)
    {
        return Some(target);
    }
    None
}

fn selectable_popup_mouse_target(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    let layout = active_selectable_popup_layout(area, state)?;
    if !rect_contains(layout.popup, column, row) {
        return Some(MouseTarget::ModalBackdrop);
    }
    layout
        .item_at(column, row)
        .map(|row| MouseTarget::PopupRow {
            target: layout.target,
            row,
        })
        .or(Some(MouseTarget::ModalBackdrop))
}

fn pane_row_mouse_target(
    area: Rect,
    pane: FocusPane,
    column: u16,
    row: u16,
    filter_active: bool,
    leading_rows: u16,
) -> Option<MouseTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block("", false).inner(area);
    let leading_rows = leading_rows.min(inner.height);
    // When the filter bar occupies the last row of the inner area, shrink the
    // list hit region so clicks on that row don't resolve to a list entry.
    let content_height = inner.height.saturating_sub(leading_rows);
    let list_height = if filter_active && content_height >= 2 {
        content_height - 1
    } else {
        content_height
    };
    let list_area = Rect {
        y: inner.y.saturating_add(leading_rows),
        height: list_height,
        ..inner
    };
    if rect_contains(list_area, column, row) {
        return Some(MouseTarget::PaneRow {
            pane,
            row: row.saturating_sub(list_area.y) as usize,
        });
    }
    Some(MouseTarget::Pane(pane))
}

fn message_mouse_target(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block_owned(String::new(), false).inner(area);
    let message_areas = message_areas(inner, state);
    if rect_contains(message_areas.composer, column, row) {
        return Some(MouseTarget::Composer);
    }
    if rect_contains(message_areas.list, column, row) {
        return Some(MouseTarget::PaneRow {
            pane: FocusPane::Messages,
            row: row.saturating_sub(message_areas.list.y) as usize,
        });
    }
    Some(MouseTarget::Pane(FocusPane::Messages))
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}
