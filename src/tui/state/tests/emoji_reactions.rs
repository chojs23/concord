use super::*;

#[test]
fn emoji_picker_items_include_available_custom_emojis_for_selected_message_guild() {
    let state = state_with_custom_emojis();

    let items = state.emoji_reaction_items();

    assert!(items.len() > 9);
    assert_eq!(
        items[..8]
            .iter()
            .map(|item| item.emoji.clone())
            .collect::<Vec<_>>(),
        vec![
            ReactionEmoji::Unicode("👍".to_owned()),
            ReactionEmoji::Unicode("❤️".to_owned()),
            ReactionEmoji::Unicode("😂".to_owned()),
            ReactionEmoji::Unicode("🎉".to_owned()),
            ReactionEmoji::Unicode("😮".to_owned()),
            ReactionEmoji::Unicode("😢".to_owned()),
            ReactionEmoji::Unicode("🙏".to_owned()),
            ReactionEmoji::Unicode("👀".to_owned()),
        ]
    );
    assert_eq!(items[0].label, "Thumbs Up");
    assert_eq!(items[8].label, "Party Time");
    assert_eq!(
        items[8].emoji,
        ReactionEmoji::Custom {
            id: Id::new(50),
            name: Some("party_time".to_owned()),
            animated: true,
        }
    );
    assert!(matches!(items[9].emoji, ReactionEmoji::Unicode(_)));
}

#[test]
fn custom_emoji_reaction_items_expose_cdn_image_url() {
    let state = state_with_custom_emojis();

    let items = state.emoji_reaction_items();

    assert_eq!(
        items[8].custom_image_url().as_deref(),
        Some("https://cdn.discordapp.com/emojis/50.gif")
    );
    assert_eq!(items[0].custom_image_url(), None);
}

#[test]
fn emoji_picker_items_include_custom_emojis_from_update_event() {
    let guild_id = Id::new(1);
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id,
        emojis: vec![CustomEmojiInfo {
            id: Id::new(60),
            name: "wave".to_owned(),
            animated: false,
            available: true,
        }],
    });

    let items = state.emoji_reaction_items();

    assert!(items.len() > 9);
    assert_eq!(items[8].label, "Wave");
    assert_eq!(
        items[8].emoji,
        ReactionEmoji::Custom {
            id: Id::new(60),
            name: Some("wave".to_owned()),
            animated: false,
        }
    );
}

#[test]
fn emoji_picker_uses_channel_guild_when_selected_message_lacks_guild_id() {
    let mut state = state_with_custom_emojis();

    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some("history message without guild".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let items = state.emoji_reaction_items();

    assert!(items.len() > 9);
    assert_eq!(items[8].label, "Party Time");
}

#[test]
fn emoji_picker_items_stay_unicode_only_for_direct_messages() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "neo".to_owned(),
        kind: "dm".to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: None,
        permission_overwrites: Vec::new(),
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: None,
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        author_role_ids: Vec::new(),
        message_kind: MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        content: Some("hello".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });

    let items = state.emoji_reaction_items();
    assert!(items.len() > 8);
    assert!(
        items
            .iter()
            .all(|item| matches!(item.emoji, ReactionEmoji::Unicode(_)))
    );
}

#[test]
fn reaction_message_actions_use_single_reacted_users_item() {
    let mut state = state_with_reaction_message();
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::Reply,
            MessageActionKind::AddReaction,
            MessageActionKind::ShowProfile,
            MessageActionKind::SetPinned(true),
            MessageActionKind::ShowReactionUsers,
            MessageActionKind::RemoveReaction(0),
        ]
    );
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.label == "Show reacted users")
            .count(),
        1
    );
    assert!(!actions.iter().any(|action| action.label == "Show 👍 users"));
}

#[test]
fn add_reaction_action_requires_history_and_existing_or_add_reactions_permission() {
    let mut without_add = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        Vec::new(),
    );
    without_add.focus_pane(FocusPane::Messages);

    assert!(
        !without_add
            .selected_message_action_items()
            .iter()
            .any(|action| action.kind == MessageActionKind::AddReaction)
    );

    let mut with_add = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY | PERM_ADD_REACTIONS,
        Vec::new(),
    );
    with_add.focus_pane(FocusPane::Messages);

    assert!(
        with_add
            .selected_message_action_items()
            .iter()
            .any(|action| action.kind == MessageActionKind::AddReaction)
    );
}

