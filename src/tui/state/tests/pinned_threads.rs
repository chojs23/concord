use super::*;
use crate::discord::AppCommand;
use crate::tui::state::MessagePaneSource;

#[test]
fn channel_show_pinned_messages_action_enters_pinned_message_view() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    state.open_selected_channel_actions();
    state.select_channel_action_row(2);

    let command = state.activate_selected_channel_action();

    assert_eq!(command, None);
    assert!(state.is_pinned_message_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::PinnedMessages {
            channel_id: Id::new(2)
        })
    );
    assert!(!state.is_channel_leader_action_active());
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert!(!state.message_auto_follow());
}

#[test]
fn pinned_message_view_title_mentions_channel_and_pins() {
    let mut state = state_with_messages(1);

    assert_eq!(state.message_pane_title(), "#general");

    state.enter_pinned_message_view(Id::new(2));

    assert_eq!(state.message_pane_title(), "#general pinned messages");
}

#[test]
fn pinned_message_view_suppresses_unread_divider_and_banner() {
    let mut state = state_with_message_ids([1, 2, 3]);
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![read_state_info(Id::new(2), Some(Id::new(1)), 0)],
    });
    state.activate_channel(Id::new(2));
    assert_eq!(state.unread_divider_message_index(), Some(1));
    assert!(state.unread_banner().is_some());

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(Id::new(2), 3)],
    });
    state.enter_pinned_message_view(Id::new(2));

    assert!(state.is_pinned_message_view());
    assert_eq!(state.unread_divider_message_index(), None);
    assert_eq!(state.unread_banner(), None);
    assert_eq!(state.message_extra_top_lines(0), 1);
}

#[test]
fn returning_from_pinned_message_view_restores_parent_message_window() {
    let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.move_up();
    state.move_up();
    let expected_selected = state.selected_message();
    let expected_scroll = state.message_scroll();
    let expected_line_scroll = state.message_line_scroll();

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id: Id::new(2),
        messages: vec![message_info(Id::new(2), 11)],
    });
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    assert!(state.return_from_pinned_message_view());

    assert!(!state.is_pinned_message_view());
    assert_eq!(state.selected_message(), expected_selected);
    assert_eq!(state.message_scroll(), expected_scroll);
    assert_eq!(state.message_line_scroll(), expected_line_scroll);
}

#[test]
fn pinned_message_view_does_not_request_older_history() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 11)],
    });
    state.enter_pinned_message_view(channel_id);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.messages().first().map(|message| message.id),
        Some(Id::new(11))
    );
    assert_eq!(state.next_older_history_command(), None);
}

#[test]
fn forum_channel_cannot_enter_pinned_message_view() {
    let mut state = state_with_forum_channel_posts();

    state.enter_pinned_message_view(Id::new(20));

    assert!(!state.is_pinned_message_view());
    assert_eq!(
        state.message_pane_source(),
        Some(MessagePaneSource::ForumPosts {
            channel_id: Id::new(20)
        })
    );
}

#[test]
fn pinned_only_messages_stay_out_of_normal_history() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 5)],
    });

    assert_eq!(
        state
            .messages()
            .into_iter()
            .map(|message| message.id.get())
            .collect::<Vec<_>>(),
        vec![10, 11, 12]
    );

    state.enter_pinned_message_view(channel_id);
    assert_eq!(
        state.messages().first().map(|message| message.id),
        Some(Id::new(5))
    );
}

#[test]
fn pinned_only_messages_do_not_become_older_history_cursor() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 5)],
    });
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
}

#[test]
fn channel_change_exits_pinned_message_view() {
    let mut state = state_with_many_channels(2);
    state.confirm_selected_channel();
    state.enter_pinned_message_view(Id::new(1));
    assert!(state.is_pinned_message_view());

    state.focus_pane(FocusPane::Channels);
    state.move_down();
    state.confirm_selected_channel();

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert!(!state.is_pinned_message_view());
}

#[test]
fn guild_change_exits_pinned_message_view() {
    let mut state = state_with_messages(1);
    state.push_event(guild_create_event(Id::new(2), "other guild", Vec::new()));
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    state.focus_pane(FocusPane::Guilds);
    state.move_down();
    state.confirm_selected_guild();

    assert_eq!(state.selected_guild_id(), Some(Id::new(2)));
    assert_eq!(state.selected_channel_id(), None);
    assert!(!state.is_pinned_message_view());
}

#[test]
fn pinned_messages_loaded_does_not_update_status() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::PinnedMessagesLoaded {
        channel_id,
        messages: vec![message_info(channel_id, 1)],
    });

    state.enter_pinned_message_view(channel_id);

    assert_eq!(state.messages().len(), 1);
}

