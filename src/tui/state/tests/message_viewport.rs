use std::time::{Duration, Instant};

use super::*;
use crate::discord::{AppCommand, MessageHistoryAfterMode, MessageHistoryLoadTarget};

#[test]
fn message_creation_keeps_viewport_on_latest() {
    let state = state_with_messages(3);

    assert_eq!(state.selected_message(), 2);
}

#[test]
fn message_scroll_preserves_position_when_not_following() {
    let mut state = state_with_messages(5);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(6);

    assert_eq!(state.selected_message(), 4);
    assert!(state.message_auto_follow());

    state.move_up();
    assert_eq!(state.selected_message(), 3);
    assert!(!state.message_auto_follow());

    push_text_message(&mut state, 6, "msg 6");

    assert_eq!(state.selected_message(), 3);
    assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
    // Cursor moved up but the viewport still showed the latest, so the new
    // event engaged auto-scroll (without moving the cursor).
    assert!(state.message_auto_follow());

    let mut state = state_with_messages(5);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(2);
    state.move_up();
    state.move_up();
    assert!(!state.message_auto_follow());

    let selected_message_id = state.messages()[state.selected_message()].id;
    let selected_message = state.selected_message();
    let message_scroll = state.message_scroll();
    let previous_revision = SnapshotRevision {
        global: 1,
        navigation: 1,
        message: 1,
        detail: 1,
    };
    let mut updated_discord = state.discord.clone();
    updated_discord.apply_event(&latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("new message".to_owned()),
            ..message_info(Id::new(2), 6)
        }],
    ));
    let snapshot = updated_discord.snapshot(SnapshotRevision {
        global: 2,
        navigation: 1,
        message: 2,
        detail: 1,
    });

    state.restore_discord_snapshot_areas(&snapshot, previous_revision);

    assert_eq!(
        state.messages()[state.selected_message()].id,
        selected_message_id
    );
    assert_eq!(state.selected_message(), selected_message);
    assert_eq!(state.message_scroll(), message_scroll);
    assert!(!state.message_auto_follow());
    assert!(
        state
            .messages()
            .iter()
            .any(|message| message.content.as_deref() == Some("new message"))
    );
}

#[test]
fn user_sent_message_from_history_position_does_not_force_follow() {
    let me: Id<UserMarker> = Id::new(10);
    let mut state = state_with_messages(5);
    // Pretend the Ready event came through so the state knows who "we" are.
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(2);

    // Scroll up far enough that the latest message is no longer visible
    // and the cursor is parked on an older message.
    state.move_up();
    state.move_up();
    state.move_up();
    assert_eq!(state.selected_message(), 1);
    assert!(!state.message_auto_follow());

    let parked_message_id = state.messages()[state.selected_message()].id;

    // Simulate the REST send response arriving as a self-authored
    // MessageCreate. Auto-follow must not yank the cursor down because the
    // user was reading older history.
    state.push_event(message_create_event(
        guild_text_message(99, "hello").with_author(me, "me"),
    ));

    let messages = state.messages();
    assert_eq!(messages[state.selected_message()].id, parked_message_id);
    assert!(!state.message_auto_follow());
    assert_eq!(state.new_messages_marker_message_id(), None);
}

