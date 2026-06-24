use crossterm::event::{KeyCode, KeyEvent};

use crate::discord::AppCommand;
use crate::tui::keybindings::{
    AttachmentViewerAction, ChannelSwitcherAction, ComposerAction, DebugLogPopupAction,
    EmojiReactionPickerAction, KeyChord, MessageConfirmationAction, NotificationInboxAction,
    OptionsPopupAction, PollVotePickerAction, PopupListAction, ProfilePopupAction,
    ProfilePopupTabAction, ReactionUsersPopupAction, ScrollAction, SearchPopupAction,
    SelectionAction, SelectionKeySet,
};
use crate::tui::state::{ActiveModalPopupKind, DashboardState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PopupKeyPhase {
    Priority,
    Deferred,
}

pub(super) fn handle_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
    phase: PopupKeyPhase,
) -> Option<Option<AppCommand>> {
    let handled_page = phase == PopupKeyPhase::Priority
        && !state.key_bindings().is_popup_close_key(key)
        && match state.key_bindings().popup_page_action(key) {
            Some(SelectionAction::Next) => state.page_active_popup_down(),
            Some(SelectionAction::Previous) => state.page_active_popup_up(),
            None => false,
        };
    if handled_page {
        return Some(None);
    }

    let kind = state.active_modal_popup_kind()?;
    if popup_key_phase(kind) != phase {
        return None;
    }

    Some(match kind {
        ActiveModalPopupKind::KeymapHelp => handle_keymap_popup_key(state, key),
        ActiveModalPopupKind::DebugLog => handle_debug_log_popup_key(state, key),
        ActiveModalPopupKind::QuitConfirmation => handle_quit_confirmation_key(state, key),
        ActiveModalPopupKind::Options => handle_options_popup_key(state, key),
        ActiveModalPopupKind::ReactionUsers => handle_reaction_users_popup_key(state, key),
        ActiveModalPopupKind::MessageConfirmation => handle_message_confirmation_key(state, key),
        ActiveModalPopupKind::GuildLeaveConfirmation => {
            handle_guild_leave_confirmation_key(state, key)
        }
        ActiveModalPopupKind::ThreadDeleteConfirmation => {
            handle_thread_delete_confirmation_key(state, key)
        }
        ActiveModalPopupKind::PollVotePicker => handle_poll_vote_picker_key(state, key),
        ActiveModalPopupKind::EmojiReactionPicker => handle_emoji_reaction_picker_key(state, key),
        ActiveModalPopupKind::ChannelSwitcher => handle_channel_switcher_key(state, key),
        ActiveModalPopupKind::NotificationInbox => handle_notification_inbox_key(state, key),
        ActiveModalPopupKind::Search => handle_search_popup_key(state, key),
        ActiveModalPopupKind::ForumPostComposer => handle_forum_post_composer_key(state, key),
        ActiveModalPopupKind::ThreadEdit => handle_thread_edit_key(state, key),
        ActiveModalPopupKind::ThreadActionMenu => handle_thread_action_menu_key(state, key),
        ActiveModalPopupKind::Leader => super::leader::handle_leader_key(state, key),
        ActiveModalPopupKind::MessageUrlPicker => handle_message_url_picker_key(state, key),
        ActiveModalPopupKind::MessageActionMenu => handle_message_action_menu_key(state, key),
        ActiveModalPopupKind::AttachmentViewer => handle_attachment_viewer_key(state, key),
        ActiveModalPopupKind::UserProfile => handle_user_profile_popup_key(state, key),
    })
}

fn popup_key_phase(kind: ActiveModalPopupKind) -> PopupKeyPhase {
    match kind {
        ActiveModalPopupKind::KeymapHelp
        | ActiveModalPopupKind::DebugLog
        | ActiveModalPopupKind::QuitConfirmation
        | ActiveModalPopupKind::Options
        | ActiveModalPopupKind::ReactionUsers
        | ActiveModalPopupKind::MessageConfirmation
        | ActiveModalPopupKind::GuildLeaveConfirmation
        | ActiveModalPopupKind::ThreadDeleteConfirmation
        | ActiveModalPopupKind::PollVotePicker
        | ActiveModalPopupKind::EmojiReactionPicker => PopupKeyPhase::Priority,
        ActiveModalPopupKind::MessageActionMenu
        | ActiveModalPopupKind::MessageUrlPicker
        | ActiveModalPopupKind::AttachmentViewer
        | ActiveModalPopupKind::Leader
        | ActiveModalPopupKind::UserProfile
        | ActiveModalPopupKind::ChannelSwitcher
        | ActiveModalPopupKind::NotificationInbox
        | ActiveModalPopupKind::Search
        | ActiveModalPopupKind::ForumPostComposer
        | ActiveModalPopupKind::ThreadEdit
        | ActiveModalPopupKind::ThreadActionMenu => PopupKeyPhase::Deferred,
    }
}

