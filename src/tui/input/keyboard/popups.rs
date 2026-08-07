use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::discord::AppCommand;
use crate::tui::keybindings::{
    AttachmentViewerAction, ChannelSwitcherAction, ComposerAction, EmojiReactionPickerAction,
    KeyChord, NotificationInboxAction, OptionsPopupAction, PollVotePickerAction, PopupKeyMapLookup,
    PopupListAction, ProfilePopupAction, ProfilePopupTabAction, ReactionUsersPopupAction,
    ScrollAction, SearchPopupAction, SelectionAction, SelectionKeySet,
    VoiceParticipantAudioPopupAction, push_to_talk_shortcut_from_key,
};
use crate::tui::state::{
    ActiveModalPopupKind, ConfirmationButton, DashboardState, PopupInputMode, PopupKeymapContext,
};

use super::is_key_sequence_cancel_key;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PopupKeyStage {
    /// Popup-owned commands that must win over configurable shared actions.
    Fixed,
    /// Text editing and local navigation after shared actions decline the key.
    Fallback,
}

enum PopupKeyDispatch {
    Handled(Option<AppCommand>),
    Unhandled,
}

pub(super) fn handle_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    let policy = state.active_popup_policy()?;
    let keymap_context = policy.keymap_context();

    // Once a sequence starts, it owns the next key. This makes every visible
    // Which Key hint truthful even when its next chord is also a local popup
    // shortcut. Esc cancels the hint without closing the underlying popup.
    if let Some(context) = keymap_context
        && state.popup_keymap_prefix(context).is_some()
    {
        if is_key_sequence_cancel_key(key) {
            state.close_key_sequence();
            return Some(None);
        }
        return Some(handle_popup_keymap_key(state, key, context).unwrap_or(None));
    }
    if state.is_key_sequence_active() {
        state.close_key_sequence();
    }

    // With no active sequence, Esc cancels the topmost popup layer, including
    // nested editors, confirmations, and raw shortcut capture.
    if is_key_sequence_cancel_key(key) {
        state.close_active_popup();
        return Some(None);
    }

    // Exclusive modes capture raw input and must not reinterpret it as a local
    // or configurable popup action.
    if policy.input_mode == PopupInputMode::Exclusive {
        state.close_key_sequence();
        return Some(dispatch_popup_fallback(state, policy.kind, key));
    }

    if let PopupKeyDispatch::Handled(command) =
        dispatch_popup_key(state, policy.kind, key, PopupKeyStage::Fixed)
    {
        state.close_key_sequence();
        return Some(command);
    }

    let close_key = match policy.input_mode {
        PopupInputMode::Routed => state.key_bindings().is_popup_close_key(key),
        PopupInputMode::TextEntry => state.key_bindings().is_text_entry_popup_close_key(key),
        PopupInputMode::Exclusive => false,
    };
    if close_key {
        state.close_key_sequence();
        state.close_active_popup();
        return Some(None);
    }

    if let Some(context) = keymap_context
        && let Some(command) = handle_popup_keymap_key(state, key, context)
    {
        return Some(command);
    }
    if keymap_context.is_none() {
        state.close_key_sequence();
    }

    let page_action = match policy.input_mode {
        PopupInputMode::Routed => state.key_bindings().physical_popup_page_action(key),
        PopupInputMode::TextEntry => state.key_bindings().text_safe_popup_page_action(key),
        PopupInputMode::Exclusive => None,
    };
    match page_action {
        // Paging to the bottom of a reactor list must still fetch the next page.
        Some(SelectionAction::Next) if state.page_active_popup_down() => {
            return Some(state.reaction_users_popup_take_load_more());
        }
        Some(SelectionAction::Previous) if state.page_active_popup_up() => {
            return Some(None);
        }
        _ => {}
    }

    Some(dispatch_popup_fallback(state, policy.kind, key))
}

