use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_debug_log_popup_open() {
        return handle_debug_log_popup_key(state, key);
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
        KeyCode::Esc => {
            state.return_from_opened_thread();
        }
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('a') if focus == FocusPane::Channels => {
            state.open_selected_channel_actions();
        }
        KeyCode::Char('a') if focus == FocusPane::Members => {
            state.open_selected_member_actions();
        }
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('1') => state.focus_pane(FocusPane::Guilds),
        KeyCode::Char('2') => state.focus_pane(FocusPane::Channels),
        KeyCode::Char('3') => state.focus_pane(FocusPane::Messages),
        KeyCode::Char('4') => state.focus_pane(FocusPane::Members),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('J') if focus == FocusPane::Messages => state.scroll_message_viewport_down(),
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
            return state.next_older_history_command();
        }
        KeyCode::Char('K') if focus == FocusPane::Messages => state.scroll_message_viewport_up(),
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
        KeyCode::Char('F') => state.toggle_message_auto_follow(),
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
            state.open_selected_message_actions()
        }
        KeyCode::Right if focus == FocusPane::Guilds => state.open_selected_folder(),
        KeyCode::Left if focus == FocusPane::Guilds => state.close_selected_folder(),
        KeyCode::Right if focus == FocusPane::Channels => state.open_selected_channel_category(),
        KeyCode::Left if focus == FocusPane::Channels => state.close_selected_channel_category(),
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
        _ => {}
    }

    None
}

fn handle_user_profile_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.close_user_profile_popup(),
        code if is_down_key(code) => state.move_user_profile_popup_down(),
        code if is_up_key(code) => state.move_user_profile_popup_up(),
        code if is_confirm_key(code) => {
            return state.activate_selected_user_profile_mutual();
        }
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
        _ => {}
    }
    None
}

fn handle_channel_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        // Esc steps back to the action list when viewing threads, otherwise
        // closes the menu entirely.
        KeyCode::Esc => state.back_channel_action_menu(),
        KeyCode::Left if state.is_channel_action_threads_phase() => {
            state.back_channel_action_menu()
        }
        code if is_down_key(code) => state.move_channel_action_down(),
        code if is_up_key(code) => state.move_channel_action_up(),
        code if is_confirm_key(code) => return state.activate_selected_channel_action(),
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

fn is_down_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('j') | KeyCode::Down)
}

fn is_up_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('k') | KeyCode::Up)
}

