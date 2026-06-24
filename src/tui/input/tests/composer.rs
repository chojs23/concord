use super::*;

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
fn thread_edit_selector_cycles_with_h_and_l() {
    let mut state = state_with_forum_channel_posts();
    state.open_thread_edit(Id::new(31));

    // Focus the auto-archive selector: Title -> Tags -> SlowMode -> AutoArchive.
    handle_key(&mut state, key(KeyCode::Tab));
    handle_key(&mut state, key(KeyCode::Tab));
    handle_key(&mut state, key(KeyCode::Tab));

    let initial = state
        .thread_edit_view()
        .expect("edit view")
        .auto_archive_label;

    // `l` cycles the value forward and `h` back, matching the arrow keys.
    handle_key(&mut state, char_key('l'));
    let forward = state
        .thread_edit_view()
        .expect("edit view")
        .auto_archive_label;
    assert_ne!(initial, forward);

    handle_key(&mut state, char_key('h'));
    let back = state
        .thread_edit_view()
        .expect("edit view")
        .auto_archive_label;
    assert_eq!(initial, back);
}

#[test]
fn forum_parent_composer_key_opens_post_overlay() {
    let mut state = state_with_forum_channel_posts();

    handle_key(&mut state, char_key('i'));

    assert!(!state.is_composing());
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ForumPostComposer)
    );
    assert_eq!(
        state
            .forum_post_composer_view()
            .map(|view| view.channel_label),
        Some("#announcements".to_owned())
    );
}

#[test]
fn forum_post_overlay_requires_enter_before_text_editing() {
    let mut state = state_with_forum_channel_posts();
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('x'));

    assert_eq!(
        state.forum_post_composer_view().map(|view| view.title),
        Some(String::new())
    );

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('x'));
    handle_key(&mut state, key(KeyCode::Esc));

    let view = state
        .forum_post_composer_view()
        .expect("forum post modal should still be open after canceling edit");
    assert_eq!(view.title, "x");
    assert_eq!(view.editing_field, None);
}

#[test]
fn forum_post_overlay_keys_submit_with_pasted_attachment() {
    let attachment = temp_upload_file("forum post.txt", b"attachment body");
    let mut state = state_with_forum_channel_posts();
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, key(KeyCode::Enter));
    for value in "Need help".chars() {
        handle_key(&mut state, char_key(value));
    }
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Tab));
    handle_key(&mut state, key(KeyCode::Enter));
    for value in "The client crashes".chars() {
        handle_key(&mut state, char_key(value));
    }
    assert!(handle_paste(
        &mut state,
        attachment.to_str().expect("temp path is valid unicode"),
    ));
    handle_key(&mut state, key(KeyCode::Enter));

    // Tab from Body past Attachments and Tags to the submit button, then Enter.
    handle_key(&mut state, key(KeyCode::Tab));
    handle_key(&mut state, key(KeyCode::Tab));
    handle_key(&mut state, key(KeyCode::Tab));
    let Some(AppCommand::CreateForumPost { post }) = handle_key(&mut state, key(KeyCode::Enter))
    else {
        panic!("forum post overlay should submit create command from the Create Post button");
    };

    assert_eq!(post.channel_id, Id::new(20));
    assert_eq!(post.title, "Need help");
    assert_eq!(post.content, "The client crashes");
    assert_eq!(post.attachments.len(), 1);
    assert_eq!(post.attachments[0].filename, "forum post.txt");
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ForumPostComposer)
    );
    remove_temp_upload_file(&attachment);
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
fn esc_closes_composer_without_clearing_draft() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "draft".chars() {
        handle_key(&mut state, char_key(value));
    }

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_composing());
    assert_eq!(state.composer_input(), "draft");
    assert!(!state.should_quit());

    handle_key(&mut state, char_key('i'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "draft");
    assert_eq!(state.composer_cursor_byte_index(), "draft".len());
}

