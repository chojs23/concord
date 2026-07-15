use super::*;

fn selected_dm_state(
    last_message_id: Option<Id<MessageMarker>>,
    ui_state: UiStateOptions,
) -> DashboardState {
    let mut state = DashboardState::new_with_options(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        ui_state,
    );
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id,
        ..dm_channel_info(Id::new(20), "alice")
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state
}

fn dm_history_message(message_id: u64, author_id: u64) -> MessageInfo {
    MessageInfo {
        guild_id: None,
        author_id: Id::new(author_id),
        author: format!("user-{author_id}"),
        ..MessageInfo::test(Id::new(20), Id::new(message_id))
    }
}

#[test]
fn direct_messages_are_sorted_by_latest_message_id() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);
}

#[test]
fn direct_message_selection_waits_for_channel_confirmation() {
    let mut state = state_with_direct_messages();

    state.confirm_selected_guild();
    assert_eq!(state.selected_channel_id(), None);

    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
}

#[test]
fn activate_channel_effect_moves_direct_message_cursor_to_target() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    assert_eq!(state.selected_channel(), 0);

    state.push_effect(AppEvent::ActivateChannel {
        channel_id: Id::new(30),
    });

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(state.selected_channel(), 2);
}

#[test]
fn direct_message_sorting_uses_channel_id_fallback() {
    let mut state = DashboardState::new();
    for (channel_id, name) in [(Id::new(10), "older-id"), (Id::new(30), "newer-id")] {
        state.push_event(AppEvent::ChannelUpsert(dm_channel_info(
            channel_id,
            name.to_owned(),
        )));
    }
    state.confirm_selected_guild();

    assert_eq!(channel_entry_names(&state), vec!["newer-id", "older-id"]);
}

#[test]
fn restoring_discord_snapshot_recovers_missed_guilds_and_direct_messages() {
    let guild_id: Id<GuildMarker> = Id::new(1);
    let guild_channel_id: Id<ChannelMarker> = Id::new(2);
    let dm_channel_id: Id<ChannelMarker> = Id::new(20);
    let mut snapshot = DiscordState::default();
    snapshot.apply_event(&AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(10)),
    });
    snapshot.apply_event(&guild_create_event(
        guild_id,
        "guild",
        vec![text_channel_info(guild_id, guild_channel_id, "general")],
    ));
    snapshot.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(200)),
        ..dm_channel_info(dm_channel_id, "alice")
    }));

    let mut state = DashboardState::new();
    state.restore_discord_snapshot(snapshot);

    assert_eq!(state.current_user(), Some("neo"));
    assert_eq!(state.current_user_id(), Some(Id::new(10)));
    assert_eq!(state.guild_pane_entries().len(), 2);

    state.confirm_selected_guild();
    assert_eq!(state.selected_guild_id(), Some(guild_id));
    assert_eq!(channel_entry_names(&state), vec!["general"]);

    state.navigation.guilds.list.selected = 0;
    state.confirm_selected_guild();
    assert_eq!(channel_entry_names(&state), vec!["alice"]);
}

#[test]
fn empty_dm_locks_the_composer_before_the_first_message() {
    let mut state = selected_dm_state(None, UiStateOptions::default());

    assert_eq!(state.composer_lock(), Some(ComposerLock::LoadingMessages));

    state.push_event(latest_history_loaded(Id::new(20), Vec::new()));
    assert_eq!(state.composer_lock(), Some(ComposerLock::EmptyChannel));
}

#[test]
fn group_dm_unlocks_after_message_history_loads() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(200)),
        name: "friends".to_owned(),
        ..ChannelInfo::test(Id::new(20), "group-dm")
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    assert_eq!(state.composer_lock(), Some(ComposerLock::LoadingMessages));

    state.push_event(latest_history_loaded(Id::new(20), Vec::new()));
    assert_eq!(state.composer_lock(), None);
}

#[test]
fn existing_dm_requires_five_current_user_messages_before_unlocking() {
    let state_before_history = || selected_dm_state(Some(Id::new(200)), UiStateOptions::default());

    let mut established = state_before_history();
    assert_eq!(
        established.composer_lock(),
        Some(ComposerLock::LoadingMessages)
    );
    established.push_event(latest_history_loaded(
        Id::new(20),
        (196..=200)
            .map(|message_id| dm_history_message(message_id, 1))
            .collect(),
    ));
    assert_eq!(established.composer_lock(), None);
    assert_eq!(
        established
            .take_ui_state_save_request()
            .expect("established DM should request persistence")
            .established_dms,
        vec![Id::new(20)]
    );

    let mut new_conversation = state_before_history();
    new_conversation.push_event(latest_history_loaded(
        Id::new(20),
        vec![
            dm_history_message(196, 1),
            dm_history_message(197, 1),
            dm_history_message(198, 1),
            dm_history_message(199, 1),
            dm_history_message(200, 99),
        ],
    ));
    assert_eq!(
        new_conversation.composer_lock(),
        Some(ComposerLock::NewConversation)
    );

    new_conversation.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id: Id::new(20),
        message_id: Id::new(201),
        author_id: Id::new(1),
        content: Some("hello".to_owned()),
        ..guild_message_create_fixture()
    }));
    assert_eq!(new_conversation.composer_lock(), None);

    let mut restored = selected_dm_state(
        Some(Id::new(200)),
        UiStateOptions {
            established_dms: vec![Id::new(20)],
            ..UiStateOptions::default()
        },
    );
    restored.push_event(latest_history_loaded(
        Id::new(20),
        vec![dm_history_message(200, 99)],
    ));
    assert_eq!(restored.composer_lock(), None);
}

#[test]
fn message_request_and_spam_dms_stay_locked_after_a_reply() {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(1)),
    });
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        is_message_request: Some(true),
        ..dm_channel_info(Id::new(20), "stranger")
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));

    // Even with one of our own messages present, an unaccepted request stays
    // locked: replying is exactly what trips the CAPTCHA gate.
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id: Id::new(20),
        message_id: Id::new(200),
        author_id: Id::new(1),
        content: Some("hey".to_owned()),
        ..guild_message_create_fixture()
    }));
    state.push_event(latest_history_loaded(Id::new(20), Vec::new()));
    assert_eq!(state.composer_lock(), Some(ComposerLock::MessageRequest));

    // Spam classification takes precedence and reports the spam reason.
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        is_spam: Some(true),
        ..dm_channel_info(Id::new(20), "stranger")
    }));
    assert_eq!(state.composer_lock(), Some(ComposerLock::Spam));
}

#[test]
fn direct_message_cursor_stays_on_same_channel_after_recency_sort() {
    let mut state = state_with_direct_messages();
    state.confirm_selected_guild();
    state.focus_pane(FocusPane::Channels);
    state.move_down();

    assert_eq!(state.selected_channel(), 1);
    assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);

    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id: Id::new(30),
        message_id: Id::new(300),
        author_id: Id::new(99),
        content: Some("new empty dm".to_owned()),
        ..guild_message_create_fixture()
    }));

    assert_eq!(channel_entry_names(&state), vec!["empty", "new", "old"]);
    assert_eq!(state.selected_channel(), 2);
}