fn dispatch_popup_key(
    state: &mut DashboardState,
    kind: ActiveModalPopupKind,
    key: KeyEvent,
    stage: PopupKeyStage,
) -> PopupKeyDispatch {
    match kind {
        ActiveModalPopupKind::KeymapHelp => {
            route_fallback_key(state, key, stage, handle_keymap_popup_key)
        }
        ActiveModalPopupKind::DebugLog => route_fallback_key(state, key, stage, ignore_popup_key),
        ActiveModalPopupKind::QuitConfirmation => route_confirmation_key(
            state,
            key,
            stage,
            |state| {
                state.confirm_quit();
                None
            },
            DashboardState::close_active_popup,
        ),
        ActiveModalPopupKind::Options => route_popup_key(
            state,
            key,
            stage,
            handle_options_popup_fixed_key,
            handle_options_popup_key,
        ),
        ActiveModalPopupKind::ReactionUsers => route_popup_key(
            state,
            key,
            stage,
            handle_reaction_users_popup_fixed_key,
            handle_reaction_users_popup_key,
        ),
        ActiveModalPopupKind::MessageConfirmation => route_confirmation_key(
            state,
            key,
            stage,
            DashboardState::confirm_message_confirmation,
            DashboardState::close_active_popup,
        ),
        ActiveModalPopupKind::GuildLeaveConfirmation => route_confirmation_key(
            state,
            key,
            stage,
            DashboardState::confirm_guild_leave,
            DashboardState::close_active_popup,
        ),
        ActiveModalPopupKind::ThreadDeleteConfirmation => route_confirmation_key(
            state,
            key,
            stage,
            DashboardState::confirm_thread_delete,
            DashboardState::close_active_popup,
        ),
        ActiveModalPopupKind::PollVotePicker => route_popup_key(
            state,
            key,
            stage,
            handle_poll_vote_picker_fixed_key,
            handle_poll_vote_picker_key,
        ),
        ActiveModalPopupKind::EmojiReactionPicker => route_popup_key(
            state,
            key,
            stage,
            handle_emoji_reaction_picker_fixed_key,
            handle_emoji_reaction_picker_key,
        ),
        ActiveModalPopupKind::ChannelSwitcher => {
            route_fallback_key(state, key, stage, handle_channel_switcher_key)
        }
        ActiveModalPopupKind::NotificationInbox => route_popup_key(
            state,
            key,
            stage,
            handle_notification_inbox_fixed_key,
            handle_notification_inbox_key,
        ),
        ActiveModalPopupKind::Search => {
            route_fallback_key(state, key, stage, handle_search_popup_key)
        }
        ActiveModalPopupKind::ForumPostComposer => route_popup_key(
            state,
            key,
            stage,
            handle_forum_post_composer_fixed_key,
            handle_forum_post_composer_key,
        ),
        ActiveModalPopupKind::ThreadEdit => route_popup_key(
            state,
            key,
            stage,
            handle_thread_edit_fixed_key,
            handle_thread_edit_key,
        ),
        ActiveModalPopupKind::ThreadActionMenu => route_action_menu_key(
            state,
            key,
            stage,
            ActionMenuCallbacks {
                shortcut_matches: DashboardState::thread_action_shortcut_matches,
                close_or_back: DashboardState::close_active_popup,
                move_down: DashboardState::move_thread_action_down,
                move_up: DashboardState::move_thread_action_up,
                activate_selected: DashboardState::activate_selected_thread_action,
                activate_shortcut: DashboardState::activate_thread_action_shortcut,
            },
        ),
        ActiveModalPopupKind::GuildActionMenu => route_action_menu_key(
            state,
            key,
            stage,
            ActionMenuCallbacks {
                shortcut_matches: DashboardState::guild_action_shortcut_matches,
                close_or_back: DashboardState::close_active_popup,
                move_down: DashboardState::move_guild_action_down,
                move_up: DashboardState::move_guild_action_up,
                activate_selected: DashboardState::activate_selected_guild_action,
                activate_shortcut: DashboardState::activate_guild_action_shortcut,
            },
        ),
        ActiveModalPopupKind::ChannelActionMenu => route_action_menu_key(
            state,
            key,
            stage,
            ActionMenuCallbacks {
                shortcut_matches: DashboardState::channel_action_shortcut_matches,
                close_or_back: DashboardState::close_active_popup,
                move_down: DashboardState::move_channel_action_down,
                move_up: DashboardState::move_channel_action_up,
                activate_selected: DashboardState::activate_selected_channel_action,
                activate_shortcut: DashboardState::activate_channel_action_shortcut,
            },
        ),
        ActiveModalPopupKind::MemberActionMenu => route_action_menu_key(
            state,
            key,
            stage,
            ActionMenuCallbacks {
                shortcut_matches: DashboardState::member_action_shortcut_matches,
                close_or_back: DashboardState::close_active_popup,
                move_down: DashboardState::move_member_action_down,
                move_up: DashboardState::move_member_action_up,
                activate_selected: DashboardState::activate_selected_member_action,
                activate_shortcut: DashboardState::activate_member_action_shortcut,
            },
        ),
        ActiveModalPopupKind::MessageUrlPicker => route_popup_key(
            state,
            key,
            stage,
            handle_message_url_picker_fixed_key,
            handle_message_url_picker_key,
        ),
        ActiveModalPopupKind::MessageActionMenu => route_action_menu_key(
            state,
            key,
            stage,
            ActionMenuCallbacks {
                shortcut_matches: DashboardState::message_action_shortcut_matches,
                close_or_back: DashboardState::close_active_popup,
                move_down: DashboardState::move_message_action_down,
                move_up: DashboardState::move_message_action_up,
                activate_selected: DashboardState::activate_selected_message_action,
                activate_shortcut: DashboardState::activate_message_action_shortcut,
            },
        ),
        ActiveModalPopupKind::AttachmentViewer => route_popup_key(
            state,
            key,
            stage,
            handle_attachment_viewer_fixed_key,
            ignore_popup_key,
        ),
        ActiveModalPopupKind::UserProfile => route_popup_key(
            state,
            key,
            stage,
            handle_user_profile_popup_fixed_key,
            handle_user_profile_popup_key,
        ),
        ActiveModalPopupKind::VoiceParticipantAudio => route_popup_key(
            state,
            key,
            stage,
            handle_voice_participant_audio_popup_fixed_key,
            handle_voice_participant_audio_popup_key,
        ),
    }
}

