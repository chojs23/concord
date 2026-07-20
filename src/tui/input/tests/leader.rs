use super::*;
use crate::discord::VoiceScope;
use std::collections::BTreeMap;

#[test]
fn enter_toggles_selected_channel_category_and_space_opens_leader() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    assert_selected_channel_category_collapsed(&state, true);
}

#[test]
fn keymap_leader_e_starts_composer() {
    let mut mappings = BTreeMap::new();
    mappings.insert("StartComposer".to_owned(), KeymapBinding::one("<leader>e"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    assert!(
        state
            .leader_keymap_shortcuts()
            .iter()
            .any(|item| item.key == "e" && item.label == "start composer")
    );
    handle_key(&mut state, char_key('e'));

    assert!(!state.is_leader_active());
    assert!(state.is_composing());
}

#[test]
fn keymap_nested_leader_m_r_replies_to_message() {
    let mut mappings = BTreeMap::new();
    mappings.insert("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    let root_shortcuts = state.leader_keymap_shortcuts();
    assert!(
        root_shortcuts
            .iter()
            .any(|item| item.key == "m" && item.has_children)
    );

    handle_key(&mut state, char_key('m'));
    let nested_shortcuts = state.leader_keymap_shortcuts();
    assert_eq!(nested_shortcuts[0].key, "r");
    assert_eq!(nested_shortcuts[0].label, "reply");

    handle_key(&mut state, char_key('r'));
    assert!(!state.is_leader_active());
    assert!(state.is_composing());

    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('k'));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "ok".to_owned(),
            reply_to: Some(crate::discord::ReplyReference {
                message_id: Id::new(1),
                mention_author: true,
            }),
            attachments: Vec::new(),
        })
    );
}

#[test]
fn keymap_nested_unknown_key_closes_without_root_fallback() {
    let mut mappings = BTreeMap::new();
    mappings.insert("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('m'));
    handle_key(&mut state, char_key('o'));

    assert!(!state.is_leader_active());
    assert!(!state.is_options_category_picker_open());
}

#[test]
fn keymap_compact_root_prefix_opens_popup_then_executes() {
    let mut mappings = BTreeMap::new();
    mappings.insert("VoiceDeafen".to_owned(), KeymapBinding::one("fd"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, char_key('f'));

    assert!(state.is_leader_active());
    assert_eq!(state.leader_keymap_title(), "f");
    assert_eq!(state.leader_keymap_shortcuts()[0].key, "d");

    handle_key(&mut state, char_key('d'));

    assert!(!state.is_leader_active());
    assert!(state.voice_options().self_deaf);
}

#[test]
fn keymap_configured_d_prefix_overrides_message_delete_default() {
    let mut mappings = BTreeMap::new();
    mappings.insert("VoiceDeafen".to_owned(), KeymapBinding::one("dd"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Messages);

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(command, None);
    assert!(state.is_leader_active());
    assert_eq!(state.leader_keymap_title(), "d");
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    handle_key(&mut state, char_key('d'));

    assert!(!state.is_leader_active());
    assert!(state.voice_options().self_deaf);
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn scoped_channel_action_keys_work_as_aliases() {
    for shortcut in ['x', 'u'] {
        let mut channel_actions = BTreeMap::new();
        channel_actions.insert(
            "ToggleMute".to_owned(),
            KeymapBinding {
                keys: vec!["x".to_owned(), "u".to_owned()],
                description: None,
            },
        );
        let mut state = state_with_keymap(KeymapOptions {
            leader: None,
            groups: BTreeMap::new(),
            channel_actions,
            ..Default::default()
        });
        state = state_with_messages_from_state(state, 1);
        state.focus_pane(FocusPane::Channels);

        handle_key(&mut state, char_key(' '));
        handle_key(&mut state, char_key('a'));
        handle_key(&mut state, char_key(shortcut));

        assert!(state.is_channel_action_mute_duration_phase());
    }
}

#[test]
fn scoped_channel_action_modified_shortcut_requires_matching_modifier() {
    let mut channel_actions = BTreeMap::new();
    channel_actions.insert("ToggleMute".to_owned(), KeymapBinding::one("<C-u>"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        channel_actions,
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));

    assert!(!state.is_channel_action_mute_duration_phase());
    assert!(state.is_channel_action_menu_active());

    handle_key(&mut state, ctrl_key('u'));

    assert!(state.is_channel_action_mute_duration_phase());
}

#[test]
fn keymap_can_execute_leader_and_options_actions() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "ChannelSwitcher".to_owned(),
        KeymapBinding::one("<leader><space>"),
    );
    mappings.insert(
        "OpenVoiceOptions".to_owned(),
        KeymapBinding::one("<leader>o v"),
    );
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));

    state.close_channel_switcher();
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));
    assert_eq!(state.options_popup_title(), "Voice Options");
}

