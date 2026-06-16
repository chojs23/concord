use super::*;

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

    state.navigation.guilds.selected = 0;
    state.confirm_selected_guild();
    assert_eq!(channel_entry_names(&state), vec!["alice"]);
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