fn route_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
    stage: PopupKeyStage,
    fixed: fn(&mut DashboardState, KeyEvent) -> Option<Option<AppCommand>>,
    fallback: fn(&mut DashboardState, KeyEvent) -> Option<AppCommand>,
) -> PopupKeyDispatch {
    // Each popup declares only the stages it needs. Fixed handlers contain
    // shortcuts whose visible popup action must take priority. Fallback
    // handlers contain editing or navigation behavior that should run only
    // after close, configurable navigation, and paging have declined the key.
    match stage {
        PopupKeyStage::Fixed => fixed(state, key)
            .map(PopupKeyDispatch::Handled)
            .unwrap_or(PopupKeyDispatch::Unhandled),
        PopupKeyStage::Fallback => PopupKeyDispatch::Handled(fallback(state, key)),
    }
}

fn route_fallback_key(
    state: &mut DashboardState,
    key: KeyEvent,
    stage: PopupKeyStage,
    fallback: fn(&mut DashboardState, KeyEvent) -> Option<AppCommand>,
) -> PopupKeyDispatch {
    match stage {
        PopupKeyStage::Fixed => PopupKeyDispatch::Unhandled,
        PopupKeyStage::Fallback => PopupKeyDispatch::Handled(fallback(state, key)),
    }
}

fn dispatch_popup_fallback(
    state: &mut DashboardState,
    kind: ActiveModalPopupKind,
    key: KeyEvent,
) -> Option<AppCommand> {
    match dispatch_popup_key(state, kind, key, PopupKeyStage::Fallback) {
        PopupKeyDispatch::Handled(command) => command,
        PopupKeyDispatch::Unhandled => None,
    }
}

fn ignore_popup_key(_: &mut DashboardState, _: KeyEvent) -> Option<AppCommand> {
    None
}

fn handle_popup_keymap_key(
    state: &mut DashboardState,
    key: KeyEvent,
    context: PopupKeymapContext,
) -> Option<Option<AppCommand>> {
    let mut prefix = state
        .popup_keymap_prefix(context)
        .unwrap_or_default()
        .to_vec();
    let had_prefix = !prefix.is_empty();
    match state
        .key_bindings()
        .popup_keymap_lookup_with_key(&prefix, key, context.scope())
    {
        Some(PopupKeyMapLookup::Pending) => {
            prefix.push(state.key_bindings().keymap_chord_for_event(key));
            state.open_popup_keymap_prefix(context, prefix);
            Some(None)
        }
        Some(PopupKeyMapLookup::Action(action)) => {
            state.close_key_sequence();
            Some(state.execute_popup_keymap_action(action))
        }
        None if had_prefix => {
            state.close_key_sequence();
            Some(None)
        }
        None => None,
    }
}

fn handle_voice_participant_audio_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state
        .key_bindings()
        .voice_participant_audio_popup_action(key)?
    {
        VoiceParticipantAudioPopupAction::Select(action) => {
            state.move_voice_participant_audio_selection(action);
            None
        }
        VoiceParticipantAudioPopupAction::AdjustVolume(_)
        | VoiceParticipantAudioPopupAction::ToggleMuted => None,
    }
}