#[test]
fn missing_thread_preview_requests_exact_latest_message_until_loaded() {
    let mut state = state_with_thread_created_message();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(30)),
        message_count: Some(12),
        member_count: None,
        total_message_sent: Some(14),
        ..thread_channel_info(Id::new(1), Id::new(2), Id::new(10), "release notes")
    }));

    assert_eq!(
        state.missing_thread_preview_load_requests(),
        vec![(Id::new(10), Id::new(30))]
    );

    state.push_event(AppEvent::ThreadPreviewLoaded {
        channel_id: Id::new(10),
        message: MessageInfo {
            content: Some("latest reply".to_owned()),
            ..message_info(Id::new(10), 30)
        },
    });
    let message = state.messages()[0];
    let summary = state
        .thread_summary_for_message(message)
        .expect("thread summary should resolve");

    assert_eq!(state.missing_thread_preview_load_requests(), Vec::new());
    assert_eq!(
        summary
            .latest_message_preview
            .map(|preview| (preview.author, preview.content)),
        Some(("neo".to_owned(), "latest reply".to_owned()))
    );
}

#[test]
fn missing_thread_preview_requests_skip_forum_posts_without_starter_preview() {
    let mut state = state_with_forum_channel_posts();
    state.push_event(AppEvent::SelectedMessageChannelChanged { channel_id: None });
    state.push_event(AppEvent::ChannelUpsert(forum_thread_info(
        Id::new(1),
        Id::new(20),
        30,
        "welcome",
        Some(300),
        false,
    )));
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(30),
        message_id: Id::new(300),
        author_id: Id::new(99),
        content: Some("starter preview".to_owned()),
        ..guild_message_create_fixture()
    }));

    let post = state
        .selected_forum_post_items()
        .into_iter()
        .find(|post| post.channel_id == Id::new(30))
        .expect("forum post should be visible");
    assert_eq!(post.preview_content.as_deref(), None);
    assert_eq!(state.missing_thread_preview_load_requests(), Vec::new());
}

#[test]
fn thread_summary_suppresses_preview_when_channel_latest_is_newer_than_cache() {
    let mut state = state_with_thread_created_message();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(40)),
        message_count: Some(12),
        member_count: None,
        total_message_sent: Some(14),
        ..thread_channel_info(Id::new(1), Id::new(2), Id::new(10), "release notes")
    }));
    state.push_event(AppEvent::ThreadPreviewLoaded {
        channel_id: Id::new(10),
        message: MessageInfo {
            content: Some("older cached reply".to_owned()),
            ..message_info(Id::new(10), 30)
        },
    });
    let message = state.messages()[0];
    let summary = state
        .thread_summary_for_message(message)
        .expect("thread summary should resolve");

    assert_eq!(summary.latest_message_id, Some(Id::new(40)));
    assert_eq!(summary.latest_message_preview, None);
    assert_eq!(
        state.missing_thread_preview_load_requests(),
        vec![(Id::new(10), Id::new(40))]
    );
}

#[test]
fn return_from_opened_thread_restores_scrolled_parent_message_window() {
    let mut state = state_with_thread_created_message_after_regular_message();
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(4);
    state.clamp_message_viewport_for_image_previews(16, 0, 0);
    state.scroll_message_viewport_top();
    for _ in 0..160 {
        state.scroll_message_viewport_down();
        if state.message_scroll() > 0 && state.message_line_scroll() > 0 {
            break;
        }
    }
    assert_eq!(state.selected_message(), 1);
    assert!(state.message_scroll() > 0);
    assert!(state.message_line_scroll() > 0);
    let expected_message_scroll = state.message_scroll();
    let expected_line_scroll = state.message_line_scroll();

    state.activate_message_action_kind(MessageActionKind::OpenThread);
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    assert!(state.return_from_opened_thread());

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), expected_message_scroll);
    assert_eq!(state.message_line_scroll(), expected_line_scroll);
}

#[test]
fn history_loaded_thread_created_message_opens_reference_thread_after_rename() {
    let mut state = state_with_thread_created_message();
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            message_kind: MessageKind::new(18),
            reference: Some(MessageReferenceInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Some(Id::new(10)),
                message_id: None,
            }),
            pinned: false,
            reactions: Vec::new(),
            content: Some("old thread name".to_owned()),
            ..message_info(Id::new(2), 2)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.jump_bottom();

    let actions = state.selected_message_action_items();
    assert!(
        actions
            .iter()
            .any(|action| action.kind == MessageActionKind::OpenThread)
    );

    state.activate_message_action_kind(MessageActionKind::OpenThread);

    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
}
