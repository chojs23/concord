use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::{AppCommand, MessageAttachmentUpload};

use super::super::state::{
    DashboardState, FocusPane, PendingNumericPrefix, PendingNumericPrefixAction,
};

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

    if state.is_channel_switcher_open() {
        return handle_channel_switcher_key(state, key);
    }

    if state.is_leader_active() {
        return handle_leader_key(state, key);
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

    match handle_pending_numeric_prefix(state, key) {
        PendingNumericPrefixOutcome::NotHandled => {}
        PendingNumericPrefixOutcome::Handled(command) => return command,
    }

    let focus = state.focus();
    match key.code {
        KeyCode::Esc if !state.return_from_pinned_message_view() => {
            state.return_from_opened_thread();
        }
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char(' ') if is_shortcut_key(key) => state.open_leader(),
        KeyCode::Char('1') => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: 1,
                action: Some(PendingNumericPrefixAction::FocusPane(FocusPane::Guilds)),
            }));
        }
        KeyCode::Char('2') => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: 2,
                action: Some(PendingNumericPrefixAction::FocusPane(FocusPane::Channels)),
            }));
        }
        KeyCode::Char('3') => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: 3,
                action: Some(PendingNumericPrefixAction::FocusPane(FocusPane::Messages)),
            }));
        }
        KeyCode::Char('4') => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: 4,
                action: Some(PendingNumericPrefixAction::FocusPane(FocusPane::Members)),
            }));
        }
        KeyCode::Char(value) if value.is_ascii_digit() && is_shortcut_key(key) => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: value.to_digit(10).map(u64::from).unwrap_or_default(),
                action: None,
            }));
        }
        KeyCode::Char('m') if is_shortcut_key(key) => return state.mute_focused_target(None),
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
        KeyCode::BackTab => state.cycle_focus_backward(),
        KeyCode::Tab => state.cycle_focus(),
        // Tree headers act like a small tree: Enter toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        KeyCode::Enter if focus == FocusPane::Guilds => state.confirm_selected_guild(),
        KeyCode::Enter if focus == FocusPane::Channels => {
            return state.confirm_selected_channel_command();
        }
        KeyCode::Enter if focus == FocusPane::Members => {
            return state.show_selected_member_profile();
        }
        KeyCode::Enter if focus == FocusPane::Messages => {
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

enum PendingNumericPrefixOutcome {
    NotHandled,
    Handled(Option<AppCommand>),
}

fn handle_pending_numeric_prefix(
    state: &mut DashboardState,
    key: KeyEvent,
) -> PendingNumericPrefixOutcome {
    let Some(prefix) = state.pending_numeric_prefix() else {
        return PendingNumericPrefixOutcome::NotHandled;
    };

    match key.code {
        KeyCode::Char(value) if value.is_ascii_digit() && is_shortcut_key(key) => {
            state.set_pending_numeric_prefix(Some(PendingNumericPrefix {
                count: prefix
                    .count
                    .saturating_mul(10)
                    .saturating_add(u64::from(value.to_digit(10).unwrap_or_default())),
                action: None,
            }));
            PendingNumericPrefixOutcome::Handled(None)
        }
        KeyCode::Char('m') if is_shortcut_key(key) => {
            state.clear_pending_numeric_prefix();
            PendingNumericPrefixOutcome::Handled(state.mute_focused_target(Some(prefix.count)))
        }
        _ => {
            state.execute_pending_numeric_prefix();
            PendingNumericPrefixOutcome::NotHandled
        }
    }
}

fn handle_leader_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.is_leader_action_mode() {
        return handle_leader_action_key(state, key);
    }

    match key.code {
        KeyCode::Char('1') if is_shortcut_key(key) => {
            state.toggle_pane_visibility(FocusPane::Guilds);
            state.close_leader();
        }
        KeyCode::Char('2') if is_shortcut_key(key) => {
            state.toggle_pane_visibility(FocusPane::Channels);
            state.close_leader();
        }
        KeyCode::Char('4') if is_shortcut_key(key) => {
            state.toggle_pane_visibility(FocusPane::Members);
            state.close_leader();
        }
        KeyCode::Char('a') if is_shortcut_key(key) => {
            state.open_leader_actions_for_focused_target()
        }
        KeyCode::Char('o') if is_shortcut_key(key) => {
            state.open_options_popup();
            state.close_leader();
        }
        KeyCode::Char(' ') if is_shortcut_key(key) => state.open_channel_switcher(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.close_leader();
            state.quit();
        }
        KeyCode::Esc => state.close_leader(),
        _ => state.close_leader(),
    }

    None
}