#[test]
fn ctrl_c_clears_composer_without_quitting() {
    let attachment = temp_upload_file("clear attachment.txt", b"attached");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "draft".chars() {
        handle_key(&mut state, char_key(value));
    }
    assert!(handle_paste(
        &mut state,
        attachment.to_str().expect("temp path is valid unicode"),
    ));

    handle_key(&mut state, ctrl_key('c'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
    assert_eq!(state.composer_cursor_byte_index(), 0);
    assert!(state.pending_composer_attachments().is_empty());
    assert!(!state.should_quit());
    remove_temp_upload_file(&attachment);
}

#[test]
fn ctrl_c_does_not_quit_dashboard() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, ctrl_key('c'));

    assert!(!state.should_quit());
}

#[test]
fn composer_treats_vim_keys_as_text() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('k'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "jk");
}

#[test]
fn plus_colon_in_composer_opens_reaction_picker_for_selected_message() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('+'));
    handle_key(&mut state, char_key(':'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
    assert_eq!(state.emoji_reaction_filter(), None);
    assert!(!state.is_editing_emoji_reaction_filter());

    let command = handle_key(&mut state, char_key('2'));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
        })
    );
    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn plus_colon_without_selected_message_stays_composer_text() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('+'));
    handle_key(&mut state, char_key(':'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "+:");
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn composer_ignores_unhandled_control_characters() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, ctrl_key('a'));
    handle_key(&mut state, ctrl_key('l'));
    handle_key(&mut state, ctrl_key('k'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
}

#[test]
fn modified_enter_and_ctrl_j_insert_newline_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('h'));
    handle_key(&mut state, shift_enter());
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, ctrl_enter());
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, alt_enter());
    handle_key(&mut state, char_key('k'));
    handle_key(&mut state, ctrl_key('j'));
    handle_key(&mut state, char_key('l'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "h\ni\nj\nk\nl");

    let mut completion_state = state_with_channel_tree();
    completion_state.focus_pane(FocusPane::Channels);
    handle_key(&mut completion_state, key(KeyCode::Down));
    handle_key(&mut completion_state, key(KeyCode::Enter));
    handle_key(&mut completion_state, char_key('i'));
    for ch in ":heart".chars() {
        handle_key(&mut completion_state, char_key(ch));
    }

    handle_key(&mut completion_state, shift_enter());
    handle_key(&mut completion_state, char_key('x'));

    assert!(completion_state.is_composing());
    assert_eq!(completion_state.composer_input(), ":heart\nx");
}

#[test]
fn composer_cursor_edits_in_middle() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "abcd".chars() {
        handle_key(&mut state, char_key(value));
    }

    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, char_key('X'));
    assert_eq!(state.composer_input(), "abXcd");
    assert_eq!(state.composer_cursor_byte_index(), 3);

    handle_key(&mut state, key(KeyCode::Backspace));
    assert_eq!(state.composer_input(), "abcd");
    assert_eq!(state.composer_cursor_byte_index(), 2);

    handle_key(&mut state, key(KeyCode::Delete));
    assert_eq!(state.composer_input(), "abcd");
    assert_eq!(state.composer_cursor_byte_index(), 2);

    handle_key(&mut state, key(KeyCode::Home));
    handle_key(&mut state, char_key('>'));
    handle_key(&mut state, key(KeyCode::End));
    handle_key(&mut state, char_key('!'));

    assert_eq!(state.composer_input(), ">abcd!");
    assert_eq!(
        state.composer_cursor_byte_index(),
        state.composer_input().len()
    );
}

#[test]
fn modified_backspace_and_ctrl_w_delete_previous_composer_word() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(&mut state, "hello brave world"));
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
    );

    assert_eq!(state.composer_input(), "hello brave ");
    assert_eq!(state.composer_cursor_byte_index(), "hello brave ".len());

    handle_key(&mut state, ctrl_key('w'));

    assert_eq!(state.composer_input(), "hello ");
    assert_eq!(state.composer_cursor_byte_index(), "hello ".len());

    assert!(handle_paste(&mut state, "brave world"));
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
    );

    assert_eq!(state.composer_input(), "hello brave ");
    assert_eq!(state.composer_cursor_byte_index(), "hello brave ".len());
}

