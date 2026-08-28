use super::*;
use crate::discord::test_builders::guild_create_event;

fn member_list_event(guild_id: Id<GuildMarker>, ops: Vec<GuildMemberListOperation>) -> AppEvent {
    AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: Some("everyone".to_owned()),
            member_count: None,
            online_count: None,
            groups: Vec::new(),
            ops,
            extra_fields: BTreeMap::new(),
        },
    }
}

#[test]
fn member_groups_follow_gateway_order_and_counts() {
    let guild_id = Id::new(1);
    let role_id = Id::new(100);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let carol = Id::new(30);
    let dave = Id::new(40);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        members: vec![
            member_info(alice, "alice"),
            member_info(bob, "bob"),
            member_info(carol, "carol"),
            member_info(dave, "dave"),
        ],
        roles: vec![RoleInfo {
            color: Some(0xFFAA00),
            ..RoleInfo::test(role_id, "Admin")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.push_event(guild_member_list_event(
        guild_id,
        vec![
            GuildMemberListItem::Group {
                id: role_id.get().to_string(),
                count: 8,
            },
            GuildMemberListItem::Member {
                member: member_info(bob, "bob"),
                presence: None,
            },
            GuildMemberListItem::Member {
                member: member_info(alice, "alice"),
                presence: None,
            },
            GuildMemberListItem::Group {
                id: "offline".to_owned(),
                count: 9,
            },
            GuildMemberListItem::Member {
                member: member_info(carol, "carol"),
                presence: None,
            },
        ],
    ));
    state.confirm_selected_guild();

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 2);
    assert_eq!((groups[0].label.as_str(), groups[0].count), ("Admin", 8));
    assert_eq!(groups[0].color, Some(0xFFAA00));
    assert_eq!(
        groups[0]
            .entries
            .iter()
            .map(|member| member.display_name())
            .collect::<Vec<_>>(),
        vec!["bob".to_owned(), "alice".to_owned()]
    );
    assert_eq!((groups[1].label.as_str(), groups[1].count), ("Offline", 9));

    state.push_event(member_list_event(
        guild_id,
        vec![GuildMemberListOperation::Sync {
            range: (200, 299),
            items: vec![GuildMemberListItem::Member {
                member: member_info(dave, "dave"),
                presence: None,
            }],
        }],
    ));

    let groups = state.members_grouped();
    assert_eq!(groups.len(), 3);
    assert_eq!((groups[2].label.as_str(), groups[2].count), ("Members", 1));
    assert_eq!(groups[2].entries[0].display_name(), "dave");
}

#[test]
fn member_role_color_picks_the_winning_coloured_role() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);

    for (name, roles, expected_color) in [
        (
            "highest position wins and colourless roles are skipped",
            vec![
                RoleInfo {
                    color: Some(0x112233),
                    position: 1,
                    ..RoleInfo::test(Id::new(100), "Low")
                },
                RoleInfo {
                    color: Some(0),
                    position: 99,
                    ..RoleInfo::test(Id::new(101), "Zero")
                },
                RoleInfo {
                    color: Some(0x445566),
                    position: 10,
                    ..RoleInfo::test(Id::new(102), "High")
                },
            ],
            0x445566,
        ),
        (
            "equal positions are broken by role id",
            vec![
                RoleInfo {
                    color: Some(0x112233),
                    position: 10,
                    ..RoleInfo::test(Id::new(200), "Newer")
                },
                RoleInfo {
                    color: Some(0x445566),
                    position: 10,
                    ..RoleInfo::test(Id::new(100), "Older")
                },
            ],
            0x445566,
        ),
    ] {
        let role_ids: Vec<_> = roles.iter().map(|role| role.id).collect();
        let mut state = DashboardState::new();

        state.push_event(guild_create_event(GuildCreateFixture {
            members: vec![member_with_roles(user_id, "alice", role_ids.clone())],
            presences: vec![PresenceEventFields {
                user_id,
                status: PresenceStatus::Online,
                activities: Vec::new(),
            }],
            roles,
            ..GuildCreateFixture::new(guild_id)
        }));
        state.push_event(guild_member_list_event(
            guild_id,
            vec![
                GuildMemberListItem::Group {
                    id: "online".to_owned(),
                    count: 1,
                },
                GuildMemberListItem::Member {
                    member: member_with_roles(user_id, "alice", role_ids),
                    presence: None,
                },
            ],
        ));
        state.confirm_selected_guild();

        let member = state.flattened_members()[0];

        assert_eq!(
            state.member_role_color(member),
            Some(expected_color),
            "{name}"
        );
    }
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
    duplicate.mentions = vec![crate::discord::MentionInfo::test(Id::new(98), "mentioned")];
    duplicate.reply = Some(ReplyInfo {
        author_id: Some(Id::new(97)),
        mentions: vec![crate::discord::MentionInfo::test(
            Id::new(96),
            "reply mention",
        )],
        ..ReplyInfo::test("reply author")
    });
    duplicate.interaction = Some(crate::discord::MessageInteractionInfo {
        user_id: Some(Id::new(94)),
        ..crate::discord::MessageInteractionInfo::test("interaction user")
    });
    duplicate.forwarded_snapshots = vec![MessageSnapshotInfo {
        source_channel_id: Some(channel_id),
        mentions: vec![crate::discord::MentionInfo::test(
            Id::new(95),
            "forwarded mention",
        )],
        ..MessageSnapshotInfo::test()
    }];
    let mut known_member = message_info(channel_id, 22);
    known_member.author_id = Id::new(10);
    known_member.author_role_ids = vec![Id::new(100)];
    let mut webhook = message_info(channel_id, 23);
    webhook.webhook_id = Some(Id::new(200));
    webhook.author_id = Id::new(93);
    webhook.mentions = vec![crate::discord::MentionInfo::test(
        Id::new(92),
        "webhook mention",
    )];

    assert_eq!(
        state.missing_message_author_member_requests(&[
            message.clone(),
            duplicate,
            known_member,
            webhook,
        ]),
        vec![(
            guild_id,
            vec![
                author_id,
                Id::new(94),
                Id::new(98),
                Id::new(97),
                Id::new(96),
                Id::new(95),
                Id::new(92),
            ]
        )]
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
fn member_groups_show_selected_dm_recipients() {
    let channel_id = Id::new(20);
    // Both DM kinds land in one flat "Members" group, ordered by status.
    let cases = [
        (
            "group-dm",
            vec![
                ChannelRecipientInfo {
                    status: Some(PresenceStatus::Idle),
                    ..ChannelRecipientInfo::test(Id::new(30), "bob")
                },
                ChannelRecipientInfo {
                    status: Some(PresenceStatus::Online),
                    ..ChannelRecipientInfo::test(Id::new(10), "alice")
                },
            ],
            vec![
                ("alice".to_owned(), PresenceStatus::Online),
                ("bob".to_owned(), PresenceStatus::Idle),
            ],
        ),
        (
            "dm",
            vec![ChannelRecipientInfo {
                status: Some(PresenceStatus::DoNotDisturb),
                ..ChannelRecipientInfo::test(Id::new(10), "alice")
            }],
            vec![("alice".to_owned(), PresenceStatus::DoNotDisturb)],
        ),
    ];

    for (kind, recipients, expected) in cases {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            kind: kind.to_owned(),
            recipients: Some(recipients),
            ..dm_channel_info(channel_id, "project chat")
        }));
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1, "{kind}");
        assert_eq!(groups[0].label, "Members", "{kind}");
        assert_eq!(
            groups[0]
                .entries
                .iter()
                .map(|member| (member.display_name(), member.status()))
                .collect::<Vec<_>>(),
            expected,
            "{kind}"
        );
    }
}

