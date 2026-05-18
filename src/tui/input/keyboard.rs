use std::path::{Path, PathBuf};

use crossterm::event::{KeyEvent, KeyEventKind};

use crate::discord::{AppCommand, MessageAttachmentUpload};
use crate::tui::keybindings::{
    ChannelSwitcherAction, ComposerAction, ComposerCompletionAction, DashboardAction,
    DebugLogPopupAction, EmojiReactionPickerAction, GlobalAction, ImageViewerAction, LeaderAction,
    LeaderActionMenuAction, MessageActionMenuAction, MessageConfirmationAction,
    MessageShortcutAction, OptionsPopupAction, PaneFilterAction, PollVotePickerAction,
    ProfilePopupAction, ReactionUsersPopupAction, ScrollAction, SelectionAction,
};

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

    if state.is_message_delete_confirmation_open() {
        return handle_message_delete_confirmation_key(state, key);
    }

    if state.is_message_pin_confirmation_open() {
        return handle_message_pin_confirmation_key(state, key);
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    // The debug log is intentionally available from regular dashboard modes,
    // but popups and the composer get first chance to handle their own keys.
    if matches!(
        state.key_bindings().global_action(key),
        Some(GlobalAction::ToggleDebugLog)
    ) {
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

    // Only intercept filter input when the pane that owns the filter is still
    // focused. Moving the mouse to another pane should let normal shortcuts
    // work (e.g. pressing `i` after clicking Messages).
    if (state.is_guild_pane_filter_active() && focus == FocusPane::Guilds)
        || (state.is_channel_pane_filter_active() && focus == FocusPane::Channels)
    {
        if let Some(command) = handle_pane_filter_key(state, key, focus) {
            return command;
        }
    }

    state
        .key_bindings()
        .dashboard_action(key, focus)
        .and_then(|action| handle_dashboard_action(state, focus, action))
}

fn handle_dashboard_action(
    state: &mut DashboardState,
    focus: FocusPane,
    action: DashboardAction,
) -> Option<AppCommand> {
    match action {
        DashboardAction::Select(SelectionAction::Next) => {
            state.move_down();
            None
        }
        DashboardAction::Select(SelectionAction::Previous) => {
            state.move_up();
            state.next_older_history_command()
        }
        DashboardAction::MessageShortcut(action) => handle_message_shortcut_action(state, action),
        DashboardAction::Back => {
            if !state.return_from_pinned_message_view() {
                state.return_from_opened_thread();
            }
            None
        }
        DashboardAction::Quit => {
            state.quit();
            None
        }
        DashboardAction::StartComposer => {
            state.start_composer();
            None
        }
        DashboardAction::OpenLeader => {
            state.open_leader();
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
            match focus {
                FocusPane::Guilds => state.open_guild_pane_filter(),
                FocusPane::Channels => state.open_channel_pane_filter(),
                _ => {}
            }
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
            state.half_page_down();
            None
        }
        DashboardAction::HalfPageUp => {
            state.half_page_up();
            state.next_older_history_command()
        }
        DashboardAction::JumpTop => {
            state.jump_top();
            None
        }
        DashboardAction::JumpBottom => {
            state.jump_bottom();
            None
        }
        DashboardAction::ScrollMessageViewportTop => {
            state.scroll_message_viewport_top();
            None
        }
        DashboardAction::ScrollMessageViewportBottom => {
            state.scroll_message_viewport_bottom();
            None
        }
        DashboardAction::ScrollMessageViewportDown => {
            state.scroll_message_viewport_down();
            None
        }
        DashboardAction::ScrollMessageViewportUp => {
            state.scroll_message_viewport_up();
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
                state.confirm_selected_guild();
                None
            }
            FocusPane::Channels => state.confirm_selected_channel_command(),
            FocusPane::Members => state.show_selected_member_profile(),
            FocusPane::Messages => state.activate_selected_message_pane_item(),
        },
        DashboardAction::OpenTreeNode => {
            match focus {
                FocusPane::Guilds => state.open_selected_folder(),
                FocusPane::Channels => state.open_selected_channel_category(),
                _ => {}
            }
            None
        }
        DashboardAction::CloseTreeNode => {
            match focus {
                FocusPane::Guilds => state.close_selected_folder(),
                FocusPane::Channels => state.close_selected_channel_category(),
                _ => {}
            }
            None
        }
    }
}

