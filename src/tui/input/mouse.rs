use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::discord::AppCommand;

use super::super::{
    state::{DashboardState, FocusPane},
    ui,
};

const DOUBLE_CLICK_MAX_DELAY: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct MouseClickTracker {
    last_left_click: Option<MouseClick>,
}

struct MouseClick {
    target: ui::MouseTarget,
    at: Instant,
}

pub struct MouseOutcome {
    pub handled: bool,
    pub command: Option<AppCommand>,
}

impl MouseOutcome {
    fn ignored() -> Self {
        Self {
            handled: false,
            command: None,
        }
    }

    fn handled(command: Option<AppCommand>) -> Self {
        Self {
            handled: true,
            command,
        }
    }
}

#[cfg(test)]
pub fn handle_mouse(state: &mut DashboardState, mouse: MouseEvent, area: Rect) -> bool {
    let mut clicks = MouseClickTracker::default();
    handle_mouse_event(state, mouse, area, &mut clicks).handled
}

pub fn handle_mouse_event(
    state: &mut DashboardState,
    mouse: MouseEvent,
    area: Rect,
    clicks: &mut MouseClickTracker,
) -> MouseOutcome {
    if state.is_leader_active() {
        state.close_all_action_menus();
        state.close_leader();
        clicks.clear();
    }

    let target = ui::mouse_target_at(area, state, mouse.column, mouse.row);
    let modal_mouse = matches!(
        target,
        Some(
            ui::MouseTarget::ActionRow { .. }
                | ui::MouseTarget::ChannelSwitcherRow { .. }
                | ui::MouseTarget::ModalBackdrop
        )
    );
    if ignores_dashboard_mouse(state) && !modal_mouse {
        return MouseOutcome::ignored();
    }
    let blurred_composer = state.is_composing()
        && target != Some(ui::MouseTarget::Composer)
        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
    if state.is_composing() && target != Some(ui::MouseTarget::Composer) && !blurred_composer {
        return MouseOutcome::ignored();
    }
    if blurred_composer {
        clicks.clear();
        state.cancel_composer();
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The user-profile popup absorbs clicks only inside its drawn
            // rectangle. Clicks outside the popup should still reach the
            // dashboard instead of making the whole screen inert.
            if state.is_user_profile_popup_open()
                && ui::user_profile_popup_contains(area, state, mouse.column, mouse.row)
            {
                clicks.clear();
                return MouseOutcome::handled(None);
            }
            let Some(target) = target else {
                clicks.clear();
                return if blurred_composer {
                    MouseOutcome::handled(None)
                } else {
                    MouseOutcome::ignored()
                };
            };
            handle_left_click(state, target, clicks)
        }
        MouseEventKind::ScrollDown => {
            clicks.clear();
            if modal_mouse {
                move_modal_down(state);
                return MouseOutcome::handled(None);
            }
            // Wheel events while the user-profile popup is open should scroll
            // the popup body, not the pane below it.
            if state.is_user_profile_popup_open() {
                state.scroll_user_profile_popup_down();
                return MouseOutcome::handled(None);
            }
            let pane = ui::focus_pane_at(area, state, mouse.column, mouse.row);
            if let Some(pane) = pane {
                state.focus_pane(pane);
            }
            scroll_focused_pane_down(state);
            MouseOutcome::handled(None)
        }
        MouseEventKind::ScrollUp => {
            clicks.clear();
            if modal_mouse {
                move_modal_up(state);
                return MouseOutcome::handled(None);
            }
            if state.is_user_profile_popup_open() {
                state.scroll_user_profile_popup_up();
                return MouseOutcome::handled(None);
            }
            let pane = ui::focus_pane_at(area, state, mouse.column, mouse.row);
            if let Some(pane) = pane {
                state.focus_pane(pane);
            }
            scroll_focused_pane_up(state);
            MouseOutcome::handled(None)
        }
        MouseEventKind::Up(MouseButton::Left) => MouseOutcome::handled(None),
        _ => {
            clicks.clear();
            MouseOutcome::ignored()
        }
    }
}

impl MouseClickTracker {
    fn clear(&mut self) {
        self.last_left_click = None;
    }

    fn record_left_click(&mut self, target: ui::MouseTarget) -> bool {
        let now = Instant::now();
        let double_click = self.last_left_click.as_ref().is_some_and(|click| {
            click.target == target && now.duration_since(click.at) <= DOUBLE_CLICK_MAX_DELAY
        });
        self.last_left_click = if double_click {
            None
        } else {
            Some(MouseClick { target, at: now })
        };
        double_click
    }
}