#[test]
fn keymap_leader_n_opens_notification_inbox_and_switches_tabs() {
    use crate::tui::state::{ActiveModalPopupKind, NotificationInboxTab};

    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    assert!(
        state
            .leader_keymap_shortcuts()
            .iter()
            .any(|item| item.key == "n" && item.label == "Notification inbox")
    );
    handle_key(&mut state, char_key('n'));
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::NotificationInbox));
    assert_eq!(
        state.notification_inbox_tab(),
        Some(NotificationInboxTab::Unreads)
    );

    // Opening fetches mentions. Unreads are derived locally.
    let commands = state.drain_pending_commands();
    assert!(commands.iter().any(|command| matches!(
        command,
        crate::discord::AppCommand::LoadInboxMentions { .. }
    )));

    handle_key(&mut state, key(KeyCode::Right));
    assert_eq!(
        state.notification_inbox_tab(),
        Some(NotificationInboxTab::Mentions)
    );
    handle_key(&mut state, key(KeyCode::Left));
    assert_eq!(
        state.notification_inbox_tab(),
        Some(NotificationInboxTab::Unreads)
    );
    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::NotificationInbox));
}

#[test]
fn notification_inbox_mentions_snapshot_respects_request_id() {
    use crate::discord::{AppCommand, AppEvent};
    use crate::tui::state::NotificationInboxLoad;

    let mut state = state_with_channel_tree();
    state.open_notification_inbox();

    // The mentions fetch carries the current open's request id.
    let request_id = state
        .drain_pending_commands()
        .into_iter()
        .find_map(|command| match command {
            AppCommand::LoadInboxMentions { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("mentions request is enqueued on open");
    assert_eq!(
        state.notification_inbox_mentions_status(),
        Some(NotificationInboxLoad::Loading)
    );

    // A stale response from a different open is ignored.
    state.push_event(AppEvent::InboxMentionsLoaded {
        request_id: request_id.wrapping_add(1),
        before: None,
        messages: Vec::new(),
        has_more: false,
    });
    assert_eq!(
        state.notification_inbox_mentions_status(),
        Some(NotificationInboxLoad::Loading)
    );

    // The matching response settles the tab.
    state.push_event(AppEvent::InboxMentionsLoaded {
        request_id,
        before: None,
        messages: Vec::new(),
        has_more: false,
    });
    assert_eq!(
        state.notification_inbox_mentions_status(),
        Some(NotificationInboxLoad::Loaded)
    );
}

#[test]
fn notification_inbox_refreshes_the_message_author_role_color_after_member_load() {
    use crate::discord::{
        AppCommand, AppEvent, ChannelInfo, GuildMembersChunkInfo, MemberInfo, MessageInfo,
        RoleInfo,
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
        },
        test_builders::{GuildCreateFixture, guild_create_event},
    };
    use crate::tui::state::NotificationInboxItem;

    let guild_id = Id::<GuildMarker>::new(1);
    let channel_id = Id::<ChannelMarker>::new(2);
    let role_id = Id::<RoleMarker>::new(90);
    let role_color = 0x12_34_56;
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "text")
        }],
        roles: vec![RoleInfo {
            color: Some(role_color),
            position: 10,
            ..RoleInfo::test(role_id, "colored")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.open_notification_inbox();
    let request_id = state
        .drain_pending_commands()
        .into_iter()
        .find_map(|command| match command {
            AppCommand::LoadInboxMentions { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("mentions request is enqueued on open");
    state.push_event(AppEvent::InboxMentionsLoaded {
        request_id,
        before: None,
        messages: vec![MessageInfo {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::<MessageMarker>::new(700),
            author_id: Id::<UserMarker>::new(42),
            author: "alice".to_owned(),
            author_role_ids: Vec::new(),
            content: Some("hello".to_owned()),
            ..MessageInfo::default()
        }],
        has_more: false,
    });
    state.push_event(AppEvent::GuildMembersChunk {
        chunk: GuildMembersChunkInfo {
            guild_id,
            members: vec![MemberInfo {
                role_ids: vec![role_id],
                ..MemberInfo::test(Id::new(42), "alice")
            }],
            presences: Vec::new(),
            chunk_index: Some(0),
            chunk_count: Some(1),
            nonce: None,
            not_found: Vec::new(),
            extra_fields: BTreeMap::new(),
        },
    });
    handle_key(&mut state, key(KeyCode::Right));

    let color = state
        .notification_inbox_items()
        .into_iter()
        .find_map(|item| match item {
            NotificationInboxItem::Mention(item) => item.author_role_color,
            NotificationInboxItem::Unread(_) => None,
        });
    assert_eq!(color, Some(role_color));
}

#[test]
fn notification_inbox_mention_read_uses_the_recent_mention_endpoint_command() {
    use crate::discord::{
        AppCommand, AppEvent, MessageInfo,
        ids::{
            Id,
            marker::{ChannelMarker, MessageMarker},
        },
    };

    let mut notification_inbox_actions = BTreeMap::new();
    notification_inbox_actions.insert("MarkRead".to_owned(), KeymapBinding::one("q"));
    let mut state = state_with_keymap(KeymapOptions {
        notification_inbox_actions,
        ..Default::default()
    });
    state.open_notification_inbox();
    let request_id = state
        .drain_pending_commands()
        .into_iter()
        .find_map(|command| match command {
            AppCommand::LoadInboxMentions { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("mentions request is enqueued on open");
    let message_id = Id::<MessageMarker>::new(700);
    state.push_event(AppEvent::InboxMentionsLoaded {
        request_id,
        before: None,
        messages: vec![MessageInfo {
            channel_id: Id::<ChannelMarker>::new(2),
            message_id,
            author: "alice".to_owned(),
            content: Some("@neo hello".to_owned()),
            ..MessageInfo::default()
        }],
        has_more: false,
    });
    assert_eq!(state.notification_inbox_mention_count(), 1);

    handle_key(&mut state, key(KeyCode::Right));
    assert_eq!(handle_key(&mut state, char_key('r')), None);
    assert_eq!(
        handle_key(&mut state, char_key('q')),
        Some(AppCommand::DeleteInboxMention { message_id })
    );
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::NotificationInbox)
    );
    state.push_event(AppEvent::InboxRecentMentionDeleted { message_id });
    assert!(state.notification_inbox_items().is_empty());
    assert_eq!(state.notification_inbox_mention_count(), 0);
}

#[test]
fn notification_inbox_keeps_loading_after_visible_mentions_are_deleted() {
    use crate::discord::{
        AppCommand, AppEvent, MessageInfo,
        ids::{
            Id,
            marker::{ChannelMarker, MessageMarker},
        },
    };

    let mut state = state_with_channel_tree();
    state.open_notification_inbox();
    let request_id = state
        .drain_pending_commands()
        .into_iter()
        .find_map(|command| match command {
            AppCommand::LoadInboxMentions { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("mentions request is enqueued on open");
    let messages = (701..=725)
        .rev()
        .map(|message_id| MessageInfo {
            channel_id: Id::<ChannelMarker>::new(2),
            message_id: Id::<MessageMarker>::new(message_id),
            author: "alice".to_owned(),
            content: Some("@neo hello".to_owned()),
            ..MessageInfo::default()
        })
        .collect::<Vec<_>>();
    let next_before = messages
        .last()
        .map(|message| message.message_id)
        .expect("mentions page is not empty");
    state.push_event(AppEvent::InboxMentionsLoaded {
        request_id,
        before: None,
        messages: messages.clone(),
        has_more: true,
    });
    handle_key(&mut state, key(KeyCode::Right));
    assert!(state.drain_pending_commands().is_empty());

    for message in messages {
        state.push_event(AppEvent::InboxRecentMentionDeleted {
            message_id: message.message_id,
        });
    }

    assert!(state.drain_pending_commands().iter().any(|command| {
        matches!(
            command,
            AppCommand::LoadInboxMentions {
                request_id: command_request_id,
                before: Some(before),
            } if *command_request_id == request_id && *before == next_before
        )
    }));
}

#[test]
fn notification_inbox_includes_unreads_from_collapsed_guild_folders() {
    use crate::discord::{
        AppEvent, ChannelInfo, GuildFolder, ReadStateInfo, UserSettingsInfo,
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, MessageMarker},
        },
        test_builders::{GuildCreateFixture, guild_create_event},
    };
    use crate::tui::state::NotificationInboxItem;

    let first_guild = Id::<GuildMarker>::new(1);
    let second_guild = Id::<GuildMarker>::new(2);
    let first_channel = Id::<ChannelMarker>::new(101);
    let second_channel = Id::<ChannelMarker>::new(201);
    let mut state = DashboardState::new();
    for (guild_id, channel_id, name) in [
        (first_guild, first_channel, "first"),
        (second_guild, second_channel, "second"),
    ] {
        state.push_event(guild_create_event(GuildCreateFixture {
            name: name.to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                last_message_id: Some(Id::<MessageMarker>::new(2)),
                ..ChannelInfo::test(channel_id, "text")
            }],
            ..GuildCreateFixture::new(guild_id)
        }));
    }
    state.push_event(AppEvent::ReadStateInit {
        entries: [first_channel, second_channel]
            .into_iter()
            .map(|channel_id| ReadStateInfo {
                last_acked_message_id: Some(Id::<MessageMarker>::new(1)),
                ..ReadStateInfo::test(channel_id)
            })
            .collect(),
    });
    state.push_event(AppEvent::UserSettingsUpdate {
        settings: UserSettingsInfo {
            guild_folders: Some(vec![GuildFolder {
                id: Some(42),
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }]),
            ..UserSettingsInfo::default()
        },
    });
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_folder_collapsed(&state, true);

    state.open_notification_inbox();

    let unread_channels = state
        .notification_inbox_items()
        .into_iter()
        .filter_map(|item| match item {
            NotificationInboxItem::Unread(item) => Some(item.channel_id),
            NotificationInboxItem::Mention(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(unread_channels, vec![first_channel, second_channel]);
}

#[test]
fn keymap_leader_ctrl_w_opens_channel_switcher() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "ChannelSwitcher".to_owned(),
        KeymapBinding::one("<leader><C-w>"),
    );
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, char_key(' '));
    assert!(
        state
            .leader_keymap_shortcuts()
            .iter()
            .any(|item| item.key == "Ctrl+w" && item.label == "Switch channels")
    );
    handle_key(&mut state, ctrl_key('w'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
}

#[test]
fn keymap_direct_ctrl_w_opens_channel_switcher_and_replaces_leader_default() {
    let mut mappings = BTreeMap::new();
    mappings.insert("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, ctrl_key('w'));
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));

    state.close_channel_switcher();
    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    handle_key(&mut state, char_key(' '));
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
}

#[test]
fn keymap_non_leader_prefix_opens_which_key_then_executes() {
    let mut mappings = BTreeMap::new();
    mappings.insert("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>f"));
    mappings.insert("OpenOptions".to_owned(), KeymapBinding::one("<C-w>q"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, ctrl_key('w'));
    assert!(state.is_leader_active());
    assert_eq!(state.leader_keymap_title(), "<C-w>");
    let shortcuts = state.leader_keymap_shortcuts();
    assert!(
        shortcuts
            .iter()
            .any(|item| item.key == "f" && item.label == "Switch channels")
    );
    assert!(
        shortcuts
            .iter()
            .any(|item| item.key == "q" && item.label == "Options")
    );

    handle_key(&mut state, char_key('f'));
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
}

#[test]
fn keymap_non_leader_nested_prefix_title_tracks_sequence() {
    let mut mappings = BTreeMap::new();
    mappings.insert("OpenOptions".to_owned(), KeymapBinding::one("<C-e>qe"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, ctrl_key('e'));
    assert_eq!(state.leader_keymap_title(), "<C-e>");
    assert_eq!(state.leader_keymap_shortcuts()[0].key, "q");

    handle_key(&mut state, char_key('q'));
    assert_eq!(state.leader_keymap_title(), "<C-e>q");
    assert_eq!(state.leader_keymap_shortcuts()[0].key, "e");

    handle_key(&mut state, char_key('e'));
    assert_eq!(state.options_popup_title(), "Options");
}

#[test]
fn keymap_description_overrides_which_key_label() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "ChannelSwitcher".to_owned(),
        KeymapBinding {
            keys: vec!["<C-w>f".to_owned()],
            description: Some("find channel".to_owned()),
        },
    );
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, ctrl_key('w'));
    assert!(
        state
            .leader_keymap_shortcuts()
            .iter()
            .any(|item| item.key == "f" && item.label == "find channel")
    );
}

#[test]
fn keymap_direct_open_pane_filter_replaces_slash_default() {
    let mut mappings = BTreeMap::new();
    mappings.insert("OpenPaneFilter".to_owned(), KeymapBinding::one("<C-f>"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key('/'));
    assert_eq!(state.guild_pane_filter_query(), None);

    handle_key(&mut state, ctrl_key('f'));
    assert_eq!(state.guild_pane_filter_query(), Some(""));
}

#[test]
fn custom_leader_replaces_space_leader_key() {
    let mut state = state_with_keymap(KeymapOptions {
        leader: Some("<C-k>".to_owned()),
        groups: BTreeMap::new(),
        mappings: BTreeMap::new(),
        ..Default::default()
    });

    handle_key(&mut state, char_key(' '));
    assert!(!state.is_leader_active());

    handle_key(&mut state, ctrl_key('k'));
    assert!(state.is_leader_active());
}

#[test]
fn keymap_executes_canonical_pane_and_voice_commands() {
    let mut mappings = BTreeMap::new();
    mappings.insert(
        "ToggleGuildPane".to_owned(),
        KeymapBinding::one("<leader>1"),
    );
    mappings.insert("VoiceMute".to_owned(), KeymapBinding::one("<leader>v m"));
    mappings.insert("VoiceLeave".to_owned(), KeymapBinding::one("<leader>v l"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });
    state.push_effect(voice_connection_status_changed_event(
        VoiceConnectionStatusChangedFixture {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Some(Id::new(11)),
            status: VoiceConnectionStatus::Connecting,
            ..VoiceConnectionStatusChangedFixture::new()
        },
    ));

    assert!(state.is_pane_visible(FocusPane::Guilds));
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('1'));
    assert!(!state.is_pane_visible(FocusPane::Guilds));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('v'));
    handle_key(&mut state, char_key('m'));
    assert!(state.voice_options().self_mute);
    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(11),
            self_mute: true,
            self_deaf: false,
        }]
    );

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('v'));
    let command = handle_key(&mut state, char_key('l'));
    assert_eq!(
        command,
        Some(AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: true,
            self_deaf: false,
        })
    );
}