fn handle_message_shortcut_action(
    state: &mut DashboardState,
    action: MessageShortcutAction,
) -> Option<AppCommand> {
    match action {
        MessageShortcutAction::CopyContent => {
            state.direct_copy_selected_message_content();
            None
        }
        MessageShortcutAction::OpenReactionPicker => {
            state.direct_open_selected_message_reaction_picker();
            None
        }
        MessageShortcutAction::Reply => {
            state.direct_reply_to_selected_message();
            None
        }
        MessageShortcutAction::OpenDeleteConfirmation => {
            state.open_selected_message_delete_confirmation();
            None
        }
        MessageShortcutAction::Edit => {
            state.direct_edit_selected_message();
            None
        }
        MessageShortcutAction::OpenUrl => state.direct_open_selected_message_url(),
        MessageShortcutAction::ViewImage => {
            state.direct_open_selected_message_image_viewer();
            None
        }
        MessageShortcutAction::ShowProfile => state.direct_show_selected_message_profile(),
        MessageShortcutAction::OpenPinConfirmation => {
            state.direct_open_selected_message_pin_confirmation();
            None
        }
    }
}

fn handle_leader_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.is_leader_action_mode() {
        return handle_leader_action_key(state, key);
    }

    match state.key_bindings().leader_action(key) {
        LeaderAction::TogglePane(pane) => {
            state.toggle_pane_visibility(pane);
            state.close_leader();
        }
        LeaderAction::OpenActions => state.open_leader_actions_for_focused_target(),
        LeaderAction::OpenOptions => {
            state.open_options_category_picker();
            state.close_leader();
        }
        LeaderAction::OpenVoiceActions => state.open_voice_actions(),
        LeaderAction::OpenChannelSwitcher => state.open_channel_switcher(),
        LeaderAction::Close => state.close_leader(),
    }

    None
}

fn handle_channel_switcher_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().channel_switcher_action(key) {
        Some(ChannelSwitcherAction::Select(SelectionAction::Next)) => {
            state.move_channel_switcher_down();
            None
        }
        Some(ChannelSwitcherAction::Select(SelectionAction::Previous)) => {
            state.move_channel_switcher_up();
            None
        }
        Some(ChannelSwitcherAction::Close) => {
            state.close_channel_switcher();
            None
        }
        Some(ChannelSwitcherAction::ActivateSelected) => {
            state.activate_selected_channel_switcher_item()
        }
        Some(ChannelSwitcherAction::MoveQueryCursorLeft) => {
            state.move_channel_switcher_query_cursor_left();
            None
        }
        Some(ChannelSwitcherAction::MoveQueryCursorRight) => {
            state.move_channel_switcher_query_cursor_right();
            None
        }
        Some(ChannelSwitcherAction::DeleteQueryChar) => {
            state.pop_channel_switcher_char();
            None
        }
        Some(ChannelSwitcherAction::InsertQueryChar(value)) => {
            state.push_channel_switcher_char(value);
            None
        }
        None => None,
    }
}

fn handle_leader_action_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().leader_action_menu_action(key) {
        LeaderActionMenuAction::BackOrClose => {
            if state.is_message_url_picker_open() {
                state.close_or_back_message_action_menu();
                return None;
            }
            if state.back_channel_leader_action() || state.back_guild_leader_action() {
                return None;
            }
            state.close_all_action_contexts();
            state.close_leader();
            None
        }
        LeaderActionMenuAction::Close => {
            state.close_all_action_contexts();
            state.close_leader();
            None
        }
        LeaderActionMenuAction::ActivateShortcut(shortcut) => {
            let (matched, command) = state.activate_leader_action_shortcut(shortcut);
            if !matched || !state.is_any_action_context_active() {
                state.close_all_action_contexts();
                state.close_leader();
            }
            command
        }
        LeaderActionMenuAction::UnknownClose => {
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
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .to_owned();
            attachments.push(MessageAttachmentUpload::from_path(
                path,
                filename,
                metadata.len(),
            ));
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
    match state.key_bindings().message_action_menu_action(key) {
        Some(MessageActionMenuAction::Close) => state.close_or_back_message_action_menu(),
        Some(MessageActionMenuAction::Select(SelectionAction::Next)) => {
            state.move_message_action_down()
        }
        Some(MessageActionMenuAction::Select(SelectionAction::Previous)) => {
            state.move_message_action_up()
        }
        Some(MessageActionMenuAction::ActivateSelected) => {
            if state.is_message_url_picker_open() {
                return state.activate_selected_message_url();
            }
            return state.activate_selected_message_action();
        }
        Some(MessageActionMenuAction::ActivateShortcut(shortcut)) => {
            return state.activate_message_action_shortcut(shortcut);
        }
        None => {}
    }

    None
}

fn handle_message_delete_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().message_confirmation_action(key) {
        Some(MessageConfirmationAction::Confirm) => state.confirm_message_delete(),
        Some(MessageConfirmationAction::Cancel) => {
            state.close_message_delete_confirmation();
            None
        }
        None => None,
    }
}