#[test]
fn composer_keymap_can_remap_editor_and_delete_word() {
    let state = state_with_keymap(KeymapOptions {
        composer: [
            ("OpenEditor".to_owned(), KeymapBinding::one("<C-o>")),
            (
                "DeletePreviousWord".to_owned(),
                KeymapBinding::one("<A-backspace>"),
            ),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    let mut state = state_with_messages_from_state(state, 1);
    state.start_composer();
    assert!(state.is_composing());
    for value in "hello brave world".chars() {
        state.push_composer_char(value);
    }

    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
    );
    assert!(!state.take_open_composer_in_editor_request());

    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(state.take_open_composer_in_editor_request());

    handle_key(&mut state, ctrl_key('w'));
    assert_eq!(state.composer_input(), "hello brave world");

    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
    );
    assert_eq!(state.composer_input(), "hello brave ");
}

#[test]
fn composer_up_down_moves_cursor_between_lines() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    assert!(handle_paste(&mut state, "abc\nde\nfghi"));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.composer_cursor_byte_index(), "abc\nde".len());

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.composer_cursor_byte_index(), "ab".len());

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.composer_cursor_byte_index(), "abc\nde".len());

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.composer_cursor_byte_index(), "abc\nde\nfg".len());

    state.clear_composer_input();
    assert!(handle_paste(&mut state, "가나\nabc"));

    handle_key(&mut state, key(KeyCode::Home));
    handle_key(&mut state, key(KeyCode::Right));
    handle_key(&mut state, key(KeyCode::Down));

    assert_eq!(state.composer_cursor_byte_index(), "가나\na".len());
    assert!(
        state
            .composer_input()
            .is_char_boundary(state.composer_cursor_byte_index())
    );
}

#[test]
fn paste_inserts_text_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(&mut state, "hello\r\nworld"));

    assert_eq!(state.composer_input(), "hello\nworld");
}

#[test]
fn paste_inserts_text_at_composer_cursor() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "helloworld".chars() {
        handle_key(&mut state, char_key(value));
    }
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Left));
    }

    assert!(handle_paste(&mut state, " "));

    assert_eq!(state.composer_input(), "hello world");
    assert_eq!(state.composer_cursor_byte_index(), "hello ".len());
}

#[test]
fn paste_file_path_adds_pending_attachment() {
    let path = temp_upload_file("paste path.txt", b"hello");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(
        &mut state,
        path.to_str().expect("temp path is valid unicode")
    ));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(
        state.pending_composer_attachments()[0]
            .path()
            .expect("upload is file backed"),
        path
    );
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "paste path.txt"
    );
    assert_eq!(state.pending_composer_attachments()[0].size_bytes, 5);
    remove_temp_upload_file(&path);
}

#[test]
fn paste_single_quoted_file_path_adds_pending_attachment() {
    let path = temp_upload_file("quoted path.txt", b"quoted");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    let pasted = format!("'{}'", path.to_str().expect("temp path is valid unicode"));

    assert!(handle_paste(&mut state, &pasted));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(
        state.pending_composer_attachments()[0]
            .path()
            .expect("upload is file backed"),
        path
    );
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "quoted path.txt"
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_backslash_escaped_file_path_adds_pending_attachment() {
    let path = temp_upload_file("escaped path.txt", b"escaped");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    let pasted = path
        .to_str()
        .expect("temp path is valid unicode")
        .replace(' ', "\\ ");

    assert!(handle_paste(&mut state, &pasted));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(
        state.pending_composer_attachments()[0]
            .path()
            .expect("upload is file backed"),
        path
    );
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "escaped path.txt"
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_file_uri_list_can_submit_attachment_only_message() {
    let path = temp_upload_file("uri path.txt", b"upload");
    let uri = format!(
        "x-special/gnome-copied-files\ncopy\nfile://{}",
        path.to_string_lossy().replace(' ', "%20")
    );
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(&mut state, &uri));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.pending_composer_attachments(), &[]);
    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: String::new(),
            reply_to: None,
            attachments: vec![crate::discord::MessageAttachmentUpload::from_path(
                path.clone(),
                "uri path.txt".to_owned(),
                6,
            )],
        })
    );
    remove_temp_upload_file(&path);
}

