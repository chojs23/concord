use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_debug_log_popup_open() {
        return handle_debug_log_popup_key(state, key);
    }

    if state.is_options_popup_open() {
        return handle_options_popup_key(state, key);
    }

    if state.is_reaction_users_popup_open() {
        return handle_reaction_users_popup_key(state, key);
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    if key.code == KeyCode::Char('`') {
        state.toggle_debug_log_popup();
        return None;
    }

    if state.is_poll_vote_picker_open() {
        return handle_poll_vote_picker_key(state, key);
    }

    if state.is_emoji_reaction_picker_open() {
        return handle_emoji_reaction_picker_key(state, key);
    }

    if state.is_message_action_menu_open() {
        return handle_message_action_menu_key(state, key);
    }

    if state.is_image_viewer_action_menu_open() {
        return handle_image_viewer_action_menu_key(state, key);
    }

    if state.is_image_viewer_open() {
        return handle_image_viewer_key(state, key);
    }

    if state.is_guild_action_menu_open() {
        return handle_guild_action_menu_key(state, key);
    }

    if state.is_channel_action_menu_open() {
        return handle_channel_action_menu_key(state, key);
    }

    if state.is_member_action_menu_open() {
        return handle_member_action_menu_key(state, key);
    }

    if state.is_user_profile_popup_open() {
        return handle_user_profile_popup_key(state, key);
    }

    let focus = state.focus();
    match key.code {
        KeyCode::Esc if !state.return_from_pinned_message_view() => {
            state.return_from_opened_thread();
        }
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('o') => state.open_options_popup(),
        KeyCode::Char('a') => state.open_actions_for_focused_target(),
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('1') => state.focus_pane(FocusPane::Guilds),
        KeyCode::Char('2') => state.focus_pane(FocusPane::Channels),
        KeyCode::Char('3') => state.focus_pane(FocusPane::Messages),
        KeyCode::Char('4') => state.focus_pane(FocusPane::Members),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('J') if focus == FocusPane::Messages => state.scroll_message_viewport_down(),
        KeyCode::Char('L') => state.scroll_focused_pane_horizontal_right(),
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
            return state.next_older_history_command();
        }
        KeyCode::Char('K') if focus == FocusPane::Messages => state.scroll_message_viewport_up(),
        KeyCode::Char('H') => state.scroll_focused_pane_horizontal_left(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.half_page_down()
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.half_page_up();
            return state.next_older_history_command();
        }
        KeyCode::PageDown => state.half_page_down(),
        KeyCode::PageUp => {
            state.half_page_up();
            return state.next_older_history_command();
        }
        KeyCode::Char('g') => {
            state.jump_top();
        }
        KeyCode::Home => {
            if focus == FocusPane::Messages {
                state.scroll_message_viewport_top();
            } else {
                state.jump_top();
            }
        }
        KeyCode::Char('G') => state.jump_bottom(),
        KeyCode::End => {
            if focus == FocusPane::Messages {
                state.scroll_message_viewport_bottom();
            } else {
                state.jump_bottom();
            }
        }
        KeyCode::Tab => state.cycle_focus(),
        // Tree headers act like a small tree: Enter/Space toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        KeyCode::Enter | KeyCode::Char(' ') if focus == FocusPane::Guilds => {
            state.confirm_selected_guild()
        }
        KeyCode::Enter | KeyCode::Char(' ') if focus == FocusPane::Channels => {
            return state.confirm_selected_channel_command();
        }
        KeyCode::Enter | KeyCode::Char(' ') if focus == FocusPane::Members => {
            return state.show_selected_member_profile();
        }
        KeyCode::Enter | KeyCode::Char(' ') if focus == FocusPane::Messages => {
            return state.activate_selected_message_pane_item();
        }
        code if is_right_key(code) && focus == FocusPane::Guilds => state.open_selected_folder(),
        code if is_left_key(code) && focus == FocusPane::Guilds => state.close_selected_folder(),
        code if is_right_key(code) && focus == FocusPane::Channels => {
            state.open_selected_channel_category()
        }
        code if is_left_key(code) && focus == FocusPane::Channels => {
            state.close_selected_channel_category()
        }
        _ => {}
    }

    None
}

fn handle_message_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_message_action_menu(),
        code if is_down_key(code) => state.move_message_action_down(),
        code if is_up_key(code) => state.move_message_action_up(),
        code if is_confirm_key(code) => return state.activate_selected_message_action(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_message_action_shortcut(shortcut);
        }
        _ => {}
    }

    None
}

fn handle_image_viewer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_image_viewer(),
        code if is_left_key(code) => state.move_image_viewer_previous(),
        code if is_right_key(code) => state.move_image_viewer_next(),
        code if is_confirm_key(code) => state.open_image_viewer_action_menu(),
        _ => {}
    }

    None
}

fn handle_image_viewer_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_image_viewer_action_menu(),
        code if is_confirm_key(code) => return state.activate_selected_image_viewer_action(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_image_viewer_action_shortcut(shortcut);
        }
        _ => {}
    }

    None
}

