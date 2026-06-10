use crossterm::event::{KeyCode, KeyEvent};

use crate::discord::AppCommand;
use crate::tui::keybindings::{
    AttachmentViewerAction, ChannelSwitcherAction, DebugLogPopupAction, EmojiReactionPickerAction,
    KeyChord, MessageConfirmationAction, OptionsPopupAction, PollVotePickerAction, PopupListAction,
    ProfilePopupAction, ProfilePopupTabAction, ReactionUsersPopupAction, ScrollAction,
    SearchPopupAction, SelectionAction, SelectionKeySet,
};
use crate::tui::state::{ActiveModalPopupKind, DashboardState};

pub(super) fn handle_priority_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if handle_popup_page_key(state, key) {
        return Some(None);
    }

    match state.active_modal_popup_kind()? {
        ActiveModalPopupKind::KeymapHelp => Some(handle_keymap_popup_key(state, key)),
        ActiveModalPopupKind::DebugLog => Some(handle_debug_log_popup_key(state, key)),
        ActiveModalPopupKind::QuitConfirmation => Some(handle_quit_confirmation_key(state, key)),
        ActiveModalPopupKind::Options => Some(handle_options_popup_key(state, key)),
        ActiveModalPopupKind::ReactionUsers => Some(handle_reaction_users_popup_key(state, key)),
        ActiveModalPopupKind::MessageDeleteConfirmation => {
            Some(handle_message_delete_confirmation_key(state, key))
        }
        ActiveModalPopupKind::MessagePinConfirmation => {
            Some(handle_message_pin_confirmation_key(state, key))
        }
        ActiveModalPopupKind::GuildLeaveConfirmation => {
            Some(handle_guild_leave_confirmation_key(state, key))
        }
        ActiveModalPopupKind::PollVotePicker => Some(handle_poll_vote_picker_key(state, key)),
        ActiveModalPopupKind::EmojiReactionPicker => {
            Some(handle_emoji_reaction_picker_key(state, key))
        }
        ActiveModalPopupKind::MessageActionMenu
        | ActiveModalPopupKind::MessageUrlPicker
        | ActiveModalPopupKind::AttachmentViewer
        | ActiveModalPopupKind::Leader
        | ActiveModalPopupKind::UserProfile
        | ActiveModalPopupKind::ChannelSwitcher
        | ActiveModalPopupKind::Search => None,
    }
}

pub(super) fn handle_deferred_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    match state.active_modal_popup_kind()? {
        ActiveModalPopupKind::ChannelSwitcher => Some(handle_channel_switcher_key(state, key)),
        ActiveModalPopupKind::Search => Some(handle_search_popup_key(state, key)),
        ActiveModalPopupKind::Leader => Some(super::leader::handle_leader_key(state, key)),
        ActiveModalPopupKind::MessageUrlPicker => Some(handle_message_url_picker_key(state, key)),
        ActiveModalPopupKind::MessageActionMenu => Some(handle_message_action_menu_key(state, key)),
        ActiveModalPopupKind::AttachmentViewer => Some(handle_attachment_viewer_key(state, key)),
        ActiveModalPopupKind::UserProfile => Some(handle_user_profile_popup_key(state, key)),
        ActiveModalPopupKind::MessageDeleteConfirmation
        | ActiveModalPopupKind::MessagePinConfirmation
        | ActiveModalPopupKind::QuitConfirmation
        | ActiveModalPopupKind::GuildLeaveConfirmation
        | ActiveModalPopupKind::Options
        | ActiveModalPopupKind::EmojiReactionPicker
        | ActiveModalPopupKind::PollVotePicker
        | ActiveModalPopupKind::ReactionUsers
        | ActiveModalPopupKind::DebugLog
        | ActiveModalPopupKind::KeymapHelp => None,
    }
}

pub(super) fn handle_channel_switcher_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
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

pub(super) fn handle_search_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().search_popup_action(key) {
        Some(SearchPopupAction::Select(SelectionAction::Next)) => state.move_search_result_down(),
        Some(SearchPopupAction::Select(SelectionAction::Previous)) => {
            state.move_search_result_up();
            None
        }
        Some(SearchPopupAction::Page(SelectionAction::Next)) => state.page_search_result_down(),
        Some(SearchPopupAction::Page(SelectionAction::Previous)) => {
            state.page_search_result_up();
            None
        }
        Some(SearchPopupAction::Close) => {
            state.close_search_popup();
            None
        }
        Some(SearchPopupAction::ActivateSelected) => state.activate_search_popup(),
        Some(SearchPopupAction::NextField) => {
            state.cycle_search_field_next();
            None
        }
        Some(SearchPopupAction::PreviousField) => {
            state.cycle_search_field_previous();
            None
        }
        Some(SearchPopupAction::MoveCursorLeft) => {
            state.move_search_cursor_left();
            None
        }
        Some(SearchPopupAction::MoveCursorRight) => {
            state.move_search_cursor_right();
            None
        }
        Some(SearchPopupAction::DeleteChar) => {
            state.pop_search_char();
            None
        }
        Some(SearchPopupAction::InsertChar(value)) => {
            state.push_search_char(value);
            None
        }
        None => None,
    }
}