fn is_confirm_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' '))
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

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::handle_key;
    use crate::{
        discord::{
            AppCommand, AppEvent, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, GuildFolder,
            MessageReferenceInfo, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji,
            ReactionUserInfo, ReactionUsersInfo,
        },
        tui::state::{
            ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MessageActionKind,
        },
    };

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn char_key(value: char) -> KeyEvent {
        key(KeyCode::Char(value))
    }

    fn ctrl_key(value: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(value), KeyModifiers::CONTROL)
    }

    fn shift_enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
    }

    #[test]
    fn enter_and_space_toggle_selected_folder() {
        let mut state = state_with_folder();
        state.focus_pane(FocusPane::Guilds);

        handle_key(&mut state, key(KeyCode::Enter));
        assert_selected_folder_collapsed(&state, true);

        handle_key(&mut state, char_key(' '));
        assert_selected_folder_collapsed(&state, false);
    }

    #[test]
    fn enter_and_space_toggle_selected_channel_category() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);

        handle_key(&mut state, key(KeyCode::Enter));
        assert_selected_channel_category_collapsed(&state, true);

        handle_key(&mut state, char_key(' '));
        assert_selected_channel_category_collapsed(&state, false);
    }

    #[test]
    fn movement_waits_for_enter_to_activate_channel() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);

        assert_eq!(state.selected_channel_id(), None);

        handle_key(&mut state, key(KeyCode::Down));
        assert_eq!(state.selected_channel_id(), None);

        let command = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(AppCommand::SubscribeGuildChannel {
                guild_id: Id::new(1),
                channel_id: Id::new(11),
            })
        );
        assert_eq!(state.selected_channel_id(), Some(Id::new(11)));

        handle_key(&mut state, key(KeyCode::Down));
        let command = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(AppCommand::SubscribeGuildChannel {
                guild_id: Id::new(1),
                channel_id: Id::new(12),
            })
        );
        assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    }

    #[test]
    fn enter_on_direct_message_subscribes_channel() {
        let mut state = state_with_direct_message("dm");
        state.focus_pane(FocusPane::Channels);

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
        assert_eq!(
            command,
            Some(AppCommand::SubscribeDirectMessage {
                channel_id: Id::new(20),
            })
        );
    }

    #[test]
    fn enter_on_group_direct_message_subscribes_channel() {
        let mut state = state_with_direct_message("group-dm");
        state.focus_pane(FocusPane::Channels);

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::SubscribeDirectMessage {
                channel_id: Id::new(20),
            })
        );
    }

    #[test]
    fn message_keys_use_scroll_controls() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(9);

        handle_key(&mut state, ctrl_key('u'));
        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());

        handle_key(&mut state, char_key('F'));
        assert_eq!(state.selected_message(), 9);
        assert!(state.message_auto_follow());

        handle_key(&mut state, key(KeyCode::PageUp));
        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());

        handle_key(&mut state, ctrl_key('d'));
        assert_eq!(state.selected_message(), 9);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_top_scroll_requests_older_history_once() {
        let mut state = state_with_messages(3);
        state.focus_pane(FocusPane::Messages);

        handle_key(&mut state, char_key('g'));
        let command = handle_key(&mut state, key(KeyCode::Up));

        assert_eq!(
            command,
            Some(AppCommand::LoadMessageHistory {
                channel_id: Id::new(2),
                before: Some(Id::new(1)),
            })
        );

        let duplicate = handle_key(&mut state, key(KeyCode::Up));

        assert_eq!(duplicate, None);
    }

    #[test]
    fn message_viewport_scroll_keys_do_not_change_selection_or_request_history() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(2, 16, 3);
        let selected = state.selected_message();

        handle_key(&mut state, char_key('J'));
        state.clamp_message_viewport_for_image_previews(2, 16, 3);

        let command = handle_key(&mut state, char_key('K'));

        assert_eq!(command, None);
        assert_eq!(state.selected_message(), selected);
        assert_eq!(state.message_line_scroll(), 0);
    }

    #[test]
    fn message_home_end_scroll_viewport_without_changing_selection() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        let selected = state.selected_message();

        handle_key(&mut state, key(KeyCode::Home));
        assert_eq!(state.selected_message(), selected);
        assert_eq!(state.message_scroll(), 0);

        handle_key(&mut state, key(KeyCode::End));
        assert_eq!(state.selected_message(), selected);
        assert!(state.message_scroll() > 0);
    }

    #[test]
    fn page_keys_scroll_non_message_panes() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        state.set_channel_view_height(9);

        handle_key(&mut state, key(KeyCode::PageDown));
        assert_eq!(state.selected_channel(), 2);

        handle_key(&mut state, key(KeyCode::PageUp));
        assert_eq!(state.selected_channel(), 0);
    }

    #[test]
    fn composer_requires_selected_channel() {
        let mut state = DashboardState::new();

        handle_key(&mut state, char_key('i'));
        assert!(!state.is_composing());

        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));

        handle_key(&mut state, char_key('i'));
        assert!(state.is_composing());
        assert_eq!(state.focus(), FocusPane::Messages);
    }

    #[test]
    fn number_keys_focus_top_level_panes() {
        let mut state = DashboardState::new();

        handle_key(&mut state, char_key('2'));
        assert_eq!(state.focus(), FocusPane::Channels);

        handle_key(&mut state, char_key('3'));
        assert_eq!(state.focus(), FocusPane::Messages);

        handle_key(&mut state, char_key('4'));
        assert_eq!(state.focus(), FocusPane::Members);

        handle_key(&mut state, char_key('1'));
        assert_eq!(state.focus(), FocusPane::Guilds);
    }

    #[test]
    fn number_keys_type_digits_while_composing() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, char_key('i'));

        handle_key(&mut state, char_key('4'));

        assert_eq!(state.focus(), FocusPane::Messages);
        assert_eq!(state.composer_input(), "4");
    }

    #[test]
    fn backtick_toggles_debug_log_popup() {
        let mut state = DashboardState::new();

        handle_key(&mut state, char_key('`'));
        assert!(state.is_debug_log_popup_open());

        handle_key(&mut state, char_key('`'));
        assert!(!state.is_debug_log_popup_open());
    }

    #[test]
    fn esc_closes_debug_log_popup_modally() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.toggle_debug_log_popup();

        handle_key(&mut state, key(KeyCode::Esc));

        assert!(!state.is_debug_log_popup_open());
        assert_eq!(state.focus(), FocusPane::Messages);
    }

    #[test]
    fn backtick_types_while_composing() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, char_key('i'));

        handle_key(&mut state, char_key('`'));

        assert!(state.is_composing());
        assert!(!state.is_debug_log_popup_open());
        assert_eq!(state.composer_input(), "`");
    }

    #[test]
    fn shift_enter_inserts_newline_while_composing() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, char_key('i'));
        handle_key(&mut state, char_key('h'));
        handle_key(&mut state, shift_enter());
        handle_key(&mut state, char_key('i'));

        assert!(state.is_composing());
        assert_eq!(state.composer_input(), "h\ni");
    }

    #[test]
    fn enter_submits_multiline_composer() {
        let mut state = state_with_channel_tree();
        state.focus_pane(FocusPane::Channels);
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, char_key('i'));
        handle_key(&mut state, char_key('h'));
        handle_key(&mut state, shift_enter());
        handle_key(&mut state, char_key('i'));

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert!(!state.is_composing());
        assert_eq!(state.composer_input(), "");
        assert_eq!(
            command,
            Some(crate::discord::AppCommand::SendMessage {
                channel_id: Id::new(11),
                content: "h\ni".to_owned(),
                reply_to: None,
            })
        );
    }

    #[test]
    fn o_key_is_reserved_for_future_attachment_actions() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);

        let command = handle_key(&mut state, char_key('o'));

        assert_eq!(command, None);
    }

    #[test]
    fn enter_and_space_open_message_action_menu() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);

        handle_key(&mut state, key(KeyCode::Enter));

        assert!(state.is_message_action_menu_open());
        state.close_message_action_menu();

        handle_key(&mut state, char_key(' '));

        assert!(state.is_message_action_menu_open());
    }

    #[test]
    fn message_action_menu_navigation_is_modal() {
        let mut state = state_with_messages(2);
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));

        handle_key(&mut state, key(KeyCode::Down));

        assert_eq!(state.selected_message(), 1);
        assert_eq!(
            state.selected_message_action().map(|action| action.kind),
            Some(MessageActionKind::AddReaction)
        );

        handle_key(&mut state, key(KeyCode::Esc));

        assert!(!state.is_message_action_menu_open());
    }

    #[test]
    fn esc_returns_from_message_opened_thread() {
        let mut state = state_with_thread_created_message();
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

        handle_key(&mut state, key(KeyCode::Esc));

        assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
        assert_eq!(state.focus(), FocusPane::Messages);
    }

    #[test]
    fn esc_closes_modal_before_returning_from_opened_thread() {
        let mut state = state_with_thread_created_message();
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

        handle_key(&mut state, char_key('`'));
        handle_key(&mut state, key(KeyCode::Esc));

        assert!(!state.is_debug_log_popup_open());
        assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

        handle_key(&mut state, key(KeyCode::Esc));
        assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    }

    #[test]
    fn message_action_menu_reply_opens_composer() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(command, None);
        assert!(!state.is_message_action_menu_open());
        assert!(state.is_composing());
        assert_eq!(state.composer_input(), "");

        handle_key(&mut state, char_key('h'));
        handle_key(&mut state, char_key('i'));
        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::SendMessage {
                channel_id: Id::new(2),
                content: "hi".to_owned(),
                reply_to: Some(Id::new(1)),
            })
        );
    }

    #[test]
    fn canceling_reply_composer_clears_reply_target() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Esc));

        handle_key(&mut state, char_key('i'));
        handle_key(&mut state, char_key('n'));
        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::SendMessage {
                channel_id: Id::new(2),
                content: "n".to_owned(),
                reply_to: None,
            })
        );
    }

    #[test]
    fn message_action_menu_download_image_returns_command() {
        let mut state = state_with_image_message();
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Down));

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::DownloadAttachment {
                url: "https://cdn.discordapp.com/cat.png".to_owned(),
                filename: "cat.png".to_owned(),
            })
        );
        assert!(!state.is_message_action_menu_open());
    }

    #[test]
    fn message_action_menu_add_reaction_opens_emoji_picker() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        handle_key(&mut state, key(KeyCode::Down));

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(command, None);
        assert!(!state.is_message_action_menu_open());
        assert!(state.is_emoji_reaction_picker_open());
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("👍".to_owned()))
        );
    }

    #[test]
    fn emoji_picker_selection_returns_reaction_command() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        open_emoji_picker(&mut state);

        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, key(KeyCode::Down));
        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::AddReaction {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                emoji: ReactionEmoji::Unicode("🎉".to_owned()),
            })
        );
        assert!(!state.is_emoji_reaction_picker_open());
    }

    #[test]
    fn emoji_picker_space_selects_reaction() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        open_emoji_picker(&mut state);

        handle_key(&mut state, key(KeyCode::Down));
        let command = handle_key(&mut state, char_key(' '));

        assert_eq!(
            command,
            Some(AppCommand::AddReaction {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                emoji: ReactionEmoji::Unicode("❤️".to_owned()),
            })
        );
        assert!(!state.is_emoji_reaction_picker_open());
    }

    #[test]
    fn emoji_picker_selection_returns_custom_reaction_command() {
        let mut state = state_with_custom_emoji_message();
        state.focus_pane(FocusPane::Messages);
        open_emoji_picker(&mut state);

        for _ in 0..8 {
            handle_key(&mut state, key(KeyCode::Down));
        }
        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::AddReaction {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                emoji: ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
            })
        );
    }

    #[test]
    fn emoji_picker_vim_and_arrow_keys_move_selection() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        open_emoji_picker(&mut state);

        handle_key(&mut state, char_key('j'));
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("❤️".to_owned()))
        );

        handle_key(&mut state, char_key('j'));
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("😂".to_owned()))
        );

        handle_key(&mut state, char_key('k'));
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("❤️".to_owned()))
        );

        handle_key(&mut state, key(KeyCode::Up));
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("👍".to_owned()))
        );
    }

    #[test]
    fn escape_closes_emoji_picker_without_reacting() {
        let mut state = state_with_messages(2);
        state.focus_pane(FocusPane::Messages);
        open_emoji_picker(&mut state);

        handle_key(&mut state, key(KeyCode::Down));
        let command = handle_key(&mut state, key(KeyCode::Esc));

        assert_eq!(command, None);
        assert!(!state.is_emoji_reaction_picker_open());
        assert_eq!(state.selected_message(), 1);
    }

    #[test]
    fn reaction_users_popup_is_modal_and_escape_closes_it() {
        let mut state = state_with_messages(2);
        state.focus_pane(FocusPane::Messages);
        state.push_event(AppEvent::ReactionUsersLoaded {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                users: vec![ReactionUserInfo {
                    user_id: Id::new(10),
                    display_name: "neo".to_owned(),
                }],
            }],
        });

        handle_key(&mut state, key(KeyCode::Down));

        assert_eq!(state.selected_message(), 1);
        assert!(state.is_reaction_users_popup_open());
        assert_eq!(
            state.reaction_users_popup().map(|popup| popup.scroll()),
            Some(1)
        );

        let command = handle_key(&mut state, key(KeyCode::Esc));

        assert_eq!(command, None);
        assert!(!state.is_reaction_users_popup_open());
    }

    #[test]
    fn multiselect_poll_picker_toggles_and_submits_selected_answers() {
        let mut state = state_with_multiselect_poll();
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        for _ in 0..5 {
            handle_key(&mut state, key(KeyCode::Down));
        }

        let command = handle_key(&mut state, key(KeyCode::Enter));
        assert_eq!(command, None);
        assert!(state.is_poll_vote_picker_open());

        handle_key(&mut state, key(KeyCode::Down));
        handle_key(&mut state, char_key(' '));
        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(
            command,
            Some(AppCommand::VotePoll {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                answer_ids: vec![1, 2],
            })
        );
        assert!(!state.is_poll_vote_picker_open());
    }

    fn state_with_folder() -> DashboardState {
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        let mut state = DashboardState::new();

        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                member_count: None,
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
                owner_id: None,
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![GuildFolder {
                id: Some(42),
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }],
        });
        state
    }
    fn assert_selected_folder_collapsed(state: &DashboardState, expected: bool) {
        let entries = state.guild_pane_entries();
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader { collapsed, .. } if collapsed == expected
        ));
    }

    fn assert_selected_channel_category_collapsed(state: &DashboardState, expected: bool) {
        let entries = state.channel_pane_entries();
        assert!(matches!(
            entries[0],
            ChannelPaneEntry::CategoryHeader { collapsed, .. } if collapsed == expected
        ));
    }

    fn state_with_channel_tree() -> DashboardState {
        let guild_id = Id::new(1);
        let category_id = Id::new(10);
        let general_id = Id::new(11);
        let random_id = Id::new(12);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: category_id,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "Text Channels".to_owned(),
                    kind: "category".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: general_id,
                    parent_id: Some(category_id),
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: random_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: None,
                    name: "random".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_direct_message(kind: &str) -> DashboardState {
        let channel_id = Id::new(20);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: kind.to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(30),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            }]),
            permission_overwrites: Vec::new(),
        }));
        state.confirm_selected_guild();
        state
    }

    fn state_with_messages(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state
    }

    fn state_with_thread_created_message() -> DashboardState {
        let guild_id = Id::new(1);
        let parent_id = Id::new(2);
        let thread_id = Id::new(10);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: parent_id,
                    parent_id: None,
                    position: None,
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "GuildText".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: thread_id,
                    parent_id: Some(parent_id),
                    position: None,
                    last_message_id: None,
                    name: "release notes".to_owned(),
                    kind: "thread".to_owned(),
                    message_count: Some(12),
                    total_message_sent: Some(14),
                    thread_archived: Some(false),
                    thread_locked: Some(false),
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id: parent_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::new(18),
            reference: Some(MessageReferenceInfo {
                guild_id: Some(guild_id),
                channel_id: Some(thread_id),
                message_id: None,
            }),
            reply: None,
            poll: None,
            content: Some("release notes".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn state_with_multiselect_poll() -> DashboardState {
        let mut state = state_with_messages(1);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(PollInfo {
                question: "Pick foods".to_owned(),
                answers: vec![
                    PollAnswerInfo {
                        answer_id: 1,
                        text: "Soup".to_owned(),
                        vote_count: Some(2),
                        me_voted: true,
                    },
                    PollAnswerInfo {
                        answer_id: 2,
                        text: "Noodles".to_owned(),
                        vote_count: Some(1),
                        me_voted: false,
                    },
                ],
                allow_multiselect: true,
                results_finalized: Some(false),
                total_votes: Some(3),
            }),
            content: Some("msg 1".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn state_with_custom_emoji_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("msg 1".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn state_with_image_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: vec![crate::discord::AttachmentInfo {
                id: Id::new(3),
                filename: "cat.png".to_owned(),
                url: "https://cdn.discordapp.com/cat.png".to_owned(),
                proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
            }],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }
    fn open_emoji_picker(state: &mut DashboardState) {
        handle_key(state, key(KeyCode::Enter));
        handle_key(state, key(KeyCode::Down));
        handle_key(state, key(KeyCode::Enter));
        assert!(state.is_emoji_reaction_picker_open());
    }
}