fn handle_voice_participant_audio_popup_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    match state
        .key_bindings()
        .voice_participant_audio_popup_action(key)?
    {
        VoiceParticipantAudioPopupAction::AdjustVolume(delta) => {
            Some(state.adjust_voice_participant_audio_volume(delta))
        }
        VoiceParticipantAudioPopupAction::ToggleMuted => {
            Some(state.activate_voice_participant_audio_field())
        }
        VoiceParticipantAudioPopupAction::Select(_) => None,
    }
}

fn handle_forum_post_composer_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.is_forum_post_tag_picker_active() {
        return (state.key_bindings().composer_action(key) == ComposerAction::Submit)
            .then(|| state.activate_forum_post_composer());
    }
    if state.is_forum_post_composer_editing() {
        return None;
    }

    match key.code {
        KeyCode::Char('s' | 'S') if shortcut_key(key, 's') => {
            Some(state.save_forum_post_composer())
        }
        KeyCode::Char('c' | 'C') if shortcut_key(key, 'c') => {
            state.close_forum_post_composer();
            Some(None)
        }
        KeyCode::Tab => {
            state.cycle_forum_post_field_next();
            Some(None)
        }
        KeyCode::BackTab => {
            state.cycle_forum_post_field_previous();
            Some(None)
        }
        _ => None,
    }
}

fn handle_forum_post_composer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
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

    let action = state.key_bindings().composer_action(key);

    match action {
        ComposerAction::Submit => return state.activate_forum_post_composer(),
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::RemoveLastAttachment => state.pop_pending_forum_post_attachment(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::EditText(_)
        | ComposerAction::InsertChar(_)
        | ComposerAction::ToggleReplyPing
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
        ComposerAction::Submit => {}
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::EditText(_)
        | ComposerAction::InsertChar(_)
        | ComposerAction::ToggleReplyPing
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_forum_post_composer_edit_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    let action = state.key_bindings().composer_action(key);

    match action {
        ComposerAction::PasteClipboard => state.request_paste_clipboard(),
        ComposerAction::InsertNewline => state.push_forum_post_char('\n'),
        ComposerAction::Submit => return state.activate_forum_post_composer(),
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_forum_post_active_field(),
        ComposerAction::InsertChar(value) => state.push_forum_post_char(value),
        ComposerAction::RemoveLastAttachment => state.pop_pending_forum_post_attachment(),
        ComposerAction::OpenInEditor => state.request_open_forum_post_body_in_editor(),
        ComposerAction::EditText(action) => state.edit_forum_post_active_text_input(action),
        ComposerAction::ToggleReplyPing => {}
        ComposerAction::Ignore => {}
    }
    None
}

fn handle_thread_edit_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.is_thread_edit_tag_picker_active() {
        return (state.key_bindings().composer_action(key) == ComposerAction::Submit)
            .then(|| state.activate_thread_edit());
    }
    if state.is_thread_edit_title_editing() {
        return None;
    }

    match key.code {
        KeyCode::Char('s' | 'S') if shortcut_key(key, 's') => Some(state.submit_thread_edit()),
        KeyCode::Char('c' | 'C') if shortcut_key(key, 'c') => {
            state.close_thread_edit();
            Some(None)
        }
        KeyCode::Tab => {
            state.cycle_thread_edit_field_next();
            Some(None)
        }
        KeyCode::BackTab => {
            state.cycle_thread_edit_field_previous();
            Some(None)
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.cycle_thread_edit_selector(false);
            Some(None)
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.cycle_thread_edit_selector(true);
            Some(None)
        }
        _ => None,
    }
}

fn handle_thread_edit_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
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

    let action = state.key_bindings().composer_action(key);

    match action {
        ComposerAction::Submit => return state.activate_thread_edit(),
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::EditText(_)
        | ComposerAction::InsertChar(_)
        | ComposerAction::ToggleReplyPing
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
        ComposerAction::Submit => {}
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::OpenInEditor
        | ComposerAction::PasteClipboard
        | ComposerAction::InsertNewline
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::EditText(_)
        | ComposerAction::InsertChar(_)
        | ComposerAction::ToggleReplyPing
        | ComposerAction::Ignore => {}
    }
    None
}