#[test]
fn existing_reaction_can_be_added_without_add_reactions_permission() {
    let mut state = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        vec![ReactionInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            count: 1,
            me: false,
        }],
    );
    state.focus_pane(FocusPane::Messages);
    let add_reaction_index = state
        .selected_message_action_items()
        .iter()
        .position(|action| action.kind == MessageActionKind::AddReaction)
        .expect("existing reaction should keep reaction picker available");
    state.open_selected_message_actions();
    assert!(state.select_message_action_row(add_reaction_index));

    assert_eq!(state.activate_selected_message_action(), None);
    assert!(state.is_emoji_reaction_picker_open());
    assert_eq!(
        state
            .emoji_reaction_items()
            .iter()
            .map(|item| item.emoji.clone())
            .collect::<Vec<_>>(),
        vec![ReactionEmoji::Unicode("👍".to_owned())]
    );
    assert_eq!(
        state.activate_selected_emoji_reaction(),
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        })
    );
}

#[test]
fn reaction_picker_prioritizes_existing_reactions_and_qwerty_shortcuts() {
    let mut state = state_with_reaction_message();

    state.open_emoji_reaction_picker();

    let items = state.filtered_emoji_reaction_items();
    assert_eq!(items[0].emoji, ReactionEmoji::Unicode("👍".to_owned()));
    assert_eq!(
        items[1].emoji,
        ReactionEmoji::Custom {
            id: Id::new(50),
            name: Some("party".to_owned()),
            animated: false,
        }
    );

    let command = state.activate_emoji_reaction_shortcut('q');
    assert_eq!(
        command,
        Some(AppCommand::RemoveReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        })
    );
}

#[test]
fn show_reacted_users_requires_read_message_history() {
    let reactions = vec![ReactionInfo {
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
        count: 1,
        me: false,
    }];
    let mut without_history =
        state_with_other_user_message_permissions(PERM_VIEW_CHANNEL, reactions.clone());
    without_history.focus_pane(FocusPane::Messages);

    assert!(
        !without_history
            .selected_message_action_items()
            .iter()
            .any(|action| action.kind == MessageActionKind::ShowReactionUsers)
    );

    let mut with_history = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        reactions,
    );
    with_history.focus_pane(FocusPane::Messages);

    assert!(
        with_history
            .selected_message_action_items()
            .iter()
            .any(|action| action.kind == MessageActionKind::ShowReactionUsers)
    );
}

#[test]
fn custom_emoji_action_label_uses_id_when_images_are_disabled() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(2),
        before: None,
        messages: vec![MessageInfo {
            reactions: vec![ReactionInfo {
                emoji: ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
                count: 1,
                me: true,
            }],
            ..message_info(Id::new(2), 1)
        }],
    });
    state.open_options_popup();
    for _ in 0..4 {
        state.move_option_down();
    }
    state.toggle_selected_display_option();
    state.close_options_popup();
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(actions.iter().any(|action| {
        action.kind == MessageActionKind::RemoveReaction(0) && action.label == "Remove 50 reaction"
    }));
}

#[test]
fn show_reacted_users_action_loads_all_reaction_emojis() {
    let mut state = state_with_reaction_message();
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    for _ in 0..4 {
        state.move_message_action_down();
    }

    let command = state.activate_selected_message_action();

    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![
                ReactionEmoji::Unicode("👍".to_owned()),
                ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                },
            ],
        })
    );
    assert!(!state.is_message_action_menu_open());
}

#[test]
fn reaction_users_loaded_opens_popup_state() {
    let mut state = state_with_messages(1);

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

    assert!(state.is_reaction_users_popup_open());
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.reactions()[0].users[0].display_name.as_str()),
        Some("neo")
    );
}

#[test]
fn reaction_users_popup_scroll_down_clamps_at_bottom() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
            users: (1..=6)
                .map(|id| ReactionUserInfo {
                    user_id: Id::new(id),
                    display_name: format!("user-{id}"),
                })
                .collect(),
        }],
    });
    // 1 header + 6 users = 7 data lines. With a 3-line viewport the
    // furthest the user can scroll is 4.
    state.set_reaction_users_popup_view_height(3);

    for _ in 0..50 {
        state.scroll_reaction_users_popup_down();
    }
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(4)
    );

    // A single 'k' press should now move the scroll back, not be eaten by
    // the inflated counter.
    state.scroll_reaction_users_popup_up();
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(3)
    );
}
