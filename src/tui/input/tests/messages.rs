use super::*;

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

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_message(), 5);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, ctrl_key('d'));
    assert_eq!(state.selected_message(), 9);
    // Half-page-down landed the cursor on the latest message, so
    // auto-follow re-engages.
    assert!(state.message_auto_follow());
}

#[test]
fn message_top_scroll_emits_older_history_target() {
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
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::AddReaction)
    );

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_message_action_menu_open());
}

#[test]
fn message_action_menu_selection_aliases_move_selection() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::AddReaction)
    );

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::Reply)
    );

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::AddReaction)
    );

    handle_key(&mut state, char_key('k'));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::Reply)
    );

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::AddReaction)
    );

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::Reply)
    );
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
    handle_key(&mut edit_state, key(KeyCode::Enter));

    let command = handle_key(&mut edit_state, char_key('e'));

    assert_eq!(command, None);
    assert!(!edit_state.is_message_action_menu_open());
    assert!(edit_state.is_composing());

    let mut delete_state = state_with_own_message();
    delete_state.focus_pane(FocusPane::Messages);
    handle_key(&mut delete_state, key(KeyCode::Enter));

    let command = handle_key(&mut delete_state, char_key('d'));

    assert_eq!(command, None);
    assert!(!delete_state.is_message_action_menu_open());
    assert!(delete_state.is_message_delete_confirmation_open());

    let command = handle_key(&mut delete_state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(!delete_state.is_message_delete_confirmation_open());
}

#[test]
fn message_pane_shortcuts_reuse_message_actions() {
    let mut reaction_state = state_with_messages(1);
    reaction_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reaction_state, char_key('r'));
    assert!(reaction_state.is_emoji_reaction_picker_open());

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
fn message_action_menu_shortcuts_match_message_pane_shortcuts() {
    let mut reaction_state = state_with_messages(1);
    reaction_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reaction_state, key(KeyCode::Enter));
    handle_key(&mut reaction_state, char_key('r'));
    assert!(reaction_state.is_emoji_reaction_picker_open());

    let mut reply_state = state_with_messages(1);
    reply_state.focus_pane(FocusPane::Messages);
    handle_key(&mut reply_state, key(KeyCode::Enter));
    handle_key(&mut reply_state, char_key('R'));
    assert!(reply_state.is_composing());

    let mut pin_state = state_with_messages(1);
    pin_state.focus_pane(FocusPane::Messages);
    handle_key(&mut pin_state, key(KeyCode::Enter));
    let command = handle_key(&mut pin_state, char_key('P'));
    assert_eq!(command, None);
    assert!(pin_state.is_message_pin_confirmation_open());
}

#[test]
fn message_action_o_shortcut_opens_url_or_url_picker() {
    let mut state = state_with_messages(0);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some("first https://one.example second https://two.example".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));
    let command = handle_key(&mut state, char_key('o'));

    assert_eq!(command, None);
    assert!(state.is_message_url_picker_open());

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(state.is_message_action_menu_open());
    assert!(!state.is_message_url_picker_open());

    handle_key(&mut state, char_key('o'));
    let command = handle_key(&mut state, char_key('2'));

    assert_eq!(
        command,
        Some(AppCommand::OpenUrl {
            url: "https://two.example".to_owned(),
        })
    );
    assert!(!state.is_message_action_menu_open());
}

#[test]
fn message_pane_copy_shortcut_requests_selected_message_content() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('y'));

    assert_eq!(
        state.take_copy_message_content_request(),
        Some("msg 1".to_owned())
    );
}

#[test]
fn message_pane_delete_shortcut_requires_confirmation() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(command, None);
    assert!(state.is_message_delete_confirmation_open());

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_message_delete_confirmation_open());

    handle_key(&mut state, char_key('d'));
    let command = handle_key(&mut state, char_key('y'));

    assert_eq!(
        command,
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(!state.is_message_delete_confirmation_open());
}

#[test]
fn message_pane_view_image_shortcut_opens_viewer() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('v'));

    assert!(state.is_image_viewer_open());
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );
}

#[test]
fn message_pane_profile_shortcut_opens_author_profile() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(
        command,
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(99),
            guild_id: Some(Id::new(1)),
        })
    );
    assert!(state.is_user_profile_popup_open());
}

#[test]
fn message_pane_pin_shortcut_requires_confirmation() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('P'));

    assert_eq!(command, None);
    assert!(state.is_message_pin_confirmation_open());

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_message_pin_confirmation_open());

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
    assert!(!state.is_message_pin_confirmation_open());
}

#[test]
fn message_action_shortcuts_ignore_control_modified_keys() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, ctrl_key('d'));

    assert_eq!(command, None);
    assert!(state.is_message_action_menu_open());
    assert_eq!(state.selected_message_action_index(), Some(0));
}

#[test]
fn message_action_menu_view_image_opens_viewer_and_esc_closes_viewer() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_image_viewer_open());
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, char_key('l'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('k'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, key(KeyCode::Left));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Right));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('h'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_image_viewer_open());
}

#[test]
fn image_viewer_d_shortcut_downloads_image() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('v'));
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            url: "https://cdn.discordapp.com/cat.png".to_owned(),
            filename: "cat.png".to_owned(),
            source: DownloadAttachmentSource::ImageViewer,
        })
    );
    assert_eq!(
        state.image_viewer_download_message(),
        Some("Downloading image...")
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

    assert!(state.is_poll_vote_picker_open());

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