pub(super) fn handle_message_url_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().popup_list_action(key) {
        Some(PopupListAction::Close) => state.close_message_url_picker(),
        Some(PopupListAction::Select(SelectionAction::Next)) => {
            state.move_message_url_picker_down()
        }
        Some(PopupListAction::Select(SelectionAction::Previous)) => {
            state.move_message_url_picker_up()
        }
        Some(PopupListAction::ActivateSelected) => {
            return state.activate_selected_message_url();
        }
        Some(PopupListAction::ActivateShortcut(shortcut)) => {
            if let Some(command) = state.activate_message_url_shortcut(shortcut) {
                return Some(command);
            }
            if state.key_bindings().is_popup_close_key(key) {
                state.close_message_url_picker();
            }
        }
        None => {}
    }

    None
}

pub(super) fn handle_message_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().popup_list_action(key) {
        Some(PopupListAction::Close) => state.close_message_action_menu(),
        Some(PopupListAction::Select(SelectionAction::Next)) => state.move_message_action_down(),
        Some(PopupListAction::Select(SelectionAction::Previous)) => state.move_message_action_up(),
        Some(PopupListAction::ActivateSelected) => {
            return state.activate_selected_message_action();
        }
        Some(PopupListAction::ActivateShortcut(shortcut)) => {
            if message_action_shortcut_matches(state, shortcut) {
                return state.activate_message_action_shortcut(shortcut);
            }
            if state.key_bindings().is_popup_close_key(key) {
                state.close_message_action_menu();
            }
        }
        None => {}
    }

    None
}

fn message_action_shortcut_matches(state: &DashboardState, shortcut: KeyChord) -> bool {
    let actions = state.selected_message_action_items();
    state
        .key_bindings()
        .matching_action_shortcut_index(
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.message_action_shortcuts(actions, index),
            |action| action.enabled,
        )
        .is_some()
}

pub(super) fn handle_message_delete_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        DashboardState::confirm_message_delete,
        DashboardState::close_message_delete_confirmation,
    )
}

pub(super) fn handle_quit_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        |state| {
            state.confirm_quit();
            None
        },
        DashboardState::close_quit_confirmation,
    )
}

pub(super) fn handle_message_pin_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        DashboardState::confirm_message_pin,
        DashboardState::close_message_pin_confirmation,
    )
}

pub(super) fn handle_guild_leave_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        DashboardState::confirm_guild_leave,
        DashboardState::close_guild_leave_confirmation,
    )
}

fn handle_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
    confirm: impl FnOnce(&mut DashboardState) -> Option<AppCommand>,
    cancel: impl FnOnce(&mut DashboardState),
) -> Option<AppCommand> {
    match state.key_bindings().message_confirmation_action(key) {
        Some(MessageConfirmationAction::Confirm) => confirm(state),
        Some(MessageConfirmationAction::Cancel) => {
            cancel(state);
            None
        }
        None => None,
    }
}

pub(super) fn handle_attachment_viewer_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().attachment_viewer_action(key) {
        Some(AttachmentViewerAction::Close) => state.close_attachment_viewer(),
        Some(AttachmentViewerAction::Previous) => state.move_attachment_viewer_previous(),
        Some(AttachmentViewerAction::Next) => state.move_attachment_viewer_next(),
        Some(AttachmentViewerAction::DownloadSelected) => {
            return state.download_selected_attachment_viewer_attachment();
        }
        Some(AttachmentViewerAction::ToggleZoom) => state.toggle_attachment_viewer_fullscreen(),
        Some(AttachmentViewerAction::ZoomIn) => state.zoom_attachment_viewer_in(),
        Some(AttachmentViewerAction::ZoomOut) => state.zoom_attachment_viewer_out(),
        None => {}
    }

    None
}

