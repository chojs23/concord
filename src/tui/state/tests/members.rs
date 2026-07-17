use super::*;
use crate::discord::test_builders::guild_create_event;

#[test]
fn member_groups_use_roles_and_status_sorted_entries() {
    let guild_id = Id::new(1);
    let alice: Id<UserMarker> = Id::new(10);
    let bob: Id<UserMarker> = Id::new(20);
    let admin_role = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![text_channel_info(guild_id, Id::new(2), "general")],
        members: vec![
            member_with_roles(bob, "bob", vec![admin_role]),
            member_with_roles(alice, "alice", vec![admin_role]),
        ],
        presences: vec![(alice, PresenceStatus::Online), (bob, PresenceStatus::Idle)],
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            ..RoleInfo::test(admin_role, "Admin")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Admin");
    assert_eq!(groups[0].color, Some(0xFFAA00));
    assert_eq!(
        groups[0]
            .entries
            .iter()
            .map(|member| member.display_name())
            .collect::<Vec<_>>(),
        vec!["alice".to_owned(), "bob".to_owned()],
    );
}

#[test]
fn member_role_color_uses_highest_nonzero_role_color() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let low_role = Id::new(100);
    let zero_role = Id::new(101);
    let high_role = Id::new(102);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        members: vec![member_with_roles(
            user_id,
            "alice",
            vec![low_role, zero_role, high_role],
        )],
        presences: vec![(user_id, PresenceStatus::Online)],
        roles: vec![
            RoleInfo {
                color: Some(0x112233),
                position: 1,
                ..RoleInfo::test(low_role, "Low")
            },
            RoleInfo {
                color: Some(0),
                position: 99,
                ..RoleInfo::test(zero_role, "Zero")
            },
            RoleInfo {
                color: Some(0x445566),
                position: 10,
                ..RoleInfo::test(high_role, "High")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    let member = state.flattened_members()[0];

    assert_eq!(state.member_role_color(member), Some(0x445566));
}

#[test]
fn message_history_authors_missing_member_roles_are_requested_from_batch() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let author_id = Id::new(99);
    let mut state = state_with_writable_channel();
    let mut message = message_info(channel_id, 20);
    message.author_id = author_id;
    let mut duplicate = message_info(channel_id, 21);
    duplicate.author_id = author_id;
    let mut known_member = message_info(channel_id, 22);
    known_member.author_id = Id::new(10);
    known_member.author_role_ids = vec![Id::new(100)];

    assert_eq!(
        state.missing_message_author_member_requests(&[message.clone(), duplicate, known_member]),
        vec![(guild_id, vec![author_id])]
    );

    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id,
        member: member_with_username(author_id, "neo", "neo"),
    });

    assert_eq!(
        state.missing_message_author_member_requests(&[message]),
        Vec::new()
    );
}

#[test]
fn message_history_author_member_requests_chunk_at_gateway_limit() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = state_with_writable_channel();
    state.drain_pending_commands();
    let messages = (1..=105)
        .map(|offset| {
            let mut message = message_info(channel_id, 1_000 + offset);
            message.author_id = Id::new(1_000 + offset);
            message
        })
        .collect::<Vec<_>>();

    let requests = state.missing_message_author_member_requests(&messages);
    state.enqueue_message_author_member_requests(requests);

    assert_eq!(
        state.drain_pending_commands(),
        vec![
            AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: (1_001..=1_100).map(Id::new).collect(),
            },
            AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: (1_101..=1_105).map(Id::new).collect(),
            },
        ]
    );
}

#[test]
fn member_role_color_breaks_equal_position_ties_by_role_id() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let older_role = Id::new(100);
    let newer_role = Id::new(200);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        members: vec![member_with_roles(
            user_id,
            "alice",
            vec![newer_role, older_role],
        )],
        presences: vec![(user_id, PresenceStatus::Online)],
        roles: vec![
            RoleInfo {
                color: Some(0x112233),
                position: 10,
                ..RoleInfo::test(newer_role, "Newer")
            },
            RoleInfo {
                color: Some(0x445566),
                position: 10,
                ..RoleInfo::test(older_role, "Older")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    let member = state.flattened_members()[0];

    assert_eq!(state.member_role_color(member), Some(0x445566));
}

#[test]
fn member_groups_show_selected_group_dm_recipients() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        kind: "group-dm".to_owned(),
        recipients: Some(vec![
            ChannelRecipientInfo {
                status: Some(PresenceStatus::Idle),
                ..ChannelRecipientInfo::test(Id::new(30), "bob")
            },
            ChannelRecipientInfo {
                status: Some(PresenceStatus::Online),
                ..ChannelRecipientInfo::test(Id::new(10), "alice")
            },
        ]),
        ..dm_channel_info(channel_id, "project chat")
    }));

    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Members");
    assert_eq!(
        groups[0]
            .entries
            .iter()
            .map(|member| (member.display_name(), member.status()))
            .collect::<Vec<_>>(),
        vec![
            ("alice".to_owned(), PresenceStatus::Online),
            ("bob".to_owned(), PresenceStatus::Idle),
        ],
    );
}