#[test]
fn configured_direct_keymap_can_override_dashboard_shortcut() {
    let mut mappings = BTreeMap::new();
    mappings.insert("ChannelSwitcher".to_owned(), KeymapBinding::one("q"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, char_key('q'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert!(!state.should_quit());
}

#[test]
fn configured_quit_key_replaces_default_q() {
    let mut mappings = BTreeMap::new();
    mappings.insert("Quit".to_owned(), KeymapBinding::one("x"));
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: BTreeMap::new(),
        mappings,
        ..Default::default()
    });

    handle_key(&mut state, char_key('q'));
    assert!(!state.should_quit());
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::QuitConfirmation)
    );

    handle_key(&mut state, char_key('x'));
    assert!(!state.should_quit());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::QuitConfirmation));

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.should_quit());
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::QuitConfirmation)
    );

    handle_key(&mut state, char_key('x'));
    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.should_quit());
}

#[test]
fn leader_channel_actions_offer_mute_duration_and_submit_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    let command = handle_key(&mut state, char_key('1'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "#general".to_owned(),
        })
    );
}

#[test]
fn leader_channel_actions_unmute_when_already_muted() {
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::UserGuildSettingsInit {
        settings: vec![UserGuildSettingsInfo {
            notification_settings: GuildNotificationSettingsInfo {
                message_notifications: Some(NotificationLevel::OnlyMentions),
                channel_overrides: vec![ChannelNotificationOverrideInfo {
                    muted: true,
                    ..ChannelNotificationOverrideInfo::test(Id::new(11))
                }],
                ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
            },
            extra_fields: BTreeMap::new(),
        }],
    });
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    let command = handle_key(&mut state, char_key('u'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(11),
            muted: false,
            duration: None,
            label: "#general".to_owned(),
        })
    );
}