pub(super) fn handle_user_profile_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if state.is_user_profile_status_picker_open() {
        if state.key_bindings().is_popup_close_key(key) {
            state.close_user_profile_status_picker();
            return None;
        }
        if key.code == KeyCode::Enter {
            return state.activate_user_profile_status_picker();
        }
        if let Some(action) = state
            .key_bindings()
            .selection_action(key, SelectionKeySet::Navigation)
        {
            match action {
                SelectionAction::Next => state.move_user_profile_status_picker_down(),
                SelectionAction::Previous => state.move_user_profile_status_picker_up(),
            }
        }
        return None;
    }

    match state
        .key_bindings()
        .profile_popup_action(key, state.is_user_profile_popup_editing())
    {
        Some(ProfilePopupAction::Close) => state.close_or_cancel_user_profile_popup(),
        Some(ProfilePopupAction::Scroll(ScrollAction::Down)) => {
            state.scroll_user_profile_popup_down()
        }
        Some(ProfilePopupAction::Scroll(ScrollAction::Up)) => state.scroll_user_profile_popup_up(),
        Some(ProfilePopupAction::NextField) => state.next_user_profile_settings_field(),
        Some(ProfilePopupAction::PreviousField) => state.previous_user_profile_settings_field(),
        Some(ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Global)) => {
            state.switch_user_profile_settings_to_global()
        }
        Some(ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Guild)) => {
            state.switch_user_profile_settings_to_guild()
        }
        Some(ProfilePopupAction::StartOrCommitEdit) => {
            return state.start_or_commit_user_profile_edit();
        }
        Some(ProfilePopupAction::PasteClipboard) => {
            if state.is_user_profile_popup_editing() {
                state.request_paste_clipboard();
            } else {
                state.request_user_profile_avatar_clipboard_paste();
            }
        }
        Some(ProfilePopupAction::Save) => return state.save_user_profile_settings_command(),
        Some(ProfilePopupAction::DeleteChar) => state.pop_user_profile_edit_char(),
        Some(ProfilePopupAction::DeletePreviousWord) => {
            state.delete_previous_user_profile_edit_word()
        }
        Some(ProfilePopupAction::MoveCursorLeft) => state.move_user_profile_edit_cursor_left(),
        Some(ProfilePopupAction::MoveCursorRight) => state.move_user_profile_edit_cursor_right(),
        Some(ProfilePopupAction::MoveCursorWordLeft) => {
            state.move_user_profile_edit_cursor_word_left()
        }
        Some(ProfilePopupAction::MoveCursorWordRight) => {
            state.move_user_profile_edit_cursor_word_right()
        }
        Some(ProfilePopupAction::MoveCursorHome) => state.move_user_profile_edit_cursor_home(),
        Some(ProfilePopupAction::MoveCursorEnd) => state.move_user_profile_edit_cursor_end(),
        Some(ProfilePopupAction::InsertChar(value)) => state.push_user_profile_edit_char(value),
        None => {}
    }

    None
}

pub(super) fn handle_popup_page_key(state: &mut DashboardState, key: KeyEvent) -> bool {
    let Some(action) = state.key_bindings().popup_page_action(key) else {
        return false;
    };

    match action {
        SelectionAction::Next => state.page_active_popup_down(),
        SelectionAction::Previous => state.page_active_popup_up(),
    }
}

/// Returns `Some(command)` when the filter handler has fully handled the key
/// and the caller should return that command. Returns `None` when the key
/// should fall through to normal navigation (e.g. j/k to scroll the list).
pub(super) fn handle_emoji_reaction_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state
        .key_bindings()
        .emoji_reaction_picker_action(key, state.is_editing_emoji_reaction_filter())
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
        Some(EmojiReactionPickerAction::CommitFilter) => {
            state.commit_emoji_reaction_filter();
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

pub(super) fn handle_poll_vote_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
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

pub(super) fn handle_reaction_users_popup_key(
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
        None => {}
    }

    None
}

pub(super) fn handle_debug_log_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if let Some(DebugLogPopupAction::Close) = state.key_bindings().debug_log_popup_action(key) {
        state.close_debug_log_popup();
    }

    None
}

pub(super) fn handle_keymap_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if state.key_bindings().is_popup_close_key(key) {
        state.close_keymap_popup();
        return None;
    }

    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        state.scroll_keymap_popup(action);
    }

    None
}

pub(super) fn handle_options_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state
        .key_bindings()
        .options_popup_action(key, state.is_options_category_picker_open())
    {
        Some(OptionsPopupAction::Close) => state.close_options_popup(),
        Some(OptionsPopupAction::OpenCategory(shortcut)) => {
            state.open_options_category_from_shortcut(shortcut)
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