#[test]
fn member_panel_title_shows_online_and_total_when_counts_available() {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(100),
        members: vec![member_info(Id::new(10), "alice")],
        presences: vec![(Id::new(10), PresenceStatus::Online)],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    state.push_event(guild_member_list_counts_event(guild_id, 25));

    let title = state.member_panel_title();
    let rendered: String = title.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(rendered, "● 25  ○ 100");
    assert_eq!(state.flattened_members().len(), 1);
}

#[test]
fn member_panel_title_stays_plain_without_guild_total_or_in_direct_messages() {
    let mut guild_state = DashboardState::new();
    guild_state.push_event(guild_create_event(GuildCreateFixture::new(Id::new(1))));
    guild_state.confirm_selected_guild();
    assert_eq!(guild_state.member_panel_title(), Line::from(" Members "));

    let mut dm_state = DashboardState::new();
    dm_state.push_event(AppEvent::ChannelUpsert(dm_channel_info(
        Id::new(20),
        "alice",
    )));
    dm_state.confirm_selected_guild();
    assert_eq!(dm_state.member_panel_title(), Line::from(" Members "));
}

#[test]
fn member_groups_keep_offline_hoisted_members_in_role_buckets() {
    let guild_id = Id::new(1);
    let admin_role = Id::new(100);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![text_channel_info(guild_id, Id::new(2), "general")],
        members: vec![
            member_with_roles(Id::new(10), "alice", vec![admin_role]),
            member_with_roles(Id::new(11), "amy", vec![admin_role]),
            member_info(Id::new(20), "bob"),
            member_info(Id::new(21), "ben"),
        ],
        presences: vec![
            // Admin online, admin offline, no-role online, no-role offline
            (Id::new(10), PresenceStatus::Online),
            (Id::new(11), PresenceStatus::Offline),
            (Id::new(20), PresenceStatus::Idle),
            (Id::new(21), PresenceStatus::Offline),
        ],
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            position: 10,
            hoist: true,
            ..RoleInfo::test(admin_role, "Admin")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(
        groups
            .iter()
            .map(|group| group.label.clone())
            .collect::<Vec<_>>(),
        vec![
            "Admin".to_owned(),
            "Online".to_owned(),
            "Offline".to_owned()
        ]
    );

    // Hoisted role groups include both online and offline members.
    let admin_names: Vec<_> = groups[0]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(admin_names, vec!["alice".to_owned(), "amy".to_owned()]);

    // Online group lists members with no hoisted role who aren't offline.
    let online_names: Vec<_> = groups[1]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(online_names, vec!["bob".to_owned()]);

    // Offline group lists only offline members that did not enter a role group.
    let offline_names: Vec<_> = groups[2]
        .entries
        .iter()
        .map(|m| m.display_name().to_owned())
        .collect();
    assert_eq!(offline_names, vec!["ben".to_owned()]);
}

#[test]
fn member_groups_treat_idle_and_dnd_as_online() {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![text_channel_info(guild_id, Id::new(2), "general")],
        members: vec![
            member_info(Id::new(10), "idle"),
            member_info(Id::new(11), "dnd"),
            member_info(Id::new(12), "unknown"),
        ],
        presences: vec![
            (Id::new(10), PresenceStatus::Idle),
            (Id::new(11), PresenceStatus::DoNotDisturb),
            // Unknown is treated as offline (Discord defaults to offline
            // when the gateway has not delivered a presence yet).
            (Id::new(12), PresenceStatus::Unknown),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Online");
    assert_eq!(groups[0].entries.len(), 2);
    assert_eq!(groups[1].label, "Offline");
    assert_eq!(groups[1].entries.len(), 1);
    assert_eq!(groups[1].entries[0].display_name(), "unknown");
}

#[test]
fn member_groups_show_selected_dm_recipient() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        recipients: Some(vec![ChannelRecipientInfo {
            status: Some(PresenceStatus::DoNotDisturb),
            ..ChannelRecipientInfo::test(Id::new(10), "alice")
        }]),
        ..dm_channel_info(channel_id, "alice")
    }));

    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Members");
    assert_eq!(groups[0].entries.len(), 1);
    assert_eq!(groups[0].entries[0].display_name(), "alice");
    assert_eq!(groups[0].entries[0].status(), PresenceStatus::DoNotDisturb);
}