#[test]
fn leader_category_actions_offer_mute_duration_and_submit_command() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Up));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    let command = handle_key(&mut state, char_key('1'));

    assert_eq!(
        command,
        Some(AppCommand::SetChannelMuted {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(10),
            muted: true,
            duration: Some(crate::discord::MuteDuration::Minutes(15)),
            label: "Text Channels".to_owned(),
        })
    );
}

#[test]
fn leader_server_actions_unmute_when_already_muted() {
    let mut state = state_with_channel_tree();
    state.push_event(AppEvent::UserGuildSettingsInit {
        settings: vec![UserGuildSettingsInfo {
            notification_settings: GuildNotificationSettingsInfo {
                message_notifications: Some(NotificationLevel::OnlyMentions),
                muted: true,
                ..GuildNotificationSettingsInfo::test(Some(Id::new(1)))
            },
            extra_fields: BTreeMap::new(),
        }],
    });
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    let command = handle_key(&mut state, char_key('u'));

    assert_eq!(
        command,
        Some(AppCommand::SetGuildMuted {
            guild_id: Id::new(1),
            muted: false,
            duration: None,
            label: "guild".to_owned(),
        })
    );
}

#[test]
fn leader_o_opens_options_category_picker() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));

    assert!(!state.is_leader_active());
    assert!(state.is_options_category_picker_open());
    assert_eq!(state.options_popup_title(), "Options");
    assert_eq!(state.display_option_items()[0].label, "Display");
    assert_eq!(state.display_option_items()[1].label, "Composer");
    assert_eq!(state.display_option_items()[2].label, "Notifications");
    assert_eq!(state.display_option_items()[3].label, "Voice");
}