#[test]
fn member_panel_title_shows_online_and_total_when_counts_available() {
    let guild_id = Id::new(1);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        member_count: Some(100),
        members: vec![member_info(Id::new(10), "alice")],
        presences: vec![PresenceEventFields {
            user_id: Id::new(10),
            status: PresenceStatus::Online,
            activities: Vec::new(),
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    state.push_event(guild_member_list_counts_event(guild_id, 25));

    let title = state.member_panel_title();
    let rendered: String = title.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(rendered, "● 25  ○ 100");
}

#[test]
fn member_list_loading_tracks_subscription_range_completeness() {
    let guild_id = Id::new(1);
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![text_channel_info(guild_id, Id::new(2), "general")],
        member_count: Some(1),
        members: vec![member_info(user_id, "alice")],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();

    assert!(state.is_member_list_loading());
    state.push_event(member_list_event(
        guild_id,
        vec![GuildMemberListOperation::Sync {
            range: (0, 99),
            items: vec![GuildMemberListItem::Member {
                member: member_info(user_id, "alice"),
                presence: None,
            }],
        }],
    ));
    assert!(!state.is_member_list_loading());
    assert_eq!(state.flattened_members().len(), 1);

    state.push_event(member_list_event(
        guild_id,
        vec![GuildMemberListOperation::Invalidate { range: (0, 99) }],
    ));
    assert!(state.is_member_list_loading());

    let voice_channel_id = Id::new(3);
    let mut voice_only = DashboardState::new();
    voice_only.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![voice_channel_info(guild_id, voice_channel_id, "Lobby")],
        members: vec![member_info(user_id, "alice")],
        ..GuildCreateFixture::new(guild_id)
    }));
    voice_only.confirm_selected_guild();
    assert_eq!(voice_only.member_list_subscription_target(), None);
    assert!(!voice_only.is_member_list_loading());
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
fn member_list_subscription_fallback_skips_hidden_and_voice_channels() {
    // Discord rejects op-37 ranges against channels the user cannot read, and
    // a voice channel carries no member list, so both have to be passed over
    // in favour of the first readable text channel.
    let state = state_with_hidden_and_visible_channels();
    assert_eq!(
        state.guild_member_list_channel(Id::new(1)),
        Some(Id::new(3))
    );
    assert_eq!(
        state.member_list_subscription_target(),
        Some((Id::new(1), Id::new(3)))
    );

    let mut voice_active = state_with_hidden_and_visible_channels();
    voice_active.activate_channel(Id::new(4));
    assert_eq!(
        voice_active.member_list_subscription_target(),
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