fn handle_left_click(
    state: &mut DashboardState,
    target: ui::MouseTarget,
    clicks: &mut MouseClickTracker,
) -> MouseOutcome {
    match target {
        ui::MouseTarget::Composer => {
            clicks.clear();
            state.start_composer();
            MouseOutcome::handled(None)
        }
        ui::MouseTarget::ModalBackdrop => {
            clicks.clear();
            MouseOutcome::handled(None)
        }
        ui::MouseTarget::ActionRow { menu, row } => {
            let selected = select_action_menu_row(state, menu, row);
            if !selected {
                clicks.clear();
                return MouseOutcome::handled(None);
            }
            let command = if clicks.record_left_click(target) {
                activate_action_menu(state, menu)
            } else {
                None
            };
            MouseOutcome::handled(command)
        }
        ui::MouseTarget::ChannelSwitcherRow { row } => {
            let selected = state.select_channel_switcher_item(row);
            if !selected {
                clicks.clear();
                return MouseOutcome::handled(None);
            }
            let command = if clicks.record_left_click(target) {
                state.activate_selected_channel_switcher_item()
            } else {
                None
            };
            MouseOutcome::handled(command)
        }
        ui::MouseTarget::Pane(pane) => {
            clicks.clear();
            state.focus_pane(pane);
            MouseOutcome::handled(None)
        }
        ui::MouseTarget::PaneRow { pane, row } => {
            state.focus_pane(pane);
            let selected = state.select_visible_pane_row(pane, row);
            if !selected {
                clicks.clear();
                return MouseOutcome::handled(None);
            }
            let command = if selected && clicks.record_left_click(target) {
                activate_focused_target(state)
            } else {
                None
            };
            MouseOutcome::handled(command)
        }
    }
}

fn move_modal_down(state: &mut DashboardState) {
    if state.is_channel_switcher_open() {
        state.move_channel_switcher_down();
    } else {
        move_action_menu_down(state);
    }
}

fn move_modal_up(state: &mut DashboardState) {
    if state.is_channel_switcher_open() {
        state.move_channel_switcher_up();
    } else {
        move_action_menu_up(state);
    }
}

fn select_action_menu_row(
    state: &mut DashboardState,
    menu: ui::ActionMenuTarget,
    row: usize,
) -> bool {
    match menu {
        ui::ActionMenuTarget::Message => state.select_message_action_row(row),
        ui::ActionMenuTarget::Guild => state.select_guild_action_row(row),
        ui::ActionMenuTarget::Channel => state.select_channel_action_row(row),
        ui::ActionMenuTarget::Member => state.select_member_action_row(row),
    }
}

fn activate_action_menu(
    state: &mut DashboardState,
    menu: ui::ActionMenuTarget,
) -> Option<AppCommand> {
    match menu {
        ui::ActionMenuTarget::Message => state.activate_selected_message_action(),
        ui::ActionMenuTarget::Guild => state.activate_selected_guild_action(),
        ui::ActionMenuTarget::Channel => state.activate_selected_channel_action(),
        ui::ActionMenuTarget::Member => state.activate_selected_member_action(),
    }
}

fn move_action_menu_down(state: &mut DashboardState) {
    if state.is_message_action_menu_open() {
        state.move_message_action_down();
    } else if state.is_guild_action_menu_open() {
        state.move_guild_action_down();
    } else if state.is_channel_action_menu_open() {
        state.move_channel_action_down();
    } else if state.is_member_action_menu_open() {
        state.move_member_action_down();
    }
}

fn move_action_menu_up(state: &mut DashboardState) {
    if state.is_message_action_menu_open() {
        state.move_message_action_up();
    } else if state.is_guild_action_menu_open() {
        state.move_guild_action_up();
    } else if state.is_channel_action_menu_open() {
        state.move_channel_action_up();
    } else if state.is_member_action_menu_open() {
        state.move_member_action_up();
    }
}

fn activate_focused_target(state: &mut DashboardState) -> Option<AppCommand> {
    match state.focus() {
        FocusPane::Guilds => {
            state.confirm_selected_guild();
            None
        }
        FocusPane::Channels => state.confirm_selected_channel_command(),
        FocusPane::Messages => state.activate_selected_message_pane_item(),
        FocusPane::Members => state.show_selected_member_profile(),
    }
}

fn ignores_dashboard_mouse(state: &DashboardState) -> bool {
    state.is_debug_log_popup_open()
        || state.is_reaction_users_popup_open()
        || state.is_poll_vote_picker_open()
        || state.is_emoji_reaction_picker_open()
        || state.is_message_action_menu_open()
        || state.is_image_viewer_open()
        || state.is_guild_action_menu_open()
        || state.is_channel_action_menu_open()
        || state.is_member_action_menu_open()
        || state.is_channel_switcher_open()
}

fn scroll_focused_pane_down(state: &mut DashboardState) {
    state.scroll_focused_pane_viewport_down();
}

fn scroll_focused_pane_up(state: &mut DashboardState) {
    state.scroll_focused_pane_viewport_up();
}