fn handle_user_profile_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.close_user_profile_popup(),
        code if is_down_key(code) => state.scroll_user_profile_popup_down(),
        code if is_up_key(code) => state.scroll_user_profile_popup_up(),
        _ => {}
    }
    None
}

fn handle_member_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_member_action_menu(),
        code if is_down_key(code) => state.move_member_action_down(),
        code if is_up_key(code) => state.move_member_action_up(),
        code if is_confirm_key(code) => return state.activate_selected_member_action(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_member_action_shortcut(shortcut);
        }
        _ => {}
    }
    None
}

fn handle_guild_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_guild_action_menu(),
        code if is_down_key(code) => state.move_guild_action_down(),
        code if is_up_key(code) => state.move_guild_action_up(),
        code if is_confirm_key(code) => return state.activate_selected_guild_action(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_guild_action_shortcut(shortcut);
        }
        _ => {}
    }
    None
}

fn handle_channel_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        // Esc steps back to the action list when viewing threads, otherwise
        // closes the menu entirely.
        KeyCode::Esc => state.back_channel_action_menu(),
        code if is_left_key(code) && state.is_channel_action_threads_phase() => {
            state.back_channel_action_menu()
        }
        code if is_down_key(code) => state.move_channel_action_down(),
        code if is_up_key(code) => state.move_channel_action_up(),
        code if is_confirm_key(code) => return state.activate_selected_channel_action(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_channel_action_shortcut(shortcut);
        }
        _ => {}
    }

    None
}

fn handle_emoji_reaction_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_emoji_reaction_picker(),
        code if is_down_key(code) => state.move_emoji_reaction_down(),
        code if is_up_key(code) => state.move_emoji_reaction_up(),
        code if is_confirm_key(code) => return state.activate_selected_emoji_reaction(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            return state.activate_emoji_reaction_shortcut(shortcut);
        }
        _ => {}
    }

    None
}

fn handle_poll_vote_picker_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_poll_vote_picker(),
        code if is_down_key(code) => state.move_poll_vote_picker_down(),
        code if is_up_key(code) => state.move_poll_vote_picker_up(),
        KeyCode::Char(' ') => state.toggle_selected_poll_vote_answer(),
        KeyCode::Enter => return state.activate_poll_vote_picker(),
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            state.toggle_poll_vote_answer_shortcut(shortcut)
        }
        _ => {}
    }

    None
}

fn handle_reaction_users_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_reaction_users_popup(),
        code if is_down_key(code) => state.scroll_reaction_users_popup_down(),
        code if is_up_key(code) => state.scroll_reaction_users_popup_up(),
        KeyCode::PageDown => state.page_reaction_users_popup_down(),
        KeyCode::PageUp => state.page_reaction_users_popup_up(),
        _ => {}
    }

    None
}

fn handle_debug_log_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('`') => state.close_debug_log_popup(),
        _ => {}
    }

    None
}

fn handle_options_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('o') => state.close_options_popup(),
        code if is_down_key(code) => state.move_option_down(),
        code if is_up_key(code) => state.move_option_up(),
        code if is_confirm_key(code) => state.toggle_selected_display_option(),
        _ => {}
    }

    None
}

fn is_down_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('j') | KeyCode::Down)
}

fn is_up_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('k') | KeyCode::Up)
}

fn is_left_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('h') | KeyCode::Left)
}

fn is_right_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('l') | KeyCode::Right)
}

fn is_confirm_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' '))
}

fn is_shortcut_key(key: KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

fn handle_composer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.composer_mention_query().is_some()
        && let Some(command) = handle_mention_picker_key(state, key)
    {
        return command;
    }
    match key.code {
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            state.push_composer_char('\n');
            None
        }
        KeyCode::Enter => state.submit_composer(),
        KeyCode::Esc => {
            state.cancel_composer();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.quit();
            None
        }
        KeyCode::Backspace => {
            state.pop_composer_char();
            None
        }
        KeyCode::Char(value) => {
            state.push_composer_char(value);
            None
        }
        _ => None,
    }
}

/// Returns `Some(None)` to mean "the picker absorbed this key, don't fall
/// through to the regular composer handler", and `None` to mean "let the
/// composer handle this key normally."
fn handle_mention_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Up => {
            state.move_composer_mention_selection(-1);
            Some(None)
        }
        KeyCode::Down => {
            state.move_composer_mention_selection(1);
            Some(None)
        }
        KeyCode::Char('p') if ctrl => {
            state.move_composer_mention_selection(-1);
            Some(None)
        }
        KeyCode::Char('n') if ctrl => {
            state.move_composer_mention_selection(1);
            Some(None)
        }
        // Both Tab and Enter confirm the highlighted mention. Enter only
        // submits the message when the picker is closed.
        KeyCode::Tab | KeyCode::Enter => {
            if state.confirm_composer_mention() {
                Some(None)
            } else {
                state.cancel_composer_mention();
                Some(None)
            }
        }
        KeyCode::Esc => {
            state.cancel_composer_mention();
            Some(None)
        }
        _ => None,
    }
}