pub(super) fn handle_forum_post_composer_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if state.is_forum_post_tag_picker_active() {
        return handle_forum_post_tag_picker_key(state, key);
    }
    if state.is_forum_post_composer_editing() {
        // Keep the text cursor on screen as the user types or moves it.
        state.request_forum_post_scroll_reveal();
        return handle_forum_post_composer_edit_key(state, key);
    }

    // The scroll keys (J/K and the arrows) pan the viewport without moving the
    // field selection, so long bodies stay readable.
    if let Some(action) = state.key_bindings().scroll_action(key) {
        state.scroll_forum_post_composer(action);
        return None;
    }

    // Anything else changes the focused field, so re-reveal it after handling.
    state.request_forum_post_scroll_reveal();

    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next => state.move_forum_post_selection_down(),
            SelectionAction::Previous => state.move_forum_post_selection_up(),
        }
        return None;
    }

    match key.code {
        KeyCode::Tab => {
            state.cycle_forum_post_field_next();
            return None;
        }
        KeyCode::BackTab => {
            state.cycle_forum_post_field_previous();
            return None;
        }
        _ => {}
    }

    match state.key_bindings().composer_action(key) {
        ComposerAction::Submit => return state.activate_forum_post_composer(),
        ComposerAction::Close => state.close_or_cancel_forum_post_composer(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::RemoveLastAttachment => state.pop_pending_forum_post_attachment(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::DeletePreviousChar
        | ComposerAction::DeletePreviousWord
        | ComposerAction::MoveCursorUp
        | ComposerAction::MoveCursorDown
        | ComposerAction::MoveCursorWordLeft
        | ComposerAction::MoveCursorLeft
        | ComposerAction::MoveCursorWordRight
        | ComposerAction::MoveCursorRight
        | ComposerAction::MoveCursorHome
        | ComposerAction::MoveCursorEnd
        | ComposerAction::InsertChar(_)
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_forum_post_tag_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next => state.move_forum_post_selection_down(),
            SelectionAction::Previous => state.move_forum_post_selection_up(),
        }
        return None;
    }

    match state.key_bindings().composer_action(key) {
        ComposerAction::Submit => return state.activate_forum_post_composer(),
        ComposerAction::Close => state.close_or_cancel_forum_post_composer(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::DeletePreviousChar
        | ComposerAction::DeletePreviousWord
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::MoveCursorUp
        | ComposerAction::MoveCursorDown
        | ComposerAction::MoveCursorWordLeft
        | ComposerAction::MoveCursorLeft
        | ComposerAction::MoveCursorWordRight
        | ComposerAction::MoveCursorRight
        | ComposerAction::MoveCursorHome
        | ComposerAction::MoveCursorEnd
        | ComposerAction::InsertChar(_)
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_forum_post_composer_edit_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().composer_action(key) {
        ComposerAction::PasteClipboard => state.request_paste_clipboard(),
        ComposerAction::InsertNewline => state.push_forum_post_char('\n'),
        ComposerAction::Submit => return state.activate_forum_post_composer(),
        ComposerAction::Close => state.close_or_cancel_forum_post_composer(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::DeletePreviousChar => state.delete_forum_post_previous_char(),
        ComposerAction::DeletePreviousWord => state.delete_forum_post_previous_word(),
        ComposerAction::MoveCursorWordLeft => state.move_forum_post_cursor_word_left(),
        ComposerAction::MoveCursorLeft => state.move_forum_post_cursor_left(),
        ComposerAction::MoveCursorWordRight => state.move_forum_post_cursor_word_right(),
        ComposerAction::MoveCursorRight => state.move_forum_post_cursor_right(),
        ComposerAction::MoveCursorHome => state.move_forum_post_cursor_home(),
        ComposerAction::MoveCursorEnd => state.move_forum_post_cursor_end(),
        ComposerAction::MoveCursorUp => state.move_forum_post_cursor_up(),
        ComposerAction::MoveCursorDown => state.move_forum_post_cursor_down(),
        ComposerAction::InsertChar(value) => state.push_forum_post_char(value),
        ComposerAction::RemoveLastAttachment => state.pop_pending_forum_post_attachment(),
        ComposerAction::OpenInEditor => state.request_open_forum_post_body_in_editor(),
        ComposerAction::Ignore => {}
    }
    None
}

pub(super) fn handle_thread_edit_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if state.is_thread_edit_tag_picker_active() {
        return handle_thread_edit_tag_picker_key(state, key);
    }
    if state.is_thread_edit_title_editing() {
        // Keep the text cursor on screen as the user types or moves it.
        state.request_thread_edit_scroll_reveal();
        return handle_thread_edit_title_key(state, key);
    }

    // The scroll keys (J/K and the arrows) pan the viewport without moving the
    // field selection. The selectors claim Left/Right and h/l below before this
    // runs.
    if !matches!(
        key.code,
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l')
    ) && let Some(action) = state.key_bindings().scroll_action(key)
    {
        state.scroll_thread_edit(action);
        return None;
    }

    // Anything else changes the focused field, so re-reveal it after handling.
    state.request_thread_edit_scroll_reveal();

    // Left/Right (or h/l) cycle the focused selector (slow mode / auto-archive).
    // These are no-ops on the non-selector fields.
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.cycle_thread_edit_selector(false);
            return None;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.cycle_thread_edit_selector(true);
            return None;
        }
        _ => {}
    }

    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next => state.move_thread_edit_selection_down(),
            SelectionAction::Previous => state.move_thread_edit_selection_up(),
        }
        return None;
    }

    match key.code {
        KeyCode::Tab => {
            state.cycle_thread_edit_field_next();
            return None;
        }
        KeyCode::BackTab => {
            state.cycle_thread_edit_field_previous();
            return None;
        }
        _ => {}
    }

    match state.key_bindings().composer_action(key) {
        ComposerAction::Submit => return state.activate_thread_edit(),
        ComposerAction::Close => state.close_or_cancel_thread_edit(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::DeletePreviousChar
        | ComposerAction::DeletePreviousWord
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::MoveCursorUp
        | ComposerAction::MoveCursorDown
        | ComposerAction::MoveCursorWordLeft
        | ComposerAction::MoveCursorLeft
        | ComposerAction::MoveCursorWordRight
        | ComposerAction::MoveCursorRight
        | ComposerAction::MoveCursorHome
        | ComposerAction::MoveCursorEnd
        | ComposerAction::InsertChar(_)
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_thread_edit_tag_picker_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next => state.move_thread_edit_selection_down(),
            SelectionAction::Previous => state.move_thread_edit_selection_up(),
        }
        return None;
    }

    match state.key_bindings().composer_action(key) {
        ComposerAction::Submit => return state.activate_thread_edit(),
        ComposerAction::Close => state.close_or_cancel_thread_edit(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::DeletePreviousChar
        | ComposerAction::DeletePreviousWord
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::MoveCursorUp
        | ComposerAction::MoveCursorDown
        | ComposerAction::MoveCursorWordLeft
        | ComposerAction::MoveCursorLeft
        | ComposerAction::MoveCursorWordRight
        | ComposerAction::MoveCursorRight
        | ComposerAction::MoveCursorHome
        | ComposerAction::MoveCursorEnd
        | ComposerAction::InsertChar(_)
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_thread_edit_title_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().composer_action(key) {
        ComposerAction::PasteClipboard => state.request_paste_clipboard(),
        ComposerAction::Submit => return state.activate_thread_edit(),
        ComposerAction::Close => state.close_or_cancel_thread_edit(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::DeletePreviousChar => state.delete_thread_edit_previous_char(),
        ComposerAction::DeletePreviousWord => state.delete_thread_edit_previous_word(),
        ComposerAction::MoveCursorWordLeft => state.move_thread_edit_cursor_word_left(),
        ComposerAction::MoveCursorLeft => state.move_thread_edit_cursor_left(),
        ComposerAction::MoveCursorWordRight => state.move_thread_edit_cursor_word_right(),
        ComposerAction::MoveCursorRight => state.move_thread_edit_cursor_right(),
        ComposerAction::MoveCursorHome => state.move_thread_edit_cursor_home(),
        ComposerAction::MoveCursorEnd => state.move_thread_edit_cursor_end(),
        ComposerAction::InsertChar(value) => state.push_thread_edit_char(value),
        // The title is a single line, so newline and the vertical/editor moves
        // and attachment shortcut do nothing here.
        ComposerAction::InsertNewline
        | ComposerAction::MoveCursorUp
        | ComposerAction::MoveCursorDown
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::OpenInEditor
        | ComposerAction::Ignore => {}
    }
    None
}

pub(super) fn handle_thread_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    fn activate_thread_action_shortcut(
        state: &mut DashboardState,
        shortcut: KeyChord,
    ) -> Option<AppCommand> {
        state
            .thread_action_shortcut_matches(shortcut)
            .then(|| state.activate_thread_action_shortcut(shortcut))?
    }

    // Esc (or a close key) backs out of the mute/notification submenu first,
    // then closes the menu.
    fn close_or_back(state: &mut DashboardState) {
        if !state.back_thread_action_menu() {
            state.close_thread_action_menu();
        }
    }

    handle_popup_list_key(
        state,
        key,
        close_or_back,
        DashboardState::move_thread_action_down,
        DashboardState::move_thread_action_up,
        DashboardState::activate_selected_thread_action,
        activate_thread_action_shortcut,
    )
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

pub(super) fn handle_notification_inbox_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    // While the "mark all read" prompt is up, keys answer it instead of driving
    // the list: confirm on Enter/y, cancel on anything else.
    if state.notification_inbox_is_confirming_mark_all() {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                state.confirm_mark_all_notification_inbox_read()
            }
            _ => {
                state.cancel_mark_all_notification_inbox_read();
                None
            }
        };
    }

    match state.key_bindings().notification_inbox_action(key) {
        Some(NotificationInboxAction::Select(SelectionAction::Next)) => {
            state.move_notification_inbox_down();
            None
        }
        Some(NotificationInboxAction::Select(SelectionAction::Previous)) => {
            state.move_notification_inbox_up();
            None
        }
        Some(NotificationInboxAction::SwitchTab(action)) => {
            state.switch_notification_inbox_tab(action);
            None
        }
        Some(NotificationInboxAction::Close) => {
            state.close_notification_inbox();
            None
        }
        Some(NotificationInboxAction::ActivateSelected) => {
            state.activate_selected_notification_inbox_item()
        }
        Some(NotificationInboxAction::MarkSelectedRead) => {
            state.mark_selected_notification_inbox_item_read()
        }
        Some(NotificationInboxAction::MarkAllRead) => {
            state.begin_mark_all_notification_inbox_read();
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
    handle_popup_list_key(
        state,
        key,
        DashboardState::close_message_url_picker,
        DashboardState::move_message_url_picker_down,
        DashboardState::move_message_url_picker_up,
        DashboardState::activate_selected_message_url,
        DashboardState::activate_message_url_shortcut,
    )
}

pub(super) fn handle_message_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    fn activate_message_action_shortcut(
        state: &mut DashboardState,
        shortcut: KeyChord,
    ) -> Option<AppCommand> {
        state
            .message_action_shortcut_matches(shortcut)
            .then(|| state.activate_message_action_shortcut(shortcut))?
    }

    handle_popup_list_key(
        state,
        key,
        DashboardState::close_message_action_menu,
        DashboardState::move_message_action_down,
        DashboardState::move_message_action_up,
        DashboardState::activate_selected_message_action,
        activate_message_action_shortcut,
    )
}