fn handle_channel_switcher_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => {
            state.close_channel_switcher();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.close_channel_switcher();
            state.quit();
            None
        }
        KeyCode::Enter => state.activate_selected_channel_switcher_item(),
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_channel_switcher_down();
            None
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_channel_switcher_up();
            None
        }
        KeyCode::Left => {
            state.move_channel_switcher_query_cursor_left();
            None
        }
        KeyCode::Right => {
            state.move_channel_switcher_query_cursor_right();
            None
        }
        KeyCode::Backspace => {
            state.pop_channel_switcher_char();
            None
        }
        KeyCode::Char(value) if is_shortcut_key(key) => {
            state.push_channel_switcher_char(value);
            None
        }
        _ => None,
    }
}

fn handle_leader_action_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => {
            state.close_all_action_menus();
            state.close_leader();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.close_all_action_menus();
            state.close_leader();
            state.quit();
            None
        }
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            let (matched, command) = state.activate_leader_action_shortcut(shortcut);
            if !matched || !state.is_channel_action_threads_phase() {
                state.close_all_action_menus();
                state.close_leader();
            }
            command
        }
        _ => {
            state.close_all_action_menus();
            state.close_leader();
            None
        }
    }
}

pub fn handle_paste(state: &mut DashboardState, text: &str) -> bool {
    if !state.is_composing() {
        return false;
    }

    if state.composer_accepts_attachments() {
        if let Some(attachments) = pasted_file_attachments(text) {
            state.add_pending_composer_attachments(attachments);
            return true;
        }
    }

    let pasted: String = text.chars().filter(|value| *value != '\r').collect();
    if pasted.is_empty() {
        return false;
    }
    state.insert_composer_text_at_cursor(&pasted);
    true
}

fn pasted_file_attachments(text: &str) -> Option<Vec<MessageAttachmentUpload>> {
    let mut attachments = Vec::new();
    for line in meaningful_paste_lines(text) {
        let values = if let Some(path) = pasted_file_path(line).filter(|path| path.is_file()) {
            vec![path.to_string_lossy().into_owned()]
        } else {
            shell_path_words(line)?
        };
        for value in values {
            let path = pasted_file_path(&value)?;
            if !path.is_file() {
                return None;
            }
            let metadata = path.metadata().ok()?;
            attachments.push(MessageAttachmentUpload {
                filename: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("attachment")
                    .to_owned(),
                path,
                size_bytes: metadata.len(),
            });
        }
    }
    (!attachments.is_empty()).then_some(attachments)
}

fn meaningful_paste_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| *line != "copy" && *line != "cut")
        .filter(|line| *line != "x-special/gnome-copied-files")
        .filter(|line| !line.starts_with('#'))
}

fn shell_path_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(value) = chars.next() {
        match value {
            '\\' if !in_single_quote => {
                current.push(chars.next()?);
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            value if value.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(value),
        }
    }

    if in_single_quote || in_double_quote {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

fn pasted_file_path(value: &str) -> Option<PathBuf> {
    if let Some(uri_path) = value.strip_prefix("file://") {
        return file_uri_path(uri_path);
    }

    let path = Path::new(value);
    path.is_absolute().then(|| path.to_path_buf())
}

fn file_uri_path(uri_path: &str) -> Option<PathBuf> {
    let path = uri_path.strip_prefix("localhost").unwrap_or(uri_path);
    if !path.starts_with('/') {
        return None;
    }
    percent_decode(path).map(PathBuf::from)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex_value(high)? * 16 + hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
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
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_option_down()
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_option_up()
        }
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
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.pop_pending_composer_attachment();
            None
        }
        KeyCode::Backspace => {
            state.pop_composer_char();
            None
        }
        KeyCode::Delete => {
            state.delete_composer_char();
            None
        }
        KeyCode::Left => {
            state.move_composer_cursor_left();
            None
        }
        KeyCode::Right => {
            state.move_composer_cursor_right();
            None
        }
        KeyCode::Home => {
            state.move_composer_cursor_home();
            None
        }
        KeyCode::End => {
            state.move_composer_cursor_end();
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