fn handle_thread_edit_title_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    let action = state.key_bindings().composer_action(key);

    match action {
        ComposerAction::PasteClipboard => state.request_paste_clipboard(),
        ComposerAction::Submit => return state.activate_thread_edit(),
        ComposerAction::Close => state.close_active_popup(),
        ComposerAction::ClearInput => state.clear_thread_edit_active_field(),
        ComposerAction::InsertChar(value) => state.push_thread_edit_char(value),
        // The title is a single line, so newline and the vertical/editor moves
        // and attachment shortcut do nothing here.
        ComposerAction::EditText(action) => state.edit_thread_edit_title_input(action),
        ComposerAction::InsertNewline
        | ComposerAction::RemoveLastAttachment
        | ComposerAction::OpenInEditor
        | ComposerAction::ToggleReplyPing
        | ComposerAction::Ignore => {}
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
            state.close_active_popup();
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

fn handle_notification_inbox_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.notification_inbox_is_confirming_mark_all() {
        return handle_confirmation_fixed_key(
            state,
            key,
            DashboardState::confirm_mark_all_notification_inbox_read,
            DashboardState::close_active_popup,
        );
    }

    let tab = state.notification_inbox_tab()?;
    match state.key_bindings().notification_inbox_action(key, tab)? {
        NotificationInboxAction::SwitchTab(action) => {
            state.switch_notification_inbox_tab(action);
            Some(None)
        }
        NotificationInboxAction::ActivateSelected => {
            Some(state.activate_selected_notification_inbox_item())
        }
        NotificationInboxAction::MarkSelectedRead => {
            Some(state.mark_selected_notification_inbox_item_read())
        }
        NotificationInboxAction::MarkAllRead => {
            state.begin_mark_all_notification_inbox_read();
            Some(None)
        }
        NotificationInboxAction::Select(_) => None,
    }
}

fn handle_notification_inbox_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.notification_inbox_is_confirming_mark_all() {
        return handle_confirmation_key(state, key);
    }

    let tab = state.notification_inbox_tab()?;
    match state.key_bindings().notification_inbox_action(key, tab) {
        Some(NotificationInboxAction::Select(SelectionAction::Next)) => {
            state.move_notification_inbox_down();
            None
        }
        Some(NotificationInboxAction::Select(SelectionAction::Previous)) => {
            state.move_notification_inbox_up();
            None
        }
        Some(NotificationInboxAction::SwitchTab(_)) => None,
        Some(
            NotificationInboxAction::ActivateSelected
            | NotificationInboxAction::MarkSelectedRead
            | NotificationInboxAction::MarkAllRead,
        ) => None,
        None => None,
    }
}

fn handle_search_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
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
            state.close_active_popup();
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

fn handle_message_url_picker_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if matches!(key.code, KeyCode::Char(_)) {
        let shortcut = state.key_bindings().keymap_chord_for_event(key);
        if state.message_url_shortcut_matches(shortcut) {
            return Some(state.activate_message_url_shortcut(shortcut));
        }
    }
    matches!(
        state.key_bindings().popup_list_action(key),
        Some(PopupListAction::ActivateSelected)
    )
    .then(|| state.activate_selected_message_url())
}

fn handle_message_url_picker_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    handle_popup_list_fallback_key(
        state,
        key,
        DashboardState::move_message_url_picker_down,
        DashboardState::move_message_url_picker_up,
    )
}

/// The per-scope state callbacks one action menu wires into
/// [`handle_action_menu_key`].
struct ActionMenuCallbacks {
    shortcut_matches: fn(&DashboardState, KeyChord) -> bool,
    close_or_back: fn(&mut DashboardState),
    move_down: fn(&mut DashboardState),
    move_up: fn(&mut DashboardState),
    activate_selected: fn(&mut DashboardState) -> Option<AppCommand>,
    activate_shortcut: fn(&mut DashboardState, KeyChord) -> Option<AppCommand>,
}

/// Shared key handling for the action menu family (message, thread, and the
/// server/channel/member menus). On top of the regular popup-list keys
/// (j/k/arrows, Enter, Esc):
/// - Left steps back like Esc so nested submenus can be exited without
///   closing the whole menu.
/// - A displayed action shortcut wins over the j/k selection keys, so menus
///   whose shortcuts overlap them (e.g. an action rebound to `j`) stay
///   activatable; arrows and Ctrl+n/p always navigate.
fn route_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
    stage: PopupKeyStage,
    menu: ActionMenuCallbacks,
) -> PopupKeyDispatch {
    match stage {
        PopupKeyStage::Fixed => handle_action_menu_fixed_key(state, key, menu)
            .map(PopupKeyDispatch::Handled)
            .unwrap_or(PopupKeyDispatch::Unhandled),
        PopupKeyStage::Fallback => {
            PopupKeyDispatch::Handled(handle_action_menu_key(state, key, menu))
        }
    }
}