#[test]
fn leader_v_opens_voice_keymap_group() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('v'));

    assert!(state.is_leader_active());
    assert!(
        state
            .leader_keymap_shortcuts()
            .iter()
            .any(|item| item.key == "m" && item.label == "mute voice")
    );
}

#[test]
fn leader_server_actions_leave_opens_confirmation_then_leaves() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('l'));

    assert!(!state.is_leader_active());
    assert!(
        state
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::GuildLeaveConfirmation)
    );

    let command = handle_key(&mut state, char_key('y'));

    assert_eq!(
        command,
        Some(AppCommand::LeaveGuild {
            guild_id: Id::new(1),
            label: "guild".to_owned(),
        })
    );
}

#[test]
fn leader_o_category_shortcuts_open_scoped_options() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('d'));

    assert_eq!(state.options_popup_title(), "Display Options");
    assert_eq!(
        state.display_option_items()[0].label,
        "Disable all image previews"
    );

    state.close_options_popup();
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('n'));

    assert_eq!(state.options_popup_title(), "Notification Options");
    assert_eq!(
        state.display_option_items()[0].label,
        "Desktop notifications"
    );

    state.close_options_popup();
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));

    assert_eq!(state.options_popup_title(), "Voice Options");
    assert_eq!(state.display_option_items()[0].label, "Voice muted");
}

