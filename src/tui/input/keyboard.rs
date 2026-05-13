use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::{AppCommand, MessageAttachmentUpload};

use super::super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_keymap_popup_open() {
        return handle_keymap_popup_key(state, key);
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

    if state.is_image_viewer_open() {
        return handle_image_viewer_key(state, key);
    }

    if state.is_user_profile_popup_open() {
        return handle_user_profile_popup_key(state, key);
    }

    let focus = state.focus();
    let kb = state.key_bindings().clone();

    // Only intercept filter input when the pane that owns the filter is still
    // focused. Moving the mouse to another pane should let normal keybinds
    // work (e.g. pressing the open_composer key after clicking Messages).
    if (state.is_guild_pane_filter_active() && focus == FocusPane::Guilds)
        || (state.is_channel_pane_filter_active() && focus == FocusPane::Channels)
    {
        state.adjust_focused_pane_width(-1);
    } else if (key.code == KeyCode::Char('l') || key.code == KeyCode::Right)
        && key.modifiers.contains(KeyModifiers::ALT)
    {
        state.adjust_focused_pane_width(1);
    } else if kb.move_down.matches(key) || key.code == KeyCode::Down {
        state.move_down();
    } else if kb.scroll_viewport_down.matches(key) && focus == FocusPane::Messages {
        state.scroll_message_viewport_down();
    } else if kb.scroll_pane_right.matches(key) {
        state.scroll_focused_pane_horizontal_right();
    } else if kb.move_up.matches(key) || key.code == KeyCode::Up {
        state.move_up();
        return state.next_older_history_command();
    } else if kb.scroll_viewport_up.matches(key) && focus == FocusPane::Messages {
        state.scroll_message_viewport_up();
    } else if kb.scroll_pane_left.matches(key) {
        state.scroll_focused_pane_horizontal_left();
    } else if kb.half_page_down.matches(key) || key.code == KeyCode::PageDown {
        state.half_page_down();
    } else if kb.half_page_up.matches(key) || key.code == KeyCode::PageUp {
        state.half_page_up();
        return state.next_older_history_command();
    } else if kb.jump_top.matches(key) {
        state.jump_top();
    } else if key.code == KeyCode::Home {
        if focus == FocusPane::Messages {
            state.scroll_message_viewport_top();
        } else {
            state.jump_top();
        }
    } else if kb.jump_bottom.matches(key) {
        state.jump_bottom();
    } else if key.code == KeyCode::End {
        if focus == FocusPane::Messages {
            state.scroll_message_viewport_bottom();
        } else {
            state.jump_bottom();
        }
    } else if key.code == KeyCode::BackTab {
        state.cycle_focus_backward();
    } else if key.code == KeyCode::Tab {
        state.cycle_focus();
    } else if kb.pane_search.matches(key) {
        // Tree headers act like a small tree: Enter toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        match focus {
            FocusPane::Guilds => state.open_guild_pane_filter(),
            FocusPane::Channels => state.open_channel_pane_filter(),
            _ => {}
        }
    } else if key.code == KeyCode::Enter && focus == FocusPane::Guilds {
        state.confirm_selected_guild();
    } else if key.code == KeyCode::Enter && focus == FocusPane::Channels {
        return state.confirm_selected_channel_command();
    } else if key.code == KeyCode::Enter && focus == FocusPane::Members {
        return state.show_selected_member_profile();
    } else if key.code == KeyCode::Enter && focus == FocusPane::Messages {
        return state.activate_selected_message_pane_item();
    } else if is_right_key(key.code) && focus == FocusPane::Guilds {
        state.open_selected_folder();
    } else if is_left_key(key.code) && focus == FocusPane::Guilds {
        state.close_selected_folder();
    } else if is_right_key(key.code) && focus == FocusPane::Channels {
        state.open_selected_channel_category();
    } else if is_left_key(key.code) && focus == FocusPane::Channels {
        state.close_selected_channel_category();
    }

    None
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
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.close_leader(),
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
            if state.back_channel_leader_action() || state.back_guild_leader_action() {
                return None;
            }
            state.close_all_action_contexts();
            state.close_leader();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.close_all_action_contexts();
            state.close_leader();
            None
        }
        KeyCode::Char(shortcut) if is_shortcut_key(key) => {
            let (matched, command) = state.activate_leader_action_shortcut(shortcut);
            if !matched || !state.is_any_action_context_active() {
                state.close_all_action_contexts();
                state.close_leader();
            }
            command
        }
        code if is_left_key(code) => {
            if !state.back_channel_leader_action() && !state.back_guild_leader_action() {
                state.close_all_action_contexts();
                state.close_leader();
            }
            None
        }
        _ => {
            state.close_all_action_contexts();
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
        KeyCode::Char('d') if is_shortcut_key(key) => {
            return state.download_selected_image_viewer_image();
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

/// Returns `Some(command)` when the filter handler has fully handled the key
/// and the caller should return that command. Returns `None` when the key
/// should fall through to normal navigation (e.g. j/k to scroll the list).
fn handle_pane_filter_key(
    state: &mut DashboardState,
    key: KeyEvent,
    focus: FocusPane,
) -> Option<Option<AppCommand>> {
    let guild_focused = focus == FocusPane::Guilds;
    match key.code {
        KeyCode::Esc => {
            if guild_focused {
                state.close_guild_pane_filter();
            } else {
                state.close_channel_pane_filter();
            }
            Some(None)
        }
        KeyCode::Enter => {
            if guild_focused {
                state.confirm_guild_pane_filter();
                Some(None)
            } else {
                Some(state.confirm_channel_pane_filter())
            }
        }
        KeyCode::Backspace => {
            if guild_focused {
                state.pop_guild_pane_filter_char();
            } else {
                state.pop_channel_pane_filter_char();
            }
            Some(None)
        }
        KeyCode::Left => {
            if guild_focused {
                state.move_guild_pane_filter_cursor_left();
            } else {
                state.move_channel_pane_filter_cursor_left();
            }
            Some(None)
        }
        KeyCode::Right => {
            if guild_focused {
                state.move_guild_pane_filter_cursor_right();
            } else {
                state.move_channel_pane_filter_cursor_right();
            }
            Some(None)
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.quit();
            Some(None)
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_down();
            Some(None)
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_up();
            Some(None)
        }
        KeyCode::Char(value) if is_shortcut_key(key) => {
            if guild_focused {
                state.push_guild_pane_filter_char(value);
            } else {
                state.push_channel_pane_filter_char(value);
            }
            Some(None)
        }
        _ => None, // fall through to normal navigation (arrows, j/k etc.)
    }
}

fn handle_emoji_reaction_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_emoji_reaction_picker(),
        KeyCode::Backspace if state.is_filtering_emoji_reactions() => {
            state.pop_emoji_reaction_filter_char();
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_emoji_reaction_down();
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_emoji_reaction_up();
        }
        KeyCode::Char('/') if is_shortcut_key(key) && !state.is_filtering_emoji_reactions() => {
            state.start_emoji_reaction_filter();
        }
        KeyCode::Char(value) if is_shortcut_key(key) && state.is_filtering_emoji_reactions() => {
            state.push_emoji_reaction_filter_char(value);
        }
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

fn handle_keymap_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => state.close_keymap_popup(),
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
    if state.composer_emoji_query().is_some()
        && let Some(command) = handle_emoji_picker_key(state, key)
    {
        return command;
    }

    // Check configurable bindings before the fixed-key match below.
    if state.key_bindings().open_in_editor.matches(key) {
        state.request_open_composer_in_editor();
        return None;
    }

    match key.code {
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
            state.push_composer_char('\n');
            None
        }
        KeyCode::Enter => state.submit_composer(),
        KeyCode::Esc => {
            state.close_composer();
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.clear_composer_input();
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
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_composer_cursor_word_left();
            None
        }
        KeyCode::Left => {
            state.move_composer_cursor_left();
            None
        }
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.move_composer_cursor_word_right();
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
    handle_composer_completion_picker_key(
        state,
        key,
        DashboardState::move_composer_mention_selection,
        DashboardState::confirm_composer_mention,
        DashboardState::cancel_composer_mention,
    )
}

fn handle_emoji_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    handle_composer_completion_picker_key(
        state,
        key,
        DashboardState::move_composer_emoji_selection,
        DashboardState::confirm_composer_emoji,
        DashboardState::cancel_composer_emoji,
    )
}

fn handle_composer_completion_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
    mut move_selection: impl FnMut(&mut DashboardState, isize),
    mut confirm: impl FnMut(&mut DashboardState) -> bool,
    mut cancel: impl FnMut(&mut DashboardState),
) -> Option<Option<AppCommand>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Up => {
            move_selection(state, -1);
            Some(None)
        }
        KeyCode::Down => {
            move_selection(state, 1);
            Some(None)
        }
        KeyCode::Char('p') if ctrl => {
            move_selection(state, -1);
            Some(None)
        }
        KeyCode::Char('n') if ctrl => {
            move_selection(state, 1);
            Some(None)
        }
        KeyCode::Tab | KeyCode::Enter => {
            if confirm(state) {
                Some(None)
            } else {
                cancel(state);
                Some(None)
            }
        }
        KeyCode::Esc => {
            cancel(state);
            Some(None)
        }
        _ => None,
    }
}