#[test]
fn delete_removes_last_pending_attachment() {
    let first = temp_upload_file("first.txt", b"first");
    let second = temp_upload_file("second.txt", b"second");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('x'));
    let pasted = format!(
        "{}\n{}",
        first.to_str().expect("temp path is valid unicode"),
        second.to_str().expect("temp path is valid unicode")
    );

    assert!(handle_paste(&mut state, &pasted));
    handle_key(&mut state, key(KeyCode::Delete));

    assert_eq!(state.composer_input(), "x");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(
        state.pending_composer_attachments()[0]
            .path()
            .expect("upload is file backed"),
        first
    );
    remove_temp_upload_file(&first);
    remove_temp_upload_file(&second);
}

#[test]
fn paste_file_path_while_editing_inserts_text_instead_of_attachment() {
    let path = temp_upload_file("edit paste.txt", b"no attach");
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('e'));

    assert!(handle_paste(
        &mut state,
        path.to_str().expect("temp path is valid unicode")
    ));

    assert!(state.pending_composer_attachments().is_empty());
    assert!(
        state
            .composer_input()
            .contains(path.to_str().expect("temp path is valid unicode"))
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_is_ignored_when_not_composing() {
    let mut state = state_with_channel_tree();

    assert!(!handle_paste(&mut state, "hello"));

    assert_eq!(state.composer_input(), "");
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

    // Composer stays open after submit so the user can keep typing
    // back-to-back messages.
    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: "h\ni".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn enter_confirms_emoji_picker_before_submit() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":heart".chars() {
        handle_key(&mut state, char_key(ch));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert_eq!(state.composer_input(), "❤️ ");
    assert!(state.is_composing());
}

#[test]
fn enter_submits_no_match_emoji_query_without_hidden_picker() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":qq".chars() {
        handle_key(&mut state, char_key(ch));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: ":qq".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn tab_confirms_emoji_picker() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":heart".chars() {
        handle_key(&mut state, char_key(ch));
    }

    handle_key(&mut state, key(KeyCode::Tab));

    assert_eq!(state.composer_input(), "❤️ ");
}

#[test]
fn enter_submits_complete_slash_command_when_optional_options_are_suggested() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    state.push_event(AppEvent::ApplicationCommandsLoaded {
        guild_id: Some(Id::new(1)),
        commands: vec![
            ApplicationCommandInfo {
                application_id: Id::new(200),
                version: "1".to_owned(),
                application_name: Some("WrongBot".to_owned()),
                description: "first achievements command".to_owned(),
                raw: serde_json::json!({
                    "id": "100",
                    "application_id": "200",
                    "version": "1",
                    "name": "achievements",
                }),
                ..ApplicationCommandInfo::test(Id::new(100), "achievements")
            },
            ApplicationCommandInfo {
                application_id: Id::new(201),
                version: "2".to_owned(),
                application_name: Some("TestBot".to_owned()),
                description: "selected achievements command".to_owned(),
                options: vec![ApplicationCommandOptionInfo {
                    description: "member option".to_owned(),
                    ..ApplicationCommandOptionInfo::test(6, "member")
                }],
                raw: serde_json::json!({
                    "id": "101",
                    "application_id": "201",
                    "version": "2",
                    "name": "achievements",
                }),
                ..ApplicationCommandInfo::test(Id::new(101), "achievements")
            },
        ],
    });
    handle_key(&mut state, char_key('i'));
    for ch in "/ach".chars() {
        handle_key(&mut state, char_key(ch));
    }
    assert_eq!(state.composer_command_query(), Some("/ach"));

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.composer_input(), "/achievements ");
    assert_eq!(state.composer_command_query(), Some(""));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert!(matches!(
        command,
        Some(AppCommand::RunApplicationCommand { ref invocation })
            if invocation.command_name == "achievements"
                && invocation.content == "/achievements"
                && invocation.command_identity.map(|identity| (identity.id, identity.application_id))
                    == Some((Id::new(101), Id::new(201)))
    ));
}

