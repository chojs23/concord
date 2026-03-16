use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_reaction_users_popup_open() {
        return handle_reaction_users_popup_key(state, key);
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
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

    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('a') if state.focus() == FocusPane::Channels => {
            state.open_selected_channel_actions();
        }
        KeyCode::Char('a') if state.focus() == FocusPane::Members => {
            state.open_selected_member_actions();
        }
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('1') => state.focus_pane(FocusPane::Guilds),
        KeyCode::Char('2') => state.focus_pane(FocusPane::Channels),
        KeyCode::Char('3') => state.focus_pane(FocusPane::Messages),
        KeyCode::Char('4') => state.focus_pane(FocusPane::Members),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('J') if state.focus() == FocusPane::Messages => {
            state.scroll_message_viewport_down()
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
            return state.next_older_history_command();
        }
        KeyCode::Char('K') if state.focus() == FocusPane::Messages => {
            state.scroll_message_viewport_up()
        }
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
            if state.focus() == FocusPane::Messages {
                state.scroll_message_viewport_top();
            } else {
                state.jump_top();
            }
        }
        KeyCode::Char('G') => state.jump_bottom(),
        KeyCode::End => {
            if state.focus() == FocusPane::Messages {
                state.scroll_message_viewport_bottom();
            } else {
                state.jump_bottom();
            }
        }
        KeyCode::Tab => state.cycle_focus(),
        // Tree headers act like a small tree: Enter/Space toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Guilds => {
            state.confirm_selected_guild()
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Channels => {
            return state.confirm_selected_channel_command();
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Members => {
            return state.show_selected_member_profile();
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Messages => {
            state.open_selected_message_actions()
        }
        KeyCode::Right if state.focus() == FocusPane::Guilds => state.open_selected_folder(),
        KeyCode::Left if state.focus() == FocusPane::Guilds => state.close_selected_folder(),
        KeyCode::Right if state.focus() == FocusPane::Channels => {
            state.open_selected_channel_category()
        }
        KeyCode::Left if state.focus() == FocusPane::Channels => {
            state.close_selected_channel_category()
        }
        _ => {}
    }

    None
}

fn handle_message_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_message_action_menu(),
        KeyCode::Char('j') | KeyCode::Down => state.move_message_action_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_message_action_up(),
        KeyCode::Enter | KeyCode::Char(' ') => return state.activate_selected_message_action(),
        _ => {}
    }

    None
}

fn handle_user_profile_popup_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
        state.close_user_profile_popup();
    }
    None
}

fn handle_member_action_menu_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_member_action_menu(),
        KeyCode::Char('j') | KeyCode::Down => state.move_member_action_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_member_action_up(),
        KeyCode::Enter | KeyCode::Char(' ') => return state.activate_selected_member_action(),
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
        KeyCode::Char('j') | KeyCode::Down => state.move_channel_action_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_channel_action_up(),
        KeyCode::Enter | KeyCode::Char(' ') => return state.activate_selected_channel_action(),
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
        KeyCode::Char('j') | KeyCode::Down => state.move_emoji_reaction_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_emoji_reaction_up(),
        KeyCode::Enter | KeyCode::Char(' ') => return state.activate_selected_emoji_reaction(),
        _ => {}
    }

    None
}

fn handle_poll_vote_picker_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    match key.code {
        KeyCode::Esc => state.close_poll_vote_picker(),
        KeyCode::Char('j') | KeyCode::Down => state.move_poll_vote_picker_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_poll_vote_picker_up(),
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
        KeyCode::Char('j') | KeyCode::Down => state.scroll_reaction_users_popup_down(),
        KeyCode::Char('k') | KeyCode::Up => state.scroll_reaction_users_popup_up(),
        KeyCode::PageDown => state.page_reaction_users_popup_down(),
        KeyCode::PageUp => state.page_reaction_users_popup_up(),
        _ => {}
    }

    None
}

