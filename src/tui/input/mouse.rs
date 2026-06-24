use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::discord::AppCommand;

use super::super::{
    state::{ActiveModalPopupKind, DashboardState, FocusPane},
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

#[derive(Default)]
pub struct MouseOutcome {
    pub handled: bool,
    pub command: Option<AppCommand>,
}

impl MouseOutcome {
    fn ignored() -> Self {
        Self::default()
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
        state.close_all_action_contexts();
        state.close_leader();
        clicks.clear();
    }

    let target = ui::mouse_target_at(area, state, mouse.column, mouse.row);
    let modal_mouse = matches!(
        target,
        Some(
            ui::MouseTarget::PopupRow { .. }
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
        state.close_composer();
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // The user-profile popup absorbs clicks only inside its drawn
            // rectangle. Clicks outside the popup should still reach the
            // dashboard instead of making the whole screen inert.
            if state.is_active_modal_popup(ActiveModalPopupKind::UserProfile)
                && ui::user_profile_popup_contains(area, mouse.column, mouse.row)
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
            if state.is_active_modal_popup(ActiveModalPopupKind::UserProfile) {
                state.scroll_user_profile_popup_down();
                return MouseOutcome::handled(None);
            }
            let pane = ui::focus_pane_at(area, state, mouse.column, mouse.row);
            if let Some(pane) = pane {
                state.focus_pane(pane);
            }
            state.scroll_focused_pane_viewport_down();
            MouseOutcome::handled(None)
        }
        MouseEventKind::ScrollUp => {
            clicks.clear();
            if modal_mouse {
                move_modal_up(state);
                return MouseOutcome::handled(None);
            }
            if state.is_active_modal_popup(ActiveModalPopupKind::UserProfile) {
                state.scroll_user_profile_popup_up();
                return MouseOutcome::handled(None);
            }
            let pane = ui::focus_pane_at(area, state, mouse.column, mouse.row);
            if let Some(pane) = pane {
                state.focus_pane(pane);
            }
            state.scroll_focused_pane_viewport_up();
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
        ui::MouseTarget::PopupRow { target, row } => {
            let selected = select_popup_row(state, target, row);
            if !selected {
                clicks.clear();
                return MouseOutcome::handled(None);
            }
            let command = if clicks.record_left_click(ui::MouseTarget::PopupRow { target, row }) {
                activate_popup_row(state, target)
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
            let command = if clicks.record_left_click(target) {
                activate_focused_target(state)
            } else {
                None
            };
            MouseOutcome::handled(command)
        }
    }
}

fn move_modal_down(state: &mut DashboardState) {
    match state.active_modal_popup_kind() {
        Some(ActiveModalPopupKind::ChannelSwitcher) => state.move_channel_switcher_down(),
        Some(ActiveModalPopupKind::MessageUrlPicker) => state.move_message_url_picker_down(),
        Some(ActiveModalPopupKind::MessageActionMenu) => state.move_message_action_down(),
        _ => {}
    }
}

fn move_modal_up(state: &mut DashboardState) {
    match state.active_modal_popup_kind() {
        Some(ActiveModalPopupKind::ChannelSwitcher) => state.move_channel_switcher_up(),
        Some(ActiveModalPopupKind::MessageUrlPicker) => state.move_message_url_picker_up(),
        Some(ActiveModalPopupKind::MessageActionMenu) => state.move_message_action_up(),
        _ => {}
    }
}

fn select_popup_row(state: &mut DashboardState, target: ui::PopupListTarget, row: usize) -> bool {
    match target {
        ui::PopupListTarget::MessageAction => state.select_message_action_row(row),
        ui::PopupListTarget::MessageUrl => state.select_message_url_row(row),
    }
}

fn activate_popup_row(
    state: &mut DashboardState,
    target: ui::PopupListTarget,
) -> Option<AppCommand> {
    match target {
        ui::PopupListTarget::MessageAction => state.activate_selected_message_action(),
        ui::PopupListTarget::MessageUrl => state.activate_selected_message_url(),
    }
}

fn activate_focused_target(state: &mut DashboardState) -> Option<AppCommand> {
    let focus = state.focus();
    if state.is_pane_filter_active(focus) {
        if state.is_pane_filter_editing(focus) {
            state.commit_pane_filter(focus);
            return None;
        }
        return state.activate_pane_filter_selection(focus);
    }

    match focus {
        FocusPane::Guilds => {
            if state.confirm_selected_guild() {
                state.focus_pane(FocusPane::Channels);
            }
            None
        }
        FocusPane::Channels => {
            let command = state.confirm_selected_channel_command();
            if command.is_some() {
                state.focus_pane(FocusPane::Messages);
            }
            command
        }
        FocusPane::Messages => state.activate_selected_message_pane_item(),
        FocusPane::Members => state.show_selected_member_profile(),
    }
}

fn ignores_dashboard_mouse(state: &DashboardState) -> bool {
    matches!(
        state.active_modal_popup_kind(),
        Some(
            ActiveModalPopupKind::DebugLog
                | ActiveModalPopupKind::ReactionUsers
                | ActiveModalPopupKind::PollVotePicker
                | ActiveModalPopupKind::EmojiReactionPicker
                | ActiveModalPopupKind::MessageActionMenu
                | ActiveModalPopupKind::MessageUrlPicker
                | ActiveModalPopupKind::AttachmentViewer
                | ActiveModalPopupKind::ChannelSwitcher
                | ActiveModalPopupKind::ForumPostComposer
                | ActiveModalPopupKind::ThreadEdit
        )
    ) || state.is_guild_leader_action_active()
        || state.is_channel_leader_action_active()
        || state.is_member_leader_action_active()
}