#[test]
fn member_subscription_ranges_grow_with_viewport() {
    let mut state = state_with_thread_created_message();
    state.set_member_view_height(20);
    // Default scroll 0, viewport ends at 20 → bucket 0.
    assert_eq!(state.member_subscription_ranges(), vec![(0, 99)]);

    state.navigation.members.list.scroll = 100;
    state.navigation.members.list.view_height = 20;
    // Viewport ends at 120 → bucket 1, contiguous coverage.
    assert_eq!(
        state.member_subscription_ranges(),
        vec![(0, 99), (100, 199)]
    );

    state.navigation.members.list.scroll = 480;
    state.navigation.members.list.view_height = 30;
    // Viewport ends at 510 → bucket 5, anchor [0,99] plus the two buckets
    // around the visible end so we never exceed the four-range cap.
    assert_eq!(
        state.member_subscription_ranges(),
        vec![(0, 99), (400, 499), (500, 599)]
    );
}

#[test]
fn member_list_subscription_target_uses_active_channel_or_fallback() {
    let mut state = state_with_thread_created_message();
    // The fixture activates `general` (id=2) on guild=1.
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(2)))
    );

    // Switching the active channel to a thread must fall back to the
    // parent text channel because Discord rejects op-37 ranges against threads.
    state.activate_channel(Id::new(10));
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(2)))
    );
}

#[test]
fn member_list_subscription_fallback_skips_hidden_channels() {
    let state = state_with_hidden_and_visible_channels();

    assert_eq!(
        state.guild_member_list_channel(Id::new(1)),
        Some(Id::new(3))
    );
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(3)))
    );
}

#[test]
fn member_list_subscription_target_skips_active_voice_channel() {
    let mut state = state_with_hidden_and_visible_channels();
    state.activate_channel(Id::new(4));

    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(3)))
    );
}

#[test]
fn member_navigation_skips_over_activity_subrows() {
    let mut state = state_with_members(3);
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(20);

    state.push_event(AppEvent::PresenceUpdate {
        guild_id: Some(Id::new(1)),
        presence: crate::discord::PresenceEventFields {
            user_id: Id::new(2),
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo::test(ActivityKind::Playing, "Concord")],
        },
    });

    // Lines: 0 group header, 1 member 1, 2 member 2, 3 activity, 4 member 3.
    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);

    state.move_down();
    assert_eq!(state.selected_member(), 1);
    assert_eq!(state.selected_member_line(), 2);

    state.move_down();
    assert_eq!(state.selected_member(), 2);
    assert_eq!(state.selected_member_line(), 4);

    state.move_up();
    assert_eq!(state.selected_member(), 1);
    assert_eq!(state.selected_member_line(), 2);

    assert_eq!(state.member_line_count(), 5);
}

#[test]
fn member_half_page_scrolls_by_rendered_lines() {
    let mut state = state_with_grouped_members();
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(9);

    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);

    state.half_page_down();
    assert_eq!(state.selected_member(), 2);
    assert_eq!(state.selected_member_line(), 5);

    state.half_page_up();
    assert_eq!(state.selected_member(), 0);
    assert_eq!(state.selected_member_line(), 1);
}

#[test]
fn member_focused_selection_line_accounts_for_scroll_offset() {
    let mut state = state_with_members(8);
    state.focus_pane(FocusPane::Members);
    state.set_member_view_height(3);

    for _ in 0..4 {
        state.move_down();
    }
    assert_eq!(state.selected_member(), 4);
    assert_eq!(state.selected_member_line(), 5);

    let scroll = state.member_scroll();
    let relative = state.focused_member_selection_line().unwrap();
    assert_eq!(relative + scroll, state.selected_member_line());
}
