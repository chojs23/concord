use super::*;
use crate::discord::{MediaPlaybackSource, MediaPlaybackTarget, MessageHistoryAfterMode};

#[test]
fn enter_on_direct_message_kinds_subscribes_channel() {
    for kind in ["dm", "group-dm"] {
        let mut state = state_with_direct_message(kind);
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
}

#[test]
fn message_keys_use_scroll_controls() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, ctrl_key('d'));
    assert_eq!(state.selected_message(), 9);
    assert_eq!(state.message_scroll(), 2);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, ctrl_key('d'));
    assert!(state.message_auto_follow());
}

#[test]
fn message_top_scroll_emits_older_history_target() {
    let mut state = state_with_messages(3);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('g'));
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

    assert_eq!(duplicate, command);
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
fn message_viewport_scroll_uses_configured_keys() {
    let mut state = state_with_messages_from_state(
        state_with_keymap(KeymapOptions {
            mappings: [
                ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
                ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        }),
        0,
    );
    push_guild_message(&mut state, 1, "abcdefghijkl");
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.scroll_message_viewport_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    handle_key(&mut state, char_key('J'));
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    assert_eq!(state.message_line_scroll(), 0);

    handle_key(&mut state, char_key('N'));
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    assert_eq!(state.message_line_scroll(), 1);

    handle_key(&mut state, char_key('P'));
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn backtick_toggles_debug_log_popup() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('`'));
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::DebugLog));

    handle_key(&mut state, char_key('`'));
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::DebugLog));
}

#[test]
fn esc_closes_debug_log_popup_modally() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.toggle_debug_log_popup();

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::DebugLog));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn enter_opens_selected_forum_post_from_message_pane() {
    let mut state = state_with_forum_channel_posts();
    state.focus_pane(FocusPane::Messages);
    state.move_down();

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(30),
        })
    );
}

#[test]
fn message_action_menu_navigation_is_modal() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.selected_message_action_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_message_action_context_active());
}

#[test]
fn message_action_menu_selection_aliases_move_disabled_selection() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_message_action_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.selected_message_action_index(), Some(0));

    handle_key(&mut state, char_key('j'));
    assert_eq!(state.selected_message_action_index(), Some(1));

    handle_key(&mut state, char_key('k'));
    assert_eq!(state.selected_message_action_index(), Some(0));

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_message_action_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_message_action_index(), Some(0));
}

#[test]
fn esc_returns_from_message_opened_thread() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('t'));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn esc_returns_from_pinned_message_view() {
    let mut state = state_with_messages(3);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Up));
    let expected_selected = state.selected_message();

    state.push_event(AppEvent::MessagePinnedUpdate {
        channel_id: Id::new(2),
        message_id: Id::new(2),
        pinned: true,
    });
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_pinned_message_view());
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.selected_message(), expected_selected);
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn message_action_shortcuts_edit_and_delete_own_message() {
    let mut edit_state = state_with_own_message();
    edit_state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut edit_state, char_key('e'));

    assert_eq!(command, None);
    assert!(!edit_state.is_message_action_context_active());
    assert!(edit_state.is_composing());

    let mut delete_state = state_with_own_message();
    delete_state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut delete_state, char_key('d'));

    assert_eq!(command, None);
    assert!(!delete_state.is_message_action_context_active());
    assert!(
        delete_state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    let command = handle_key(&mut delete_state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(
        !delete_state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn message_pane_shortcuts_reuse_message_actions() {
    let mut reaction_state = state_with_messages(1);
    reaction_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reaction_state, char_key('r'));
    assert!(
        reaction_state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );

    let mut reply_state = state_with_messages(1);
    reply_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reply_state, char_key('R'));
    assert!(reply_state.is_composing());
    handle_key(&mut reply_state, char_key('o'));
    let command = handle_key(&mut reply_state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "o".to_owned(),
            reply_to: Some(Id::new(1)),
            attachments: Vec::new(),
        })
    );

    let mut edit_state = state_with_own_message();
    edit_state.focus_pane(FocusPane::Messages);
    handle_key(&mut edit_state, char_key('e'));
    assert!(edit_state.is_composing());
    assert_eq!(edit_state.composer_input(), "msg 1");
}

#[test]
fn message_pane_default_shortcuts_work_from_message_pane() {
    let mut reaction_state = state_with_messages(1);
    reaction_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reaction_state, char_key('r'));
    assert!(
        reaction_state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );

    let mut reply_state = state_with_messages(1);
    reply_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reply_state, char_key('R'));
    assert!(reply_state.is_composing());

    let mut pin_state = state_with_messages(1);
    pin_state.focus_pane(FocusPane::Messages);
    handle_key(&mut pin_state, key(KeyCode::Enter));
    let command = handle_key(&mut pin_state, char_key('P'));
    assert_eq!(command, None);
    assert!(
        pin_state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn message_action_menu_d_shortcut_removes_embeds() {
    let mut state = state_with_own_message();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            author_id: Id::new(99),
            embeds: vec![EmbedInfo::test()],
            ..MessageInfo::test(Id::new(2), Id::new(1))
        }],
    });
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, char_key('D'));

    assert_eq!(command, None);
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::RemoveMessageEmbeds {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(!state.is_message_action_context_active());
}