fn handle_popup_list_key(
    state: &mut DashboardState,
    key: KeyEvent,
    close: impl Fn(&mut DashboardState),
    move_down: impl Fn(&mut DashboardState),
    move_up: impl Fn(&mut DashboardState),
    activate_selected: impl Fn(&mut DashboardState) -> Option<AppCommand>,
    activate_shortcut: impl Fn(&mut DashboardState, KeyChord) -> Option<AppCommand>,
) -> Option<AppCommand> {
    match state.key_bindings().popup_list_action(key) {
        Some(PopupListAction::Close) => close(state),
        Some(PopupListAction::Select(SelectionAction::Next)) => move_down(state),
        Some(PopupListAction::Select(SelectionAction::Previous)) => move_up(state),
        Some(PopupListAction::ActivateSelected) => return activate_selected(state),
        Some(PopupListAction::ActivateShortcut(shortcut)) => {
            if let Some(command) = activate_shortcut(state, shortcut) {
                return Some(command);
            }
            if state.key_bindings().is_popup_close_key(key) {
                close(state);
            }
        }
        None => {}
    }

    None
}

pub(super) fn handle_message_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        DashboardState::confirm_message_confirmation,
        DashboardState::close_message_confirmation,
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

pub(super) fn handle_thread_delete_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    handle_confirmation_key(
        state,
        key,
        DashboardState::confirm_thread_delete,
        DashboardState::close_thread_delete_confirmation,
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
        Some(AttachmentViewerAction::PlaySelected) => {
            return state.play_selected_attachment_viewer_attachment();
        }
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
        Some(ProfilePopupAction::SignOut) => return state.sign_out_command(),
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