#[test]
fn leader_number_keys_toggle_side_panes() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());

    handle_key(&mut state, char_key('1'));
    assert!(!state.is_leader_active());
    assert!(!state.is_pane_visible(FocusPane::Guilds));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('2'));
    assert!(!state.is_pane_visible(FocusPane::Channels));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('4'));
    assert!(!state.is_pane_visible(FocusPane::Members));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('1'));
    assert!(state.is_pane_visible(FocusPane::Guilds));
}

#[test]
fn leader_esc_and_unknown_key_cancel_without_toggling_panes() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_leader_active());
    assert!(state.is_pane_visible(FocusPane::Guilds));

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('x'));
    assert!(!state.is_leader_active());
    assert!(state.is_pane_visible(FocusPane::Channels));
}

#[test]
fn leader_leader_switcher_filters_and_opens_selected_channel() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    assert!(!state.is_leader_active());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));

    for ch in "rand".chars() {
        handle_key(&mut state, char_key(ch));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn leader_leader_switcher_expands_collapsed_parent_category() {
    let mut state = state_with_channel_tree();
    state.toggle_selected_channel_category();
    assert_selected_channel_category_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    for ch in "rand".chars() {
        handle_key(&mut state, char_key(ch));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_selected_channel_category_collapsed(&state, false);
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert!(matches!(
        state.channel_pane_entries().get(state.selected_channel()),
        Some(ChannelPaneEntry::Channel { state, .. }) if state.id == Id::new(12)
    ));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn leader_leader_switcher_opens_direct_message() {
    let mut state = state_with_direct_message("dm");

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeDirectMessage {
            channel_id: Id::new(20),
        })
    );
}

#[test]
fn leader_leader_switcher_j_and_k_type_into_search() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('j'));
    handle_key(&mut state, char_key('k'));

    assert_eq!(state.channel_switcher_query(), Some("jk"));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn leader_leader_switcher_selection_aliases_move_selection() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn leader_leader_switcher_left_right_move_search_cursor() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
    for ch in "raXndom".chars() {
        handle_key(&mut state, char_key(ch));
    }
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Left));
    }
    handle_key(&mut state, key(KeyCode::Right));
    handle_key(&mut state, key(KeyCode::Backspace));

    assert_eq!(state.channel_switcher_query(), Some("random"));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn mouse_input_closes_leader_hint() {
    let mut state = DashboardState::new();
    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());

    handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 1),
        dashboard_area(),
    );

    assert!(!state.is_leader_active());
}