#[test]
fn open_url_shortcut_opens_url_or_url_picker() {
    let mut state = state_with_messages(0);
    state.push_event(message_create_event(MessageCreateFixture {
        message_id: Id::new(1),
        content: Some("first https://one.example second https://two.example".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('o'));

    assert_eq!(command, None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageUrlPicker));
    assert!(!state.is_message_action_context_active());

    let command = handle_key(&mut state, char_key('2'));

    assert_eq!(
        command,
        Some(AppCommand::OpenUrl {
            url: "https://two.example".to_owned(),
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageUrlPicker)
    );
    assert!(!state.is_message_action_context_active());
}

#[test]
fn play_media_shortcut_returns_media_command() {
    let mut state = state_with_messages(0);
    state.push_event(message_create_event(MessageCreateFixture {
        message_id: Id::new(1),
        content: Some("watch https://youtu.be/dQw4w9WgXcQ".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('x'));

    assert_eq!(
        command,
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: "https://youtu.be/dQw4w9WgXcQ".to_owned(),
                label: "media URL".to_owned(),
                source: MediaPlaybackSource::Message,
            },
            request_id: None,
        })
    );
}

#[test]
fn message_pane_copy_shortcut_requests_selected_message_content() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('y'));

    assert_eq!(
        state.take_copy_text_request(),
        Some(("msg 1".to_owned(), "Message copied"))
    );
}

#[test]
fn message_action_popup_q_runs_configured_action_before_close_popup() {
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: std::collections::BTreeMap::new(),
        message_actions: [("CopyMessage".to_owned(), KeymapBinding::one("q"))]
            .into_iter()
            .collect(),
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_message_action_context_active());

    handle_key(&mut state, char_key('q'));

    assert_eq!(
        state.take_copy_text_request(),
        Some(("msg 1".to_owned(), "Message copied"))
    );
    assert!(!state.is_message_action_context_active());
}

#[test]
fn message_action_popup_configured_navigation_key_closes_popup() {
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: std::collections::BTreeMap::new(),
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("j"))]
            .into_iter()
            .collect(),
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_message_action_context_active());

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_message_action_context_active());

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_message_action_context_active());

    handle_key(&mut state, char_key('j'));

    assert!(!state.is_message_action_context_active());
}

#[test]
fn message_pane_delete_shortcut_requires_confirmation() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(command, None);
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    handle_key(&mut state, char_key('d'));
    let command = handle_key(&mut state, char_key('y'));

    assert_eq!(
        command,
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn message_pane_view_attachment_shortcut_opens_viewer() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('v'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(1)
    );
}

#[test]
fn message_pane_profile_shortcut_opens_author_profile() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(
        command,
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(99),
            guild_id: Some(Id::new(1)),
        })
    );
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));
}

#[test]
fn goto_referenced_message_shortcut_merges_target_window_into_normal_messages() {
    let mut state = state_with_messages(0);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(10),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Some(Id::new(2)),
            message_id: Some(Id::new(5)),
        }),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    let command = handle_key(&mut state, char_key('g'));

    assert_eq!(
        command,
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id: Id::new(2),
            message_id: Id::new(5),
        })
    );

    state.push_event(AppEvent::MessageHistoryAroundLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(5),
        messages: vec![
            MessageInfo::test(Id::new(2), Id::new(4)),
            MessageInfo::test(Id::new(2), Id::new(5)),
            MessageInfo::test(Id::new(2), Id::new(6)),
        ],
    });

    assert_eq!(state.messages()[state.selected_message()].id, Id::new(5));
    assert_eq!(
        state
            .messages()
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![4, 5, 6, 10]
    );

    assert_eq!(handle_key(&mut state, char_key('j')), None);
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(6));

    assert_eq!(
        handle_key(&mut state, char_key('j')),
        Some(AppCommand::LoadMessageHistoryAfter {
            channel_id: Id::new(2),
            after: Id::new(6),
            mode: MessageHistoryAfterMode::GapFill,
        })
    );
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(6));

    assert_eq!(handle_key(&mut state, char_key('G')), None);
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(10));
    state.move_up();
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(6));

    state.push_event(AppEvent::MessageHistoryAfterLoaded {
        channel_id: Id::new(2),
        after: Id::new(6),
        messages: vec![
            MessageInfo::test(Id::new(2), Id::new(7)),
            MessageInfo::test(Id::new(2), Id::new(8)),
            MessageInfo::test(Id::new(2), Id::new(9)),
        ],
        has_more: false,
        mode: MessageHistoryAfterMode::GapFill,
    });

    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(11),
        ..guild_message_create_fixture()
    }));
    assert_eq!(
        state
            .messages()
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![4, 5, 6, 7, 8, 9, 10, 11]
    );

    assert_eq!(handle_key(&mut state, char_key('G')), None);
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(11));
}

