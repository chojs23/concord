use super::*;
use crate::discord::AppCommand;

fn push_foreign_reaction_emojis(state: &mut DashboardState) {
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(9),
        emojis: vec![
            CustomEmojiInfo::test(Id::new(60), "wave_foreign"),
            CustomEmojiInfo {
                animated: true,
                ..CustomEmojiInfo::test(Id::new(61), "dance_foreign")
            },
        ],
    });
}

#[test]
fn emoji_picker_items_include_available_custom_emojis_for_selected_message_guild() {
    let mut state = state_with_custom_emojis();
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

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
    let mut state = state_with_custom_emojis();
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

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
        emojis: vec![CustomEmojiInfo::test(Id::new(60), "wave")],
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
fn emoji_picker_items_include_foreign_custom_emojis_for_nitro_users() {
    let mut state = state_with_custom_emojis();
    push_foreign_reaction_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

    let items = state.emoji_reaction_items();

    assert!(items.iter().any(|item| matches!(
        &item.emoji,
        ReactionEmoji::Custom { id, name, animated: false }
            if *id == Id::new(60) && name.as_deref() == Some("wave_foreign")
    )));
    assert!(items.iter().any(|item| matches!(
        &item.emoji,
        ReactionEmoji::Custom { id, name, animated: true }
            if *id == Id::new(61) && name.as_deref() == Some("dance_foreign")
    )));
}

#[test]
fn emoji_picker_selection_returns_foreign_custom_reaction_command_for_nitro_users() {
    let mut state = state_with_custom_emojis();
    push_foreign_reaction_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });
    state.focus_pane(FocusPane::Messages);
    state.open_emoji_reaction_picker();
    state.start_emoji_reaction_filter();
    for value in "wave foreign".chars() {
        state.push_emoji_reaction_filter_char(value);
    }

    let command = state.activate_selected_emoji_reaction();

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Custom {
                id: Id::new(60),
                name: Some("wave_foreign".to_owned()),
                animated: false,
            },
        })
    );
}

#[test]
fn direct_messages_include_foreign_custom_reactions_for_nitro_users() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(dm_channel_info(channel_id, "neo")));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..guild_message_create_fixture()
    }));
    push_foreign_reaction_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

    let items = state.emoji_reaction_items();

    assert!(items.iter().any(|item| matches!(
        &item.emoji,
        ReactionEmoji::Custom { id, name, animated: false }
            if *id == Id::new(60) && name.as_deref() == Some("wave_foreign")
    )));
}

#[test]
fn emoji_picker_uses_channel_guild_when_selected_message_lacks_guild_id() {
    let mut state = state_with_custom_emojis();
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id: Id::new(2),
        message_id: Id::new(2),
        author_id: Id::new(99),
        content: Some("history message without guild".to_owned()),
        ..guild_message_create_fixture()
    }));

    let items = state.emoji_reaction_items();

    assert!(items.len() > 9);
    assert_eq!(items[8].label, "Party Time");
}

#[test]
fn emoji_picker_items_stay_unicode_only_for_direct_messages() {
    let mut state = DashboardState::new();
    let channel_id = Id::new(20);
    state.push_event(AppEvent::ChannelUpsert(dm_channel_info(channel_id, "neo")));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: None,
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        content: Some("hello".to_owned()),
        ..guild_message_create_fixture()
    }));

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

    let open_thread = actions
        .iter()
        .find(|action| action.kind == MessageActionKind::OpenThread)
        .expect("open thread action should exist");
    let show_reaction_users = actions
        .iter()
        .find(|action| action.kind == MessageActionKind::ShowReactionUsers)
        .expect("reaction users action should exist");
    let open_poll_vote_picker = actions
        .iter()
        .find(|action| action.kind == MessageActionKind::OpenPollVotePicker)
        .expect("poll action should exist");
    assert!(!open_thread.enabled);
    assert!(show_reaction_users.enabled);
    assert!(!open_poll_vote_picker.enabled);
    assert_eq!(
        actions
            .iter()
            .filter(|action| action.kind == MessageActionKind::ShowReactionUsers)
            .count(),
        1
    );
}

#[test]
fn reaction_picker_requires_history_and_existing_or_add_reactions_permission() {
    let mut without_add = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        Vec::new(),
    );
    without_add.focus_pane(FocusPane::Messages);

    without_add.open_emoji_reaction_picker();
    assert!(
        !without_add
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );

    let mut with_add = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY | PERM_ADD_REACTIONS,
        Vec::new(),
    );
    with_add.focus_pane(FocusPane::Messages);

    with_add.open_emoji_reaction_picker();
    assert!(
        with_add
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
}

#[test]
fn existing_reaction_can_be_added_without_add_reactions_permission() {
    let mut state = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        vec![ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))],
    );
    state.focus_pane(FocusPane::Messages);
    state.open_emoji_reaction_picker();

    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::EmojiReactionPicker)
    );
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
fn reaction_picker_prioritizes_existing_reactions_and_digit_shortcuts() {
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

    // The prioritized existing reaction sits first, so `1` toggles it off.
    let command = state.activate_emoji_reaction_shortcut('1');
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
    let reactions = vec![ReactionInfo::test(ReactionEmoji::Unicode("👍".to_owned()))];
    let mut without_history =
        state_with_other_user_message_permissions(PERM_VIEW_CHANNEL, reactions.clone());
    without_history.focus_pane(FocusPane::Messages);

    let without_history_action = without_history
        .selected_message_action_items()
        .into_iter()
        .find(|action| action.kind == MessageActionKind::ShowReactionUsers)
        .expect("show reacted users action should still be visible");
    assert!(!without_history_action.enabled);

    let mut with_history = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        reactions,
    );
    with_history.focus_pane(FocusPane::Messages);

    let with_history_action = with_history
        .selected_message_action_items()
        .into_iter()
        .find(|action| action.kind == MessageActionKind::ShowReactionUsers)
        .expect("show reacted users action should be visible");
    assert!(with_history_action.enabled);
}

#[test]
fn show_reacted_users_action_loads_all_reaction_emojis() {
    let mut state = state_with_reaction_message();
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();
    let row = state
        .selected_message_action_items()
        .iter()
        .position(|action| action.kind == MessageActionKind::ShowReactionUsers)
        .expect("reaction users action should exist");
    assert!(state.select_message_action_row(row));

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
    assert!(!state.is_message_action_context_active());
}

#[test]
fn reaction_users_loaded_opens_popup_state() {
    let mut state = state_with_messages(1);

    state.push_event(AppEvent::ReactionUsersLoaded {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        reactions: vec![ReactionUsersInfo {
            users: vec![ReactionUserInfo::test(Id::new(10), "neo")],
            ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
        }],
    });

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ReactionUsers));
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
            users: (1..=6)
                .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
                .collect(),
            ..ReactionUsersInfo::test(ReactionEmoji::Unicode("👍".to_owned()))
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