#[test]
fn image_preview_rows_keep_latest_message_visible_when_auto_following() {
    let mut state = state_with_image_messages(6, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(6);

    assert_eq!(state.message_scroll(), 0);

    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_scroll() > 0 || state.message_line_scroll() > 0);
    let selected_bottom = state
        .selected_message_rendered_row(200, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(200, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn image_preview_scrolloff_keeps_selected_message_visible() {
    let mut state = state_with_image_messages(8, &[5, 6, 7]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);

    while state.selected_message() > 3 {
        state.move_up();
    }
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert_eq!(state.following_message_rendered_rows(200, 16, 3, 3), 15);
    let selected_bottom = state
        .selected_message_rendered_row(200, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(200, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn first_loaded_message_has_date_separator() {
    let state = state_with_message_ids([10, 11]);

    assert!(state.message_starts_new_day_at(0));
    assert_eq!(state.message_extra_top_lines(0), 1);
}

#[test]
fn incoming_message_while_scrolled_away_sets_new_messages_marker() {
    let mut state = state_with_messages(5);
    clear_scheduled_read_ack(&mut state);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();

    push_text_message(&mut state, 6, "new while reading older messages");

    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(6)));
    assert_eq!(state.new_messages_count(), 1);
    assert_eq!(state.message_extra_top_lines(5), 0);
    assert_eq!(state.channel_unread(Id::new(2)), ChannelUnreadState::Unread);
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn recently_viewed_channel_does_not_force_stale_reload() {
    let mut state = warm_state_with_message_channels();
    let now = Instant::now();

    state.activate_channel_at(Id::new(2), now);
    state.activate_channel_at(Id::new(3), now + Duration::from_secs(1));
    state.activate_channel_at(Id::new(2), now + Duration::from_secs(29 * 60));

    assert!(!state.selected_message_history_needs_reload());
}

#[test]
fn stale_reopen_reload_need_survives_failure_and_clears_after_success() {
    let mut state = warm_state_with_message_channels();
    let now = Instant::now();

    state.activate_channel_at(Id::new(2), now);
    state.activate_channel_at(Id::new(3), now + Duration::from_secs(1));
    state.activate_channel_at(Id::new(2), now + Duration::from_secs(30 * 60 + 2));

    assert!(state.selected_message_history_needs_reload());

    state.push_event(AppEvent::MessageHistoryLoadFailed {
        channel_id: Id::new(2),
        target: MessageHistoryLoadTarget::Latest,
        message: "temporary failure".to_owned(),
    });
    assert!(state.selected_message_history_needs_reload());

    state.push_event(AppEvent::MessageHistoryRefreshed {
        channel_id: Id::new(2),
        messages: vec![message_info(Id::new(2), 30), message_info(Id::new(2), 31)],
    });

    assert!(!state.selected_message_history_needs_reload());
    let message_ids = state
        .messages()
        .iter()
        .map(|message| message.id)
        .collect::<Vec<_>>();
    assert_eq!(message_ids, vec![Id::new(30), Id::new(31)]);

    state.activate_channel_at(Id::new(2), now + Duration::from_secs(30 * 60 + 3));
    assert!(!state.selected_message_history_needs_reload());
}

fn warm_state_with_message_channels() -> DashboardState {
    let mut state = state_with_many_channels(3);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![message_info(Id::new(2), 20)],
    ));
    state.push_event(latest_history_loaded(
        Id::new(3),
        vec![message_info(Id::new(3), 30)],
    ));
    state
}

#[test]
fn catch_up_messages_while_scrolled_away_set_new_messages_marker() {
    let mut state = state_with_messages(5);
    clear_scheduled_read_ack(&mut state);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();

    state.push_event(AppEvent::MessageHistoryAfterLoaded {
        channel_id: Id::new(2),
        after: Id::new(5),
        messages: vec![message_info(Id::new(2), 7), message_info(Id::new(2), 6)],
        has_more: false,
        mode: MessageHistoryAfterMode::CatchUp,
    });

    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(6)));
    assert_eq!(state.new_messages_count(), 2);
    assert!(state.drain_pending_commands().is_empty());
}

#[test]
fn new_messages_count_includes_messages_after_marker() {
    let mut state = state_with_messages(5);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();

    push_text_message(&mut state, 6, "first unread");
    push_text_message(&mut state, 7, "second unread");

    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(6)));
    assert_eq!(state.new_messages_count(), 2);
}

#[test]
fn viewport_scroll_away_from_latest_sets_new_messages_marker_even_when_cursor_is_latest() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let selected = state.selected_message();

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    push_text_message(&mut state, 11, "new while viewport is above latest");

    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.new_messages_marker_message_id(), Some(Id::new(11)));
    assert_eq!(state.new_messages_count(), 1);
}

