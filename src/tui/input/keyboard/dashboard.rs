use crate::discord::AppCommand;
use crate::tui::keybindings::{
    DashboardAction, OptionsCategoryShortcut, SelectionAction, UiAction,
};
use crate::tui::state::{DashboardState, FocusPane};

pub(super) fn handle_dashboard_action(
    state: &mut DashboardState,
    focus: FocusPane,
    action: DashboardAction,
) -> Option<AppCommand> {
    match action {
        DashboardAction::Select(SelectionAction::Next) => {
            if let Some(command) = state.next_newer_history_command_for_down_by(1) {
                return Some(command);
            }
            state.move_down();
            None
        }
        DashboardAction::Select(SelectionAction::Previous) => {
            state.move_up();
            state.next_older_history_command()
        }
        DashboardAction::MessageShortcut(kind) => state.activate_message_action_kind(kind),
        DashboardAction::Back => {
            if !state.return_from_pinned_message_view()
                && !state.return_from_channel_thread_list_view()
            {
                state.return_from_opened_thread();
            }
            None
        }
        DashboardAction::Quit => {
            state.open_quit_confirmation();
            None
        }
        DashboardAction::StartComposer => {
            state.start_composer();
            None
        }
        DashboardAction::FocusPane(pane) => {
            state.show_and_focus_pane(pane);
            None
        }
        DashboardAction::CycleFocusBackward => {
            state.cycle_focus_backward();
            None
        }
        DashboardAction::CycleFocusForward => {
            state.cycle_focus();
            None
        }
        DashboardAction::OpenFocusedPaneFilter => {
            state.open_search_popup_for_focus(focus);
            None
        }
        DashboardAction::ResizePaneLeft => {
            state.adjust_focused_pane_width(-1);
            None
        }
        DashboardAction::ResizePaneRight => {
            state.adjust_focused_pane_width(1);
            None
        }
        DashboardAction::HalfPageDown => {
            if let Some(command) = state.next_newer_history_command_for_half_page_down() {
                return Some(command);
            }
            state.half_page_down();
            None
        }
        DashboardAction::HalfPageUp => {
            state.half_page_up();
            state.next_older_history_command_for_half_page_up()
        }
        DashboardAction::JumpTop => {
            state.jump_top();
            None
        }
        DashboardAction::JumpBottom => {
            state.jump_bottom();
            None
        }
        DashboardAction::ScrollViewportDown => {
            state.scroll_focused_pane_viewport_down();
            None
        }
        DashboardAction::ScrollViewportUp => {
            state.scroll_focused_pane_viewport_up();
            None
        }
        DashboardAction::ScrollHorizontalLeft => {
            state.scroll_focused_pane_horizontal_left();
            None
        }
        DashboardAction::ScrollHorizontalRight => {
            state.scroll_focused_pane_horizontal_right();
            None
        }
        DashboardAction::ActivateFocused => match focus {
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
            FocusPane::Members => {
                state.open_selected_member_actions();
                None
            }
            FocusPane::Messages => state.activate_selected_message_pane_item(),
        },
    }
}

pub(super) fn execute_ui_action(
    state: &mut DashboardState,
    focus: FocusPane,
    action: UiAction,
) -> Option<AppCommand> {
    if let Some(dashboard_action) = state
        .key_bindings()
        .dashboard_action_for_ui_action(action, focus)
    {
        return handle_dashboard_action(state, focus, dashboard_action);
    }

    match action {
        UiAction::ToggleGuildPane => state.toggle_pane_visibility(FocusPane::Guilds),
        UiAction::ToggleChannelPane => state.toggle_pane_visibility(FocusPane::Channels),
        UiAction::ToggleMemberPane => state.toggle_pane_visibility(FocusPane::Members),
        UiAction::OpenFocusedPaneAction => state.open_leader_actions_for_focused_target(),
        UiAction::OpenCurrentUserProfile => return state.open_current_user_profile_popup(),
        UiAction::OpenOptions => state.open_options_category_picker(),
        UiAction::ChannelSwitcher => state.open_channel_switcher(),
        UiAction::OpenNotificationInbox => state.open_notification_inbox(),
        UiAction::OpenDisplayOptions => {
            state.open_options_category_from_shortcut(OptionsCategoryShortcut::Display)
        }
        UiAction::OpenComposerOptions => {
            state.open_options_category_from_shortcut(OptionsCategoryShortcut::Composer)
        }
        UiAction::OpenNotificationOptions => {
            state.open_options_category_from_shortcut(OptionsCategoryShortcut::Notifications)
        }
        UiAction::OpenVoiceOptions => {
            state.open_options_category_from_shortcut(OptionsCategoryShortcut::Voice)
        }
        UiAction::VoiceDeafen => state.toggle_voice_deafen(),
        UiAction::VoiceMute => state.toggle_voice_mute(),
        UiAction::VoiceLeave => return state.leave_current_voice_channel_command(),
        _ => {}
    }
    None
}