fn handle_action_menu_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
    menu: ActionMenuCallbacks,
) -> Option<Option<AppCommand>> {
    if key.code == KeyCode::Left && key.modifiers.is_empty() {
        (menu.close_or_back)(state);
        return Some(None);
    }
    if matches!(key.code, KeyCode::Char(_)) {
        let shortcut = state.key_bindings().keymap_chord_for_event(key);
        if (menu.shortcut_matches)(state, shortcut) {
            return Some((menu.activate_shortcut)(state, shortcut));
        }
    }
    matches!(
        state.key_bindings().popup_list_action(key),
        Some(PopupListAction::ActivateSelected)
    )
    .then(|| (menu.activate_selected)(state))
}

fn handle_action_menu_key(
    state: &mut DashboardState,
    key: KeyEvent,
    menu: ActionMenuCallbacks,
) -> Option<AppCommand> {
    handle_popup_list_fallback_key(state, key, menu.move_down, menu.move_up)
}

fn handle_popup_list_fallback_key(
    state: &mut DashboardState,
    key: KeyEvent,
    move_down: impl Fn(&mut DashboardState),
    move_up: impl Fn(&mut DashboardState),
) -> Option<AppCommand> {
    match state.key_bindings().popup_list_action(key) {
        Some(PopupListAction::Select(SelectionAction::Next)) => move_down(state),
        Some(PopupListAction::Select(SelectionAction::Previous)) => move_up(state),
        Some(PopupListAction::ActivateSelected | PopupListAction::ActivateShortcut(_)) => {}
        None => {}
    }

    None
}

fn route_confirmation_key(
    state: &mut DashboardState,
    key: KeyEvent,
    stage: PopupKeyStage,
    confirm: fn(&mut DashboardState) -> Option<AppCommand>,
    cancel: fn(&mut DashboardState),
) -> PopupKeyDispatch {
    match stage {
        PopupKeyStage::Fixed => handle_confirmation_fixed_key(state, key, confirm, cancel)
            .map(PopupKeyDispatch::Handled)
            .unwrap_or(PopupKeyDispatch::Unhandled),
        PopupKeyStage::Fallback => PopupKeyDispatch::Handled(handle_confirmation_key(state, key)),
    }
}

fn handle_confirmation_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        match action {
            SelectionAction::Next | SelectionAction::Previous => state.next_confirmation_button(),
        }
        return None;
    }

    None
}

fn handle_confirmation_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
    confirm: impl FnOnce(&mut DashboardState) -> Option<AppCommand>,
    cancel: impl FnOnce(&mut DashboardState),
) -> Option<Option<AppCommand>> {
    if shortcut_key(key, 'y') {
        return Some(confirm(state));
    }
    if shortcut_key(key, 'n') {
        cancel(state);
        return Some(None);
    }

    match key.code {
        KeyCode::Tab | KeyCode::BackTab => {
            state.next_confirmation_button();
            Some(None)
        }
        KeyCode::Enter => Some(match state.active_confirmation_button() {
            ConfirmationButton::Confirm => confirm(state),
            ConfirmationButton::Cancel => {
                cancel(state);
                None
            }
        }),
        _ => None,
    }
}

fn shortcut_key(key: KeyEvent, expected: char) -> bool {
    let KeyCode::Char(value) = key.code else {
        return false;
    };
    value.eq_ignore_ascii_case(&expected)
        && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
}

fn handle_attachment_viewer_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    match state.key_bindings().attachment_viewer_action(key)? {
        AttachmentViewerAction::Previous => {
            state.move_attachment_viewer_previous();
            Some(None)
        }
        AttachmentViewerAction::Next => {
            state.move_attachment_viewer_next();
            Some(None)
        }
        AttachmentViewerAction::OpenSelected => {
            Some(state.open_selected_attachment_viewer_attachment())
        }
        AttachmentViewerAction::CopyUrl => {
            state.copy_selected_attachment_viewer_url();
            Some(None)
        }
        AttachmentViewerAction::PlaySelected => {
            Some(state.play_selected_attachment_viewer_attachment())
        }
        AttachmentViewerAction::DownloadSelected => {
            Some(state.download_selected_attachment_viewer_attachment())
        }
        AttachmentViewerAction::ToggleZoom => {
            state.toggle_attachment_viewer_fullscreen();
            Some(None)
        }
        AttachmentViewerAction::ZoomIn => {
            state.zoom_attachment_viewer_in();
            Some(None)
        }
        AttachmentViewerAction::ZoomOut => {
            state.zoom_attachment_viewer_out();
            Some(None)
        }
    }
}