fn handle_composer_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use twilight_model::id::Id;

    use super::handle_key;
    use crate::{
        discord::{
            AppCommand, AppEvent, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo, GuildFolder,
            PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionUserInfo,
            ReactionUsersInfo,
        },
        tui::state::{
            ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MessageActionKind,
        },
    };

    #[test]
    fn enter_and_space_toggle_selected_folder() {
        let mut state = state_with_folder();
        focus_guilds(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_selected_folder_collapsed(&state, true);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert_selected_folder_collapsed(&state, false);
    }

    #[test]
    fn enter_and_space_toggle_selected_channel_category() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_selected_channel_category_collapsed(&state, true);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert_selected_channel_category_collapsed(&state, false);
    }

    #[test]
    fn movement_waits_for_enter_to_activate_channel() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);

        assert_eq!(state.selected_channel_id(), None);

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(state.selected_channel_id(), None);

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(
            command,
            Some(AppCommand::SubscribeGuildChannel {
                guild_id: Id::new(1),
                channel_id: Id::new(11),
            })
        );
        assert_eq!(state.selected_channel_id(), Some(Id::new(11)));

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
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
        focus_channels(&mut state);

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_channels(&mut state);

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        state.set_message_view_height(9);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('F'), KeyModifiers::NONE),
        );
        assert_eq!(state.selected_message(), 9);
        assert!(state.message_auto_follow());

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        );
        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.selected_message(), 9);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_top_scroll_requests_older_history_once() {
        let mut state = state_with_messages(3);
        focus_messages(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        let command = handle_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(
            command,
            Some(AppCommand::LoadMessageHistory {
                channel_id: Id::new(2),
                before: Some(Id::new(1)),
            })
        );

        let duplicate = handle_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(duplicate, None);
    }

    #[test]
    fn message_viewport_scroll_keys_do_not_change_selection_or_request_history() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);
        state.clamp_message_viewport_for_image_previews(2, 16, 3);
        let selected = state.selected_message();

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
        );
        state.clamp_message_viewport_for_image_previews(2, 16, 3);

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('K'), KeyModifiers::NONE),
        );

        assert_eq!(command, None);
        assert_eq!(state.selected_message(), selected);
        assert_eq!(state.message_line_scroll(), 0);
    }

    #[test]
    fn message_home_end_scroll_viewport_without_changing_selection() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        let selected = state.selected_message();

        handle_key(&mut state, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(state.selected_message(), selected);
        assert_eq!(state.message_scroll(), 0);

        handle_key(&mut state, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(state.selected_message(), selected);
        assert!(state.message_scroll() > 0);
    }

    #[test]
    fn page_keys_scroll_non_message_panes() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);
        state.set_channel_view_height(9);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE),
        );
        assert_eq!(state.selected_channel(), 2);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
        );
        assert_eq!(state.selected_channel(), 0);
    }

    #[test]
    fn composer_requires_selected_channel() {
        let mut state = DashboardState::new();

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        assert!(!state.is_composing());

        let mut state = state_with_channel_tree();
        focus_channels(&mut state);
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        assert!(state.is_composing());
        assert_eq!(state.focus(), FocusPane::Messages);
    }

    #[test]
    fn number_keys_focus_top_level_panes() {
        let mut state = DashboardState::new();

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
        );
        assert_eq!(state.focus(), FocusPane::Channels);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
        );
        assert_eq!(state.focus(), FocusPane::Messages);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
        );
        assert_eq!(state.focus(), FocusPane::Members);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        );
        assert_eq!(state.focus(), FocusPane::Guilds);
    }

    #[test]
    fn number_keys_type_digits_while_composing() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
        );

        assert_eq!(state.focus(), FocusPane::Messages);
        assert_eq!(state.composer_input(), "4");
    }

    #[test]
    fn shift_enter_inserts_newline_while_composing() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );

        assert!(state.is_composing());
        assert_eq!(state.composer_input(), "h\ni");
    }

    #[test]
    fn enter_submits_multiline_composer() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
        );

        assert_eq!(command, None);
    }

    #[test]
    fn enter_and_space_open_message_action_menu() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert!(state.is_message_action_menu_open());
        state.close_message_action_menu();

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );

        assert!(state.is_message_action_menu_open());
    }

    #[test]
    fn message_action_menu_navigation_is_modal() {
        let mut state = state_with_messages(2);
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(state.selected_message(), 1);
        assert_eq!(
            state.selected_message_action().map(|action| action.kind),
            Some(MessageActionKind::AddReaction)
        );

        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!state.is_message_action_menu_open());
    }

    #[test]
    fn message_action_menu_reply_opens_composer() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert_eq!(command, None);
        assert!(!state.is_message_action_menu_open());
        assert!(state.is_composing());
        assert_eq!(state.composer_input(), "");

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        open_emoji_picker(&mut state);

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        open_emoji_picker(&mut state);

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        open_emoji_picker(&mut state);

        for _ in 0..8 {
            handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        focus_messages(&mut state);
        open_emoji_picker(&mut state);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("❤️".to_owned()))
        );

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("😂".to_owned()))
        );

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        );
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("❤️".to_owned()))
        );

        handle_key(&mut state, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            state.selected_emoji_reaction().map(|item| item.emoji),
            Some(ReactionEmoji::Unicode("👍".to_owned()))
        );
    }

    #[test]
    fn escape_closes_emoji_picker_without_reacting() {
        let mut state = state_with_messages(2);
        focus_messages(&mut state);
        open_emoji_picker(&mut state);

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let command = handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(command, None);
        assert!(!state.is_emoji_reaction_picker_open());
        assert_eq!(state.selected_message(), 1);
    }

    #[test]
    fn reaction_users_popup_is_modal_and_escape_closes_it() {
        let mut state = state_with_messages(2);
        focus_messages(&mut state);
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

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(state.selected_message(), 1);
        assert!(state.is_reaction_users_popup_open());
        assert_eq!(
            state.reaction_users_popup().map(|popup| popup.scroll()),
            Some(1)
        );

        let command = handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(command, None);
        assert!(!state.is_reaction_users_popup_open());
    }

    #[test]
    fn multiselect_poll_picker_toggles_and_submits_selected_answers() {
        let mut state = state_with_multiselect_poll();
        focus_messages(&mut state);
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        for _ in 0..5 {
            handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }

        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(command, None);
        assert!(state.is_poll_vote_picker_open());

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        let command = handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
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

    fn focus_guilds(state: &mut DashboardState) {
        while state.focus() != FocusPane::Guilds {
            state.cycle_focus();
        }
    }

    fn focus_channels(state: &mut DashboardState) {
        while state.focus() != FocusPane::Channels {
            state.cycle_focus();
        }
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
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
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
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            }]),
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
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
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
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("msg 1".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
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
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn focus_messages(state: &mut DashboardState) {
        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
    }

    fn open_emoji_picker(state: &mut DashboardState) {
        handle_key(state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        handle_key(state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(state, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(state.is_emoji_reaction_picker_open());
    }
}