#[test]
fn emoji_picker_escape_returns_to_composer_text() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":he".chars() {
        handle_key(&mut state, char_key(ch));
    }

    handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(state.composer_input(), ":he");
    assert!(state.is_composing());
}

#[test]
fn direct_reply_shortcut_opens_composer() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('R'));

    assert_eq!(command, None);
    assert!(!state.is_message_action_context_active());
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
            attachments: Vec::new(),
        })
    );
}

#[test]
fn canceling_reply_composer_clears_reply_target() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('R'));
    handle_key(&mut state, char_key('x'));
    handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(state.composer_input(), "");

    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('n'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "n".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn canceling_edit_composer_clears_edit_draft() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('e'));
    handle_key(&mut state, char_key('!'));

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_composing());
    assert_eq!(state.composer_input(), "");

    handle_key(&mut state, char_key('i'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
}

#[test]
fn direct_reaction_shortcut_opens_emoji_picker() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('r'));

    assert_eq!(command, None);
    assert!(!state.is_message_action_context_active());
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
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
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn emoji_picker_selection_removes_existing_own_reaction() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::CurrentUserReactionAdd {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
    });
    open_emoji_picker(&mut state);

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::RemoveReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn emoji_picker_number_shortcut_selects_reaction() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    let command = handle_key(&mut state, char_key('2'));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn emoji_picker_slash_filter_matches_name_and_implementation_case_insensitively() {
    let mut state = state_with_custom_emoji_message();
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('T'));
    handle_key(&mut state, char_key('s'));

    assert_eq!(state.emoji_reaction_filter(), Some("Ts"));
    assert!(state.is_editing_emoji_reaction_filter());
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Custom {
            id: Id::new(51),
            name: Some("this".to_owned()),
            animated: false,
        })
    );

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(command, None);
    assert_eq!(state.emoji_reaction_filter(), Some("Ts"));
    assert!(!state.is_editing_emoji_reaction_filter());

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Custom {
                id: Id::new(51),
                name: Some("this".to_owned()),
                animated: false,
            },
        })
    );
}

#[test]
fn emoji_picker_filter_treats_vim_keys_as_text() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('k'));

    assert_eq!(state.emoji_reaction_filter(), Some("jk"));
    assert!(state.is_editing_emoji_reaction_filter());
    assert_ne!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );
}

#[test]
fn emoji_picker_filter_matches_remaining_unicode_emojis() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    for value in "rocket".chars() {
        handle_key(&mut state, char_key(value));
    }

    assert_eq!(state.emoji_reaction_filter(), Some("rocket"));
    assert!(state.is_editing_emoji_reaction_filter());
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("🚀".to_owned()))
    );
}

#[test]
fn emoji_picker_enter_locks_filter_before_activating_reaction() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    for value in "heart".chars() {
        handle_key(&mut state, char_key(value));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
    assert_eq!(state.emoji_reaction_filter(), Some("heart"));
    assert!(!state.is_editing_emoji_reaction_filter());

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
        })
    );
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

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );

    handle_key(&mut state, ctrl_key('p'));
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
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
    assert_eq!(state.selected_message(), 1);
}

#[test]
fn multiselect_poll_picker_toggles_and_submits_selected_answers() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let poll_row = state
        .selected_message_action_items()
        .iter()
        .position(|action| action.kind == MessageActionKind::OpenPollVotePicker)
        .expect("poll action should exist");
    for _ in 0..poll_row {
        handle_key(&mut state, key(KeyCode::Down));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(command, None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));

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
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));
}