fn handle_message_pin_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().message_confirmation_action(key) {
        Some(MessageConfirmationAction::Confirm) => state.confirm_message_pin(),
        Some(MessageConfirmationAction::Cancel) => {
            state.close_message_pin_confirmation();
            None
        }
        None => None,
    }
}

fn handle_image_viewer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().image_viewer_action(key) {
        Some(ImageViewerAction::Close) => state.close_image_viewer(),
        Some(ImageViewerAction::Previous) => state.move_image_viewer_previous(),
        Some(ImageViewerAction::Next) => state.move_image_viewer_next(),
        Some(ImageViewerAction::DownloadSelected) => {
            return state.download_selected_image_viewer_image();
        }
        None => {}
    }

    None
}

fn handle_user_profile_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().profile_popup_action(key) {
        Some(ProfilePopupAction::Close) => state.close_user_profile_popup(),
        Some(ProfilePopupAction::Scroll(ScrollAction::Down)) => {
            state.scroll_user_profile_popup_down()
        }
        Some(ProfilePopupAction::Scroll(ScrollAction::Up)) => state.scroll_user_profile_popup_up(),
        None => {}
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
    match state.key_bindings().pane_filter_action(key) {
        Some(PaneFilterAction::Select(SelectionAction::Next)) => {
            state.move_down();
            Some(None)
        }
        Some(PaneFilterAction::Select(SelectionAction::Previous)) => {
            state.move_up();
            Some(None)
        }
        Some(PaneFilterAction::Close) => {
            if guild_focused {
                state.close_guild_pane_filter();
            } else {
                state.close_channel_pane_filter();
            }
            Some(None)
        }
        Some(PaneFilterAction::Confirm) => {
            if guild_focused {
                state.confirm_guild_pane_filter();
                Some(None)
            } else {
                Some(state.confirm_channel_pane_filter())
            }
        }
        Some(PaneFilterAction::DeleteChar) => {
            if guild_focused {
                state.pop_guild_pane_filter_char();
            } else {
                state.pop_channel_pane_filter_char();
            }
            Some(None)
        }
        Some(PaneFilterAction::MoveCursorLeft) => {
            if guild_focused {
                state.move_guild_pane_filter_cursor_left();
            } else {
                state.move_channel_pane_filter_cursor_left();
            }
            Some(None)
        }
        Some(PaneFilterAction::MoveCursorRight) => {
            if guild_focused {
                state.move_guild_pane_filter_cursor_right();
            } else {
                state.move_channel_pane_filter_cursor_right();
            }
            Some(None)
        }
        Some(PaneFilterAction::Ignore) => Some(None),
        Some(PaneFilterAction::InsertChar(value)) => {
            if guild_focused {
                state.push_guild_pane_filter_char(value);
            } else {
                state.push_channel_pane_filter_char(value);
            }
            Some(None)
        }
        None => None, // fall through to normal navigation (arrows, j/k etc.)
    }
}

fn handle_emoji_reaction_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state
        .key_bindings()
        .emoji_reaction_picker_action(key, state.is_filtering_emoji_reactions())
    {
        Some(EmojiReactionPickerAction::Select(SelectionAction::Next)) => {
            state.move_emoji_reaction_down()
        }
        Some(EmojiReactionPickerAction::Select(SelectionAction::Previous)) => {
            state.move_emoji_reaction_up()
        }
        Some(EmojiReactionPickerAction::Close) => {
            state.close_emoji_reaction_picker();
            return None;
        }
        Some(EmojiReactionPickerAction::DeleteFilterChar) => {
            state.pop_emoji_reaction_filter_char();
            return None;
        }
        Some(EmojiReactionPickerAction::StartFilter) => {
            state.start_emoji_reaction_filter();
            return None;
        }
        Some(EmojiReactionPickerAction::InsertFilterChar(value)) => {
            state.push_emoji_reaction_filter_char(value);
            return None;
        }
        Some(EmojiReactionPickerAction::ActivateSelected) => {
            return state.activate_selected_emoji_reaction();
        }
        Some(EmojiReactionPickerAction::ActivateShortcut(shortcut)) => {
            return state.activate_emoji_reaction_shortcut(shortcut);
        }
        None => {}
    }

    None
}