#[test]
fn enter_opens_message_action_menu_and_space_opens_leader() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.is_message_action_menu_active());
    state.close_message_action_menu();

    handle_key(&mut state, char_key(' '));

    assert!(state.is_leader_active());
    assert!(!state.is_message_action_menu_active());
}

#[test]
fn leader_a_p_enters_pinned_message_view_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(command, None);
    assert!(state.is_pinned_message_view());
    assert!(!state.is_leader_active());
}

#[test]
fn leader_a_opens_selected_channel_actions_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_channel_action_menu_active());
}

#[test]
fn leader_channel_subphase_esc_returns_to_channel_actions() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    assert!(state.is_channel_action_mute_duration_phase());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_channel_action_menu_active());
    assert!(!state.is_channel_action_mute_duration_phase());
}

#[test]
fn leader_a_t_opens_channel_thread_list_view() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('t'));

    assert_eq!(command, None);
    assert!(!state.is_leader_active());
    assert!(state.is_channel_thread_list_view());
}

#[test]
fn leader_guild_subphase_esc_returns_to_server_actions() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('u'));
    assert!(state.is_guild_action_mute_duration_phase());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_guild_action_menu_active());
    assert!(!state.is_guild_action_mute_duration_phase());
}

#[test]
fn leader_a_opens_message_actions_from_message_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_message_action_menu_active());
    assert!(!state.is_channel_action_menu_active());
}