fn handle_user_profile_popup_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.is_user_profile_status_picker_open() {
        return (key.code == KeyCode::Enter).then(|| state.activate_user_profile_status_picker());
    }
    if state.is_user_profile_activity_picker_open() {
        return (key.code == KeyCode::Enter).then(|| state.activate_user_profile_activity_picker());
    }
    if state.is_user_profile_popup_editing() {
        return None;
    }

    match state.key_bindings().profile_popup_action(key, false)? {
        ProfilePopupAction::Close if shortcut_key(key, 'c') => {
            state.close_active_popup();
            Some(None)
        }
        ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Global) => {
            state.switch_user_profile_settings_to_global();
            Some(None)
        }
        ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Guild) => {
            state.switch_user_profile_settings_to_guild();
            Some(None)
        }
        ProfilePopupAction::StartOrCommitEdit => Some(state.start_or_commit_user_profile_edit()),
        ProfilePopupAction::PasteClipboard => {
            state.request_user_profile_avatar_clipboard_paste();
            Some(None)
        }
        ProfilePopupAction::Save => Some(state.save_user_profile_settings_command()),
        ProfilePopupAction::SignOut => Some(state.sign_out_command()),
        ProfilePopupAction::Close
        | ProfilePopupAction::Scroll(_)
        | ProfilePopupAction::NextField
        | ProfilePopupAction::PreviousField
        | ProfilePopupAction::EditText(_)
        | ProfilePopupAction::InsertChar(_) => None,
    }
}

fn handle_user_profile_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.is_user_profile_status_picker_open() {
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

    if state.is_user_profile_activity_picker_open() {
        if let Some(action) = state
            .key_bindings()
            .selection_action(key, SelectionKeySet::Navigation)
        {
            match action {
                SelectionAction::Next => state.move_user_profile_activity_picker_down(),
                SelectionAction::Previous => state.move_user_profile_activity_picker_up(),
            }
        }
        return None;
    }

    match state
        .key_bindings()
        .profile_popup_action(key, state.is_user_profile_popup_editing())
    {
        Some(ProfilePopupAction::Close) => state.close_active_popup(),
        Some(ProfilePopupAction::Scroll(ScrollAction::Down)) => {
            state.scroll_user_profile_popup_down()
        }
        Some(ProfilePopupAction::Scroll(ScrollAction::Up)) => state.scroll_user_profile_popup_up(),
        Some(ProfilePopupAction::NextField) => state.next_user_profile_settings_field(),
        Some(ProfilePopupAction::PreviousField) => state.previous_user_profile_settings_field(),
        Some(ProfilePopupAction::SwitchTab(_)) => {}
        Some(ProfilePopupAction::StartOrCommitEdit) => {
            if state.is_user_profile_popup_editing() {
                return state.start_or_commit_user_profile_edit();
            }
        }
        Some(ProfilePopupAction::PasteClipboard) => {
            if state.is_user_profile_popup_editing() {
                state.request_paste_clipboard();
            }
        }
        Some(ProfilePopupAction::Save | ProfilePopupAction::SignOut) => {}
        Some(ProfilePopupAction::EditText(action)) => state.edit_user_profile_text_input(action),
        Some(ProfilePopupAction::InsertChar(value)) => state.push_user_profile_edit_char(value),
        None => {}
    }

    None
}

fn handle_emoji_reaction_picker_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.is_editing_emoji_reaction_filter() {
        return None;
    }
    if let KeyCode::Char(shortcut) = key.code
        && state.emoji_reaction_shortcut_matches(state.key_bindings().keymap_chord_for_event(key))
    {
        return Some(state.activate_emoji_reaction_shortcut(shortcut));
    }

    match state
        .key_bindings()
        .emoji_reaction_picker_action(key, false)?
    {
        EmojiReactionPickerAction::ActivateSelected => {
            Some(state.activate_selected_emoji_reaction())
        }
        EmojiReactionPickerAction::StartFilter => {
            state.start_emoji_reaction_filter();
            Some(None)
        }
        EmojiReactionPickerAction::Select(_)
        | EmojiReactionPickerAction::DeleteFilterChar
        | EmojiReactionPickerAction::CommitFilter
        | EmojiReactionPickerAction::InsertFilterChar(_)
        | EmojiReactionPickerAction::ActivateShortcut(_) => None,
    }
}

fn handle_emoji_reaction_picker_key(
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
        Some(EmojiReactionPickerAction::DeleteFilterChar) => {
            state.pop_emoji_reaction_filter_char();
            return None;
        }
        Some(EmojiReactionPickerAction::CommitFilter) => {
            state.commit_emoji_reaction_filter();
            return None;
        }
        Some(EmojiReactionPickerAction::StartFilter) => return None,
        Some(EmojiReactionPickerAction::InsertFilterChar(value)) => {
            state.push_emoji_reaction_filter_char(value);
            return None;
        }
        Some(
            EmojiReactionPickerAction::ActivateSelected
            | EmojiReactionPickerAction::ActivateShortcut(_),
        ) => return None,
        None => {}
    }

    None
}

