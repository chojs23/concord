use ratatui::layout::Rect;

use super::super::state::{DashboardState, FocusPane};
use super::{
    layout::{centered_rect, dashboard_areas, message_areas},
    panel_block, panel_block_owned,
    popups::{
        channel_switcher_item_index_at, channel_switcher_popup_area, user_profile_popup_area,
    },
    types::{ActionMenuTarget, MouseTarget},
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
    if let Some(target) = channel_switcher_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if let Some(target) = action_menu_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if state.is_pane_visible(FocusPane::Guilds)
        && let Some(target) = pane_row_mouse_target(
            areas.guilds,
            FocusPane::Guilds,
            column,
            row,
            state.guild_pane_filter_query().is_some(),
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
        )
    {
        return Some(target);
    }
    if let Some(target) = message_mouse_target(areas.messages, state, column, row) {
        return Some(target);
    }
    if state.is_pane_visible(FocusPane::Members)
        && let Some(target) =
            pane_row_mouse_target(areas.members, FocusPane::Members, column, row, false)
    {
        return Some(target);
    }
    None
}

fn channel_switcher_mouse_target(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<MouseTarget> {
    if !state.is_channel_switcher_open() {
        return None;
    }
    let popup = channel_switcher_popup_area(area);
    if !rect_contains(popup, column, row) {
        return Some(MouseTarget::ModalBackdrop);
    }
    channel_switcher_item_index_at(area, state, column, row)
        .map(|row| MouseTarget::ChannelSwitcherRow { row })
        .or(Some(MouseTarget::ModalBackdrop))
}

pub(crate) fn user_profile_popup_contains(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> bool {
    let areas = dashboard_areas(area, state);
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
    (!actions.is_empty()).then(|| centered_rect(area, 54, (actions.len() as u16).saturating_add(2)))
}

fn pane_row_mouse_target(
    area: Rect,
    pane: FocusPane,
    column: u16,
    row: u16,
    filter_active: bool,
) -> Option<MouseTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block("", false).inner(area);
    // When the filter bar occupies the last row of the inner area, shrink the
    // list hit region so clicks on that row don't resolve to a list entry.
    let list_height = if filter_active && inner.height >= 2 {
        inner.height - 1
    } else {
        inner.height
    };
    let list_area = Rect {
        height: list_height,
        ..inner
    };
    if rect_contains(list_area, column, row) {
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