#[test]
fn leader_a_opens_server_actions_from_guild_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_guild_action_menu_active());
}

#[test]
fn folder_settings_edits_name_and_color() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('r'));
    assert!(state.is_folder_settings_open());
    assert!(state.folder_settings_name_active());

    handle_key(&mut state, key(KeyCode::BackTab));
    assert!(state.folder_settings_cancel_active());
    handle_key(&mut state, key(KeyCode::Tab));
    assert!(state.folder_settings_name_active());

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, ctrl_key('w'));
    for ch in "work".chars() {
        handle_key(&mut state, char_key(ch));
    }
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));

    handle_key(&mut state, key(KeyCode::Enter));
    for ch in "#00AAFF".chars() {
        handle_key(&mut state, char_key(ch));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::UpdateGuildFolderSettings {
            folder_id: 42,
            name: Some("work".to_owned()),
            color: Some(0x00aaff),
        })
    );
    assert!(!state.is_folder_settings_open());
}

#[test]
fn folder_settings_keeps_overlay_open_for_invalid_color() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('r'));
    handle_key(&mut state, ctrl_key('n'));
    handle_key(&mut state, char_key('b'));
    assert_eq!(state.folder_settings_color_value(), Some(""));

    handle_key(&mut state, key(KeyCode::Enter));
    for ch in "blue".chars() {
        handle_key(&mut state, char_key(ch));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(state.is_folder_settings_open());
    assert_eq!(
        state.folder_settings_color_error(),
        Some("Use #RRGGBB or leave blank")
    );
}

#[test]
fn folder_settings_esc_cancels_current_field_edit() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('r'));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, ctrl_key('w'));
    for ch in "draft".chars() {
        handle_key(&mut state, char_key(ch));
    }
    assert_eq!(state.folder_settings_name_value(), Some("draft"));

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_folder_settings_open());
    assert!(!state.is_folder_settings_editing());
    assert_eq!(state.folder_settings_name_value(), Some("folder"));

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::UpdateGuildFolderSettings {
            folder_id: 42,
            name: Some("folder".to_owned()),
            color: None,
        })
    );
}

#[test]
fn leader_a_opens_member_actions_from_member_pane() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_member_action_menu_active());
    let actions = state.selected_member_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].label, "Show profile");
    assert!(actions[0].is_enabled());
}

#[test]
fn leader_a_p_opens_member_profile() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(
        command,
        Some(AppCommand::LoadUserProfile {
            user_id: Id::new(1),
            guild_id: Some(Id::new(1)),
        })
    );
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));
    assert!(!state.is_leader_active());
}