fn handle_poll_vote_picker_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if let KeyCode::Char(shortcut) = key.code
        && state.poll_vote_shortcut_matches(state.key_bindings().keymap_chord_for_event(key))
    {
        state.toggle_poll_vote_answer_shortcut(shortcut);
        return Some(None);
    }

    match state.key_bindings().poll_vote_picker_action(key)? {
        PollVotePickerAction::ToggleSelected => {
            state.toggle_selected_poll_vote_answer();
            Some(None)
        }
        PollVotePickerAction::Submit => Some(state.activate_poll_vote_picker()),
        PollVotePickerAction::Select(_) | PollVotePickerAction::ToggleShortcut(_) => None,
    }
}

fn handle_poll_vote_picker_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match state.key_bindings().poll_vote_picker_action(key) {
        Some(PollVotePickerAction::Select(SelectionAction::Next)) => {
            state.move_poll_vote_picker_down()
        }
        Some(PollVotePickerAction::Select(SelectionAction::Previous)) => {
            state.move_poll_vote_picker_up()
        }
        Some(PollVotePickerAction::ToggleSelected | PollVotePickerAction::Submit) => {}
        Some(PollVotePickerAction::ToggleShortcut(shortcut)) => {
            state.toggle_poll_vote_answer_shortcut(shortcut)
        }
        None => {}
    }

    None
}

fn handle_reaction_users_popup_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    match state.key_bindings().reaction_users_popup_action(key)? {
        ReactionUsersPopupAction::Back => {
            state.reaction_users_popup_back();
            Some(None)
        }
        ReactionUsersPopupAction::Activate => Some(state.activate_reaction_users_popup()),
        ReactionUsersPopupAction::Navigate(_) => None,
    }
}

fn handle_reaction_users_popup_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<AppCommand> {
    match state.key_bindings().reaction_users_popup_action(key) {
        Some(ReactionUsersPopupAction::Back | ReactionUsersPopupAction::Activate) => None,
        Some(ReactionUsersPopupAction::Navigate(action)) => {
            state.navigate_reaction_users_popup(action)
        }
        None => None,
    }
}

fn handle_keymap_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if let Some(action) = state
        .key_bindings()
        .selection_action(key, SelectionKeySet::Navigation)
    {
        state.scroll_keymap_popup(action);
    }

    None
}

fn handle_options_popup_fixed_key(
    state: &mut DashboardState,
    key: KeyEvent,
) -> Option<Option<AppCommand>> {
    if state.is_capturing_push_to_talk_shortcut() {
        return None;
    }

    match state
        .key_bindings()
        .options_popup_action(key, state.is_options_category_picker_open())?
    {
        OptionsPopupAction::Close if key.code == KeyCode::Char('o') => {
            state.close_active_popup();
            Some(None)
        }
        OptionsPopupAction::OpenCategory(shortcut) => {
            state.open_options_category_from_shortcut(shortcut);
            Some(None)
        }
        OptionsPopupAction::ToggleSelected => {
            state.toggle_selected_display_option();
            Some(None)
        }
        OptionsPopupAction::AdjustSelected(delta) => {
            state.adjust_selected_display_option(delta);
            Some(None)
        }
        OptionsPopupAction::Close | OptionsPopupAction::Select(_) => None,
    }
}

fn handle_options_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if state.is_capturing_push_to_talk_shortcut() {
        match push_to_talk_shortcut_from_key(key) {
            Ok(shortcut) => state.capture_push_to_talk_shortcut(shortcut),
            Err(error) => state.show_error_toast(error, std::time::Instant::now()),
        }
        return None;
    }

    match state
        .key_bindings()
        .options_popup_action(key, state.is_options_category_picker_open())
    {
        Some(OptionsPopupAction::Close) => state.close_active_popup(),
        Some(OptionsPopupAction::OpenCategory(_)) => {}
        Some(OptionsPopupAction::Select(SelectionAction::Next)) => state.move_option_down(),
        Some(OptionsPopupAction::Select(SelectionAction::Previous)) => state.move_option_up(),
        Some(OptionsPopupAction::ToggleSelected | OptionsPopupAction::AdjustSelected(_)) => {}
        None => {}
    }

    None
}
