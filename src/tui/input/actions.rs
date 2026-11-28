use crate::discord::AppCommand;

use super::super::state::{DashboardState, FocusPane};

/// Activate the focused selection using the same semantics for keyboard and
/// pointer input. Keeping this here prevents double-click behavior from
/// drifting away from Enter as new pane types are added.
pub(super) fn activate_focused_target(state: &mut DashboardState) -> Option<AppCommand> {
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
                return state.restore_last_opened_channel_command();
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
        FocusPane::Members => {
            state.open_selected_member_actions();
            None
        }
    }
}