#[test]
fn new_messages_marker_clears_when_user_reaches_latest() {
    enum LatestAction {
        JumpBottom,
        ScrollViewportBottom,
        ScrollViewportDown,
    }

    for action in [
        LatestAction::JumpBottom,
        LatestAction::ScrollViewportBottom,
        LatestAction::ScrollViewportDown,
    ] {
        let mut state = state_with_messages(5);
        clear_scheduled_read_ack(&mut state);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(3);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.jump_top();
        push_text_message(&mut state, 6, "new while reading older messages");

        match action {
            LatestAction::JumpBottom => state.jump_bottom(),
            LatestAction::ScrollViewportBottom => state.scroll_message_viewport_bottom(),
            LatestAction::ScrollViewportDown => {
                for _ in 0..50 {
                    if state.new_messages_marker_message_id().is_none() {
                        break;
                    }
                    state.scroll_message_viewport_down();
                }
            }
        }

        assert_eq!(state.new_messages_marker_message_id(), None);
        assert_eq!(
            state.drain_pending_commands(),
            vec![AppCommand::ScheduleAckChannel {
                channel_id: Id::new(2),
                message_id: Id::new(6),
            }]
        );
    }
}

#[test]
fn viewport_scroll_back_to_latest_re_engages_auto_follow_when_cursor_is_latest() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let selected = state.selected_message();

    state.scroll_message_viewport_up();
    state.scroll_message_viewport_up();
    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    for _ in 0..50 {
        state.scroll_message_viewport_down();
    }

    assert_eq!(state.selected_message(), selected);
    assert!(!state.message_auto_follow());

    push_text_message(&mut state, 11, "new while viewport is latest again");

    assert_eq!(state.messages()[state.selected_message()].id, Id::new(11));
    assert!(state.message_auto_follow());
}

#[test]
fn incoming_message_at_latest_does_not_set_new_messages_marker() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);

    push_text_message(&mut state, 3, "new while following latest");

    assert_eq!(state.new_messages_marker_message_id(), None);
}

#[test]
fn message_scroll_uses_scrolloff() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);

    assert_eq!(state.message_scroll(), 5);

    state.move_up();
    state.move_up();
    assert_eq!(state.selected_message(), 9);
    assert_eq!(state.message_scroll(), 5);

    state.move_up();
    assert_eq!(state.selected_message(), 8);
    assert_eq!(state.message_scroll(), 5);
}

#[test]
fn message_auto_follow_keeps_latest_message_at_bottom_after_rendered_clamp() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);

    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_auto_follow());
    assert_eq!(state.selected_message(), 11);
    assert_eq!(state.message_scroll(), 7);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 4);
}

#[test]
fn message_selection_centers_selected_message_when_possible() {
    let mut state = state_with_messages(12);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(7);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    for _ in 0..4 {
        state.move_up();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
    }

    assert_eq!(state.selected_message(), 7);
    assert_eq!(state.message_scroll(), 5);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 2);
}

#[test]
fn message_selection_centers_with_line_offset_inside_previous_message() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    for id in 2..=5 {
        push_text_message(&mut state, id, &format!("msg {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 4);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 1);
}

#[test]
fn message_selection_keeps_top_when_next_message_is_already_visible() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    for id in 2..=5 {
        push_text_message(&mut state, id, &format!("msg {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 5);
}

#[test]
fn message_selection_centers_with_image_preview_height() {
    let mut state = state_with_image_messages(8, &[4]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    for _ in 0..3 {
        state.move_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
    }

    assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
    assert_eq!(state.selected_message_rendered_height(200, 16, 3), 7);
    assert_eq!(state.message_scroll(), 2);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(200, 16, 3), 1);
}

#[test]
fn message_viewport_scrolls_by_rendered_line() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);

    state.scroll_message_viewport_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 1);
    assert_eq!(state.selected_message(), 0);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 3);
}

#[test]
fn message_half_page_scrolls_by_rendered_rows() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    for id in 2..=5 {
        push_text_message(&mut state, id, &format!("msg {id}"));
    }
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.half_page_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);

    state.half_page_up();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn viewport_scroll_moves_to_next_message_after_current_message() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 5);
    assert_eq!(state.selected_message(), 0);
}

#[test]
fn focused_message_selection_returns_none_when_viewport_scrolled_past_selection() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    for _ in 0..6 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.focused_message_selection(), Some(0));
}

#[test]
fn moving_cursor_to_first_message_resets_top_line_scroll() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    for _ in 0..2 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }
    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert!(state.message_line_scroll() > 0);

    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
    assert_eq!(state.selected_message_rendered_row(5, 16, 3), 0);
}