#[test]
fn pinned_and_forum_down_keys_do_not_request_newer_history() {
    let mut pinned_state = state_with_messages(0);
    pinned_state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo::test(Id::new(2), Id::new(10))],
    });
    pinned_state.push_event(AppEvent::MessageHistoryAroundLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(5),
        messages: vec![
            MessageInfo::test(Id::new(2), Id::new(4)),
            MessageInfo::test(Id::new(2), Id::new(5)),
            MessageInfo::test(Id::new(2), Id::new(6)),
        ],
    });
    pinned_state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![MessageInfo::test(Id::new(2), Id::new(6))],
    });
    pinned_state.enter_pinned_message_view(Id::new(2));
    pinned_state.focus_pane(FocusPane::Messages);

    assert_eq!(handle_key(&mut pinned_state, char_key('j')), None);
    assert_eq!(handle_key(&mut pinned_state, ctrl_key('d')), None);

    let mut forum_state = state_with_forum_channel_posts();
    forum_state.focus_pane(FocusPane::Messages);

    assert_eq!(handle_key(&mut forum_state, char_key('j')), None);
    assert_eq!(handle_key(&mut forum_state, ctrl_key('d')), None);
}

#[test]
fn goto_referenced_message_shortcut_noops_for_unknown_forward_channel() {
    let mut state = state_with_messages(0);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(10),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(Id::new(9)),
            channel_id: Some(Id::new(999)),
            message_id: Some(Id::new(50)),
        }),
        forwarded_snapshots: vec![MessageSnapshotInfo::test()],
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    let command = handle_key(&mut state, char_key('g'));

    assert_eq!(command, None);
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
}

#[test]
fn message_pane_pin_shortcut_requires_confirmation() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    let command = handle_key(&mut state, char_key('P'));

    assert_eq!(command, None);
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('P'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SetMessagePinned {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            pinned: true,
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn message_action_menu_control_page_keys_move_selection() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, ctrl_key('d'));

    assert_eq!(command, None);
    assert!(state.is_message_action_context_active());
    assert_eq!(state.selected_message_action_index(), Some(10));

    handle_key(&mut state, ctrl_key('u'));

    assert_eq!(state.selected_message_action_index(), Some(0));
}

#[test]
fn direct_view_attachment_shortcut_opens_viewer_and_esc_closes_viewer() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('v'));

    assert_eq!(command, None);
    assert!(!state.is_message_action_context_active());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, char_key('l'));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('k'));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, key(KeyCode::Left));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Right));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('h'));
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer)
    );
}

#[test]
fn attachment_viewer_d_shortcut_downloads_attachment() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('v'));

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            id: AttachmentDownloadId::new(0),
            url: "https://cdn.discordapp.com/cat.png".to_owned(),
            filename: "cat.png".to_owned(),
            source: DownloadAttachmentSource::AttachmentViewer,
        })
    );
    assert!(state.attachment_downloads().is_empty());
}

#[test]
fn attachment_viewer_x_shortcut_plays_video_attachment() {
    let mut state = state_with_messages(0);
    state.push_event(message_create_event(MessageCreateFixture {
        message_id: Id::new(1),
        content: Some(String::new()),
        attachments: vec![crate::discord::AttachmentInfo {
            id: Id::new(3),
            filename: "clip.mp4".to_owned(),
            url: "https://cdn.discordapp.com/clip.mp4".to_owned(),
            proxy_url: "https://media.discordapp.net/clip.mp4".to_owned(),
            content_type: Some("video/mp4".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }],
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('v'));

    let command = handle_key(&mut state, char_key('x'));

    assert_eq!(
        command,
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: "https://cdn.discordapp.com/clip.mp4".to_owned(),
                label: "clip.mp4".to_owned(),
                source: MediaPlaybackSource::AttachmentViewer,
            },
            request_id: None,
        })
    );
}

#[test]
fn reaction_users_popup_is_modal_and_escape_closes_it() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            users: vec![ReactionUserInfo::test(Id::new(10), "neo")],
            ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
        }],
    });

    handle_key(&mut state, key(KeyCode::Down));

    assert_eq!(state.selected_message(), 1);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ReactionUsers));
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(1)
    );

    let command = handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(command, None);
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ReactionUsers));
}

#[test]
fn poll_picker_number_shortcut_toggles_answer() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('c'));

    handle_key(&mut state, char_key('2'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1, 2],
        })
    );
}

#[test]
fn poll_picker_selection_aliases_move_selection() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('c'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(0));

    handle_key(&mut state, char_key('j'));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(1));

    handle_key(&mut state, char_key('k'));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(0));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.selected_poll_vote_picker_index(), Some(0));
}
