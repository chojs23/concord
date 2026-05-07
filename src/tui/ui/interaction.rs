use ratatui::layout::Rect;

use super::super::state::{DashboardState, FocusPane};
use super::{
    layout::{centered_rect, dashboard_areas, message_areas},
    panel_block, panel_block_owned,
    popups::user_profile_popup_area,
    types::{ActionMenuTarget, MouseTarget},
};

pub(crate) fn focus_pane_at(area: Rect, column: u16, row: u16) -> Option<FocusPane> {
    let areas = dashboard_areas(area);
    [
        (areas.guilds, FocusPane::Guilds),
        (areas.channels, FocusPane::Channels),
        (areas.messages, FocusPane::Messages),
        (areas.members, FocusPane::Members),
    ]
    .into_iter()
    .find_map(|(area, pane)| rect_contains(area, column, row).then_some(pane))
}

pub(crate) fn mouse_target_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    let areas = dashboard_areas(area);
    if let Some(target) = action_menu_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if let Some(target) = pane_row_mouse_target(areas.guilds, FocusPane::Guilds, column, row) {
        return Some(target);
    }
    if let Some(target) = pane_row_mouse_target(areas.channels, FocusPane::Channels, column, row) {
        return Some(target);
    }
    if let Some(target) = message_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if let Some(target) = pane_row_mouse_target(areas.members, FocusPane::Members, column, row) {
        return Some(target);
    }
    None
}

pub(crate) fn user_profile_popup_contains(area: Rect, column: u16, row: u16) -> bool {
    let areas = dashboard_areas(area);
    rect_contains(user_profile_popup_area(areas.messages), column, row)
}

fn action_menu_mouse_target(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    if state.is_message_action_menu_open() {
        return action_menu_row_target(
            message_action_menu_area(area, state),
            state.selected_message_action_items().len(),
            ActionMenuTarget::Message,
            column,
            row,
        );
    }
    if state.is_guild_action_menu_open() {
        return action_menu_row_target(
            guild_action_menu_area(area, state),
            state.selected_guild_action_items().len(),
            ActionMenuTarget::Guild,
            column,
            row,
        );
    }
    if state.is_channel_action_menu_open() {
        let item_count = if state.is_channel_action_threads_phase() {
            state.channel_action_thread_items().len()
        } else {
            state.selected_channel_action_items().len()
        };
        return action_menu_row_target(
            channel_action_menu_area(area, state),
            item_count,
            ActionMenuTarget::Channel,
            column,
            row,
        );
    }
    if state.is_member_action_menu_open() {
        return action_menu_row_target(
            member_action_menu_area(area, state),
            state.selected_member_action_items().len(),
            ActionMenuTarget::Member,
            column,
            row,
        );
    }
    None
}

fn action_menu_row_target(
    popup: Option<Rect>,
    item_count: usize,
    menu: ActionMenuTarget,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    let Some(popup) = popup else {
        return Some(MouseTarget::ModalBackdrop);
    };
    if !rect_contains(popup, column, row) {
        return Some(MouseTarget::ModalBackdrop);
    }
    let inner = panel_block("", false).inner(popup);
    if rect_contains(inner, column, row) {
        let row = row.saturating_sub(inner.y) as usize;
        if row < item_count {
            return Some(MouseTarget::ActionRow { menu, row });
        }
    }
    Some(MouseTarget::ModalBackdrop)
}

fn message_action_menu_area(area: Rect, state: &DashboardState) -> Option<Rect> {
    let actions = state.selected_message_action_items();
    (!actions.is_empty()).then(|| centered_rect(area, 54, (actions.len() as u16).saturating_add(4)))
}

fn guild_action_menu_area(area: Rect, state: &DashboardState) -> Option<Rect> {
    let actions = state.selected_guild_action_items();
    (!actions.is_empty()).then(|| centered_rect(area, 48, (actions.len() as u16).saturating_add(4)))
}

fn channel_action_menu_area(area: Rect, state: &DashboardState) -> Option<Rect> {
    if state.is_channel_action_threads_phase() {
        let row_count = state.channel_action_thread_items().len().max(1) as u16;
        Some(centered_rect(area, 54, row_count.saturating_add(4)))
    } else {
        let actions = state.selected_channel_action_items();
        (!actions.is_empty())
            .then(|| centered_rect(area, 54, (actions.len() as u16).saturating_add(4)))
    }
}

fn member_action_menu_area(area: Rect, state: &DashboardState) -> Option<Rect> {
    let actions = state.selected_member_action_items();
    (!actions.is_empty()).then(|| centered_rect(area, 48, (actions.len() as u16).saturating_add(4)))
}

fn pane_row_mouse_target(
    area: Rect,
    pane: FocusPane,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block("", false).inner(area);
    if rect_contains(inner, column, row) {
        return Some(MouseTarget::PaneRow {
            pane,
            row: row.saturating_sub(inner.y) as usize,
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