#[test]
fn jumping_to_first_message_resets_item_scroll_when_view_has_spare_rows() {
    let mut state = state_with_messages(20);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(20);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert!(state.message_scroll() > 0);

    state.jump_top();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn viewport_scrolls_by_rendered_line_when_selected_message_is_below_top() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message(), 0);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    assert_eq!(state.selected_message(), 1);
    let selected_bottom = state
        .selected_message_rendered_row(5, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(5, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn tall_message_clamp_keeps_next_selected_message_visible() {
    let mut state =
        state_with_single_message_content("abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    state.move_down();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);

    let selected_bottom = state
        .selected_message_rendered_row(5, 16, 3)
        .saturating_add(
            state
                .selected_message_rendered_height(5, 16, 3)
                .saturating_sub(1),
        );
    assert!(selected_bottom < state.message_view_height());
}

#[test]
fn viewport_scroll_up_enters_previous_long_message_at_last_line() {
    let mut state = state_with_single_message_content("abcdefghijkl");
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("next".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.jump_top();
    state.clamp_message_viewport_for_image_previews(5, 16, 3);
    for _ in 0..3 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
    }

    state.scroll_message_viewport_up();

    assert_eq!(state.message_scroll(), 0);
    assert_eq!(state.message_line_scroll(), 2);
    assert_eq!(state.selected_message(), 0);
}

#[test]
fn viewport_scroll_does_not_move_list_pane_selection() {
    let mut guild_state = state_with_many_guilds(8);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(3);
    let selected_guild = guild_state.selected_guild();
    let guild_scroll = guild_state.guild_scroll();

    guild_state.scroll_focused_pane_viewport_down();
    guild_state.scroll_focused_pane_viewport_down();
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 2);
    assert_eq!(guild_state.focused_guild_selection(), None);

    guild_state.scroll_focused_pane_viewport_up();
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 1);

    let mut channel_state = state_with_many_channels(8);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(3);
    let selected_channel = channel_state.selected_channel();
    let channel_scroll = channel_state.channel_scroll();

    channel_state.scroll_focused_pane_viewport_down();
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll + 1);
    assert!(channel_state.selected_channel() < channel_state.channel_scroll());

    let mut member_state = state_with_members(8);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(3);
    let selected_member = member_state.selected_member();
    let member_scroll = member_state.member_scroll();

    member_state.scroll_focused_pane_viewport_down();
    member_state.scroll_focused_pane_viewport_down();
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll + 2);
    assert_eq!(member_state.focused_member_selection_line(), None);
}

#[test]
fn repeated_viewport_scroll_survives_view_height_sync() {
    let mut guild_state = state_with_many_guilds(12);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(4);
    let selected_guild = guild_state.selected_guild();
    let guild_scroll = guild_state.guild_scroll();
    for _ in 0..3 {
        guild_state.scroll_focused_pane_viewport_down();
        guild_state.set_guild_view_height(4);
    }
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll + 3);

    let mut channel_state = state_with_many_channels(12);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(4);
    let selected_channel = channel_state.selected_channel();
    let channel_scroll = channel_state.channel_scroll();
    for _ in 0..3 {
        channel_state.scroll_focused_pane_viewport_down();
        channel_state.set_channel_view_height(4);
    }
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll + 3);

    let mut member_state = state_with_members(12);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(4);
    let selected_member = member_state.selected_member();
    let member_scroll = member_state.member_scroll();
    for _ in 0..3 {
        member_state.scroll_focused_pane_viewport_down();
        member_state.set_member_view_height(4);
    }
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll + 3);
}

