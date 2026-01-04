use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::discord::AppCommand;

use super::state::{DashboardState, FocusPane};

pub fn handle_key(state: &mut DashboardState, key: KeyEvent) -> Option<AppCommand> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if state.is_composing() {
        return handle_composer_key(state, key);
    }

    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Char('i') => state.start_composer(),
        KeyCode::Char('1') => state.focus_pane(FocusPane::Guilds),
        KeyCode::Char('2') => state.focus_pane(FocusPane::Channels),
        KeyCode::Char('3') => state.focus_pane(FocusPane::Messages),
        KeyCode::Char('4') => state.focus_pane(FocusPane::Members),
        KeyCode::Char('j') | KeyCode::Down => state.move_down(),
        KeyCode::Char('k') | KeyCode::Up => state.move_up(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.half_page_down()
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => state.half_page_up(),
        KeyCode::PageDown => state.half_page_down(),
        KeyCode::PageUp => state.half_page_up(),
        KeyCode::Char('F') => state.toggle_message_auto_follow(),
        KeyCode::Char('g') => state.jump_top(),
        KeyCode::Home => state.jump_top(),
        KeyCode::Char('G') => state.jump_bottom(),
        KeyCode::End => state.jump_bottom(),
        KeyCode::Tab => state.cycle_focus(),
        // Tree headers act like a small tree: Enter/Space toggles, Right
        // opens, and Left closes. Anywhere else these keys are no-ops.
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Guilds => {
            state.confirm_selected_guild()
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.focus() == FocusPane::Channels => {
            state.confirm_selected_channel()
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
        discord::{AppEvent, ChannelInfo, GuildFolder},
        tui::state::{ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry},
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

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(state.selected_channel_id(), Some(Id::new(11)));

        handle_key(&mut state, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
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
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: general_id,
                    parent_id: Some(category_id),
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: random_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: None,
                    name: "random".to_owned(),
                    kind: "text".to_owned(),
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
        });
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
            }],
            members: Vec::new(),
            presences: Vec::new(),
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
                message_kind: crate::discord::MessageKind::regular(),
                content: Some(format!("msg {id}")),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state
    }

    fn focus_messages(state: &mut DashboardState) {
        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
    }
}