fn handle_poll_vote_picker_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().poll_vote_picker_action(key) {
        Some(PollVotePickerAction::Close) => {
            state.close_poll_vote_picker();
            return None;
        }
        Some(PollVotePickerAction::Select(SelectionAction::Next)) => {
            state.move_poll_vote_picker_down()
        }
        Some(PollVotePickerAction::Select(SelectionAction::Previous)) => {
            state.move_poll_vote_picker_up()
        }
        Some(PollVotePickerAction::ToggleSelected) => state.toggle_selected_poll_vote_answer(),
        Some(PollVotePickerAction::Submit) => return state.activate_poll_vote_picker(),
        Some(PollVotePickerAction::ToggleShortcut(shortcut)) => {
            state.toggle_poll_vote_answer_shortcut(shortcut)
        }
        None => {}
    }

    None
}

fn handle_reaction_users_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().reaction_users_popup_action(key) {
        Some(ReactionUsersPopupAction::Close) => state.close_reaction_users_popup(),
        Some(ReactionUsersPopupAction::Scroll(ScrollAction::Down)) => {
            state.scroll_reaction_users_popup_down()
        }
        Some(ReactionUsersPopupAction::Scroll(ScrollAction::Up)) => {
            state.scroll_reaction_users_popup_up()
        }
        Some(ReactionUsersPopupAction::PageDown) => state.page_reaction_users_popup_down(),
        Some(ReactionUsersPopupAction::PageUp) => state.page_reaction_users_popup_up(),
        None => {}
    }

    None
}

fn handle_debug_log_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().debug_log_popup_action(key) {
        DebugLogPopupAction::Close => state.close_debug_log_popup(),
        DebugLogPopupAction::Ignore => {}
    }

    None
}

fn handle_options_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state
        .key_bindings()
        .options_popup_action(key, state.is_options_category_picker_open())
    {
        Some(OptionsPopupAction::Close) => state.close_options_popup(),
        Some(OptionsPopupAction::OpenCategory(shortcut)) => {
            state.open_options_category_shortcut(shortcut)
        }
        Some(OptionsPopupAction::Select(SelectionAction::Next)) => state.move_option_down(),
        Some(OptionsPopupAction::Select(SelectionAction::Previous)) => state.move_option_up(),
        Some(OptionsPopupAction::ToggleSelected) => state.toggle_selected_display_option(),
        Some(OptionsPopupAction::AdjustSelected(delta)) => {
            state.adjust_selected_display_option(delta)
        }
        None => {}
    }

    None
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

    match state.key_bindings().composer_action(key) {
        ComposerAction::OpenInEditor => {
            state.request_open_composer_in_editor();
            None
        }
        ComposerAction::InsertNewline => {
            state.push_composer_char('\n');
            None
        }
        ComposerAction::Submit => state.submit_composer(),
        ComposerAction::Close => {
            state.close_composer();
            None
        }
        ComposerAction::ClearInput => {
            state.clear_composer_input();
            None
        }
        ComposerAction::RemoveLastAttachment => {
            state.pop_pending_composer_attachment();
            None
        }
        ComposerAction::DeletePreviousChar => {
            state.pop_composer_char();
            None
        }
        ComposerAction::DeleteNextChar => {
            state.delete_composer_char();
            None
        }
        ComposerAction::MoveCursorUp => {
            state.move_composer_cursor_up();
            None
        }
        ComposerAction::MoveCursorDown => {
            state.move_composer_cursor_down();
            None
        }
        ComposerAction::MoveCursorWordLeft => {
            state.move_composer_cursor_word_left();
            None
        }
        ComposerAction::MoveCursorLeft => {
            state.move_composer_cursor_left();
            None
        }
        ComposerAction::MoveCursorWordRight => {
            state.move_composer_cursor_word_right();
            None
        }
        ComposerAction::MoveCursorRight => {
            state.move_composer_cursor_right();
            None
        }
        ComposerAction::MoveCursorHome => {
            state.move_composer_cursor_home();
            None
        }
        ComposerAction::MoveCursorEnd => {
            state.move_composer_cursor_end();
            None
        }
        ComposerAction::InsertChar(value) => {
            state.push_composer_char(value);
            None
        }
        ComposerAction::Ignore => None,
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
    match state.key_bindings().composer_completion_action(key) {
        ComposerCompletionAction::Select(SelectionAction::Next) => {
            move_selection(state, 1);
            Some(None)
        }
        ComposerCompletionAction::Select(SelectionAction::Previous) => {
            move_selection(state, -1);
            Some(None)
        }
        ComposerCompletionAction::Confirm => {
            if confirm(state) {
                Some(None)
            } else {
                cancel(state);
                Some(None)
            }
        }
        ComposerCompletionAction::Cancel => {
            cancel(state);
            Some(None)
        }
        ComposerCompletionAction::FallThrough => None,
    }
}