#[test]
fn viewport_scroll_survives_selection_clamp_after_events() {
    let mut guild_state = state_with_many_guilds(12);
    guild_state.focus_pane(FocusPane::Guilds);
    guild_state.set_guild_view_height(4);
    let selected_guild = guild_state.selected_guild();
    guild_state.scroll_focused_pane_viewport_down();
    guild_state.scroll_focused_pane_viewport_down();
    let guild_scroll = guild_state.guild_scroll();
    guild_state.push_event(AppEvent::UpdateAvailable {
        latest_version: "tick".to_owned(),
    });
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll);
    let guild_snapshot = guild_state.discord.clone();
    guild_state.restore_discord_snapshot(guild_snapshot);
    assert_eq!(guild_state.selected_guild(), selected_guild);
    assert_eq!(guild_state.guild_scroll(), guild_scroll);

    let mut channel_state = state_with_many_channels(12);
    channel_state.focus_pane(FocusPane::Channels);
    channel_state.set_channel_view_height(4);
    let selected_channel = channel_state.selected_channel();
    channel_state.scroll_focused_pane_viewport_down();
    channel_state.scroll_focused_pane_viewport_down();
    let channel_scroll = channel_state.channel_scroll();
    channel_state.push_event(AppEvent::UpdateAvailable {
        latest_version: "tick".to_owned(),
    });
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll);
    let channel_snapshot = channel_state.discord.clone();
    channel_state.restore_discord_snapshot(channel_snapshot);
    assert_eq!(channel_state.selected_channel(), selected_channel);
    assert_eq!(channel_state.channel_scroll(), channel_scroll);

    let mut member_state = state_with_members(12);
    member_state.focus_pane(FocusPane::Members);
    member_state.set_member_view_height(4);
    let selected_member = member_state.selected_member();
    member_state.scroll_focused_pane_viewport_down();
    member_state.scroll_focused_pane_viewport_down();
    let member_scroll = member_state.member_scroll();
    member_state.push_event(AppEvent::UpdateAvailable {
        latest_version: "tick".to_owned(),
    });
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll);
    let member_snapshot = member_state.discord.clone();
    member_state.restore_discord_snapshot(member_snapshot);
    assert_eq!(member_state.selected_member(), selected_member);
    assert_eq!(member_state.member_scroll(), member_scroll);
}

#[test]
fn message_half_page_up_disables_follow() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    state.half_page_up();

    assert_eq!(state.selected_message(), 0);
    assert_eq!(state.message_scroll(), 0);
    assert!(!state.message_auto_follow());
}

#[test]
fn message_jump_bottom_re_engages_auto_follow() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);

    state.move_up();
    assert!(!state.message_auto_follow());

    state.jump_bottom();

    // Cursor is back on the latest message, so auto-follow turns on again
    // (sticky-bottom rule).
    assert_eq!(state.selected_message(), 9);
    assert!(state.message_auto_follow());
}

#[test]
fn message_half_page_down_re_engages_auto_follow_after_viewport_returns_to_latest() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    state.half_page_up();
    assert!(!state.message_auto_follow());

    state.half_page_down();
    assert_eq!(state.selected_message(), 9);
    assert_eq!(state.message_scroll(), 2);
    assert!(!state.message_auto_follow());

    state.half_page_down();
    assert!(state.message_auto_follow());
}

#[test]
fn history_load_preserves_manual_scroll_position_by_message_id() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(3);
    state.move_up();
    state.move_up();

    let selected_id = state.messages()[state.selected_message()].id;
    let scroll_id = state.messages()[state.message_scroll()].id;

    state.push_event(latest_history_loaded(
        channel_id,
        vec![message_info(channel_id, 5)],
    ));

    assert_eq!(state.messages()[state.selected_message()].id, selected_id);
    assert_eq!(state.messages()[state.message_scroll()].id, scroll_id);
    assert!(!state.message_auto_follow());
}

#[test]
fn older_history_request_emits_visible_cursor_target() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5)],
    });

    state.move_up();
    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(5)),
        })
    );
}

#[test]
fn older_history_request_advances_after_cache_limit_retention() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids(10..=209);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: vec![message_info(channel_id, 5)],
    });

    assert_eq!(
        state.messages().last().map(|message| message.id),
        Some(Id::new(209))
    );

    state.move_up();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(5)),
        })
    );
}

#[test]
fn older_history_request_leaves_empty_page_exhaustion_to_backend() {
    let channel_id: Id<ChannelMarker> = Id::new(2);
    let mut state = state_with_message_ids([10, 11, 12]);
    state.focus_pane(FocusPane::Messages);
    state.jump_top();

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );

    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id,
        before: Some(Id::new(10)),
        messages: Vec::new(),
    });

    assert_eq!(
        state.next_older_history_command(),
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(Id::new(10)),
        })
    );
}
