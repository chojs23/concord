use super::*;
use crate::discord::AppCommand;
use crate::discord::test_builders::{ReactionUsersLoadedFixture, reaction_users_loaded_event};

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
fn emoji_picker_respects_channel_permission_for_foreign_custom_emojis() {
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

    let mut state = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY | PERM_ADD_REACTIONS,
        Vec::new(),
    );
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo::test(Id::new(50), "local")],
    });
    push_foreign_reaction_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities {
        premium_tier: PremiumTier::Nitro,
    });

    let items = state.emoji_reaction_items();

    assert!(items.iter().any(|item| matches!(
        item.emoji,
        ReactionEmoji::Custom { id, .. } if id == Id::new(50)
    )));
    assert!(!items.iter().any(|item| matches!(
        item.emoji,
        ReactionEmoji::Custom { id, .. } if id == Id::new(60) || id == Id::new(61)
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
    assert!(!open_thread.is_enabled());
    assert!(show_reaction_users.is_enabled());
    assert!(!open_poll_vote_picker.is_enabled());
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
    assert!(!without_history_action.is_enabled());

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
    assert!(with_history_action.is_enabled());
}

#[test]
fn show_reacted_users_action_opens_reaction_list() {
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

    // Opening the popup makes no request: it shows the reaction list, and users
    // are fetched only once the reader drills into a reaction.
    assert_eq!(command, None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ReactionUsers));
    let popup = state
        .reaction_users_popup()
        .expect("popup should be open on the reaction list");
    assert_eq!(popup.entries().len(), 2);
    assert!(!popup.is_viewing_users());
    assert!(!state.is_message_action_menu_active());
}

#[test]
fn reaction_users_activate_requests_first_page_and_loaded_fills_users() {
    let mut state = state_with_messages(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 1)]);

    // Drilling into the highlighted reaction requests its first page.
    let command = state.activate_reaction_users_popup();
    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: emoji.clone(),
            after: None,
        })
    );
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.is_viewing_users()),
        Some(true)
    );

    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji,
        users: vec![ReactionUserInfo::test(Id::new(10), "neo")],
        next_after: None,
        after: None,
    }));

    assert_eq!(
        state.reaction_users_popup().and_then(|popup| {
            popup
                .viewed_entry()
                .map(|entry| entry.users()[0].display_name.clone())
        }),
        Some("neo".to_owned())
    );
}

#[test]
fn reaction_users_load_failure_clears_loading_and_allows_retry() {
    let mut state = state_with_messages(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 1)]);
    state.activate_reaction_users_popup();

    state.push_event(AppEvent::ReactionUsersLoadFailed {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji: emoji.clone(),
    });
    // The failure clears the loading flag rather than leaving it stuck.
    assert_eq!(
        state
            .reaction_users_popup()
            .and_then(|popup| popup.viewed_entry())
            .map(|entry| entry.is_loading()),
        Some(false)
    );

    // Backing out and reopening the reaction issues a fresh request.
    assert!(state.reaction_users_popup_back());
    assert_eq!(
        state.activate_reaction_users_popup(),
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji,
            after: None,
        })
    );
}

#[test]
fn reaction_users_popup_page_down_requests_next_page() {
    let mut state = state_with_messages(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 150)]);
    state.activate_reaction_users_popup();
    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji: emoji.clone(),
        users: (1..=100)
            .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
            .collect(),
        next_after: Some(Id::new(100)),
        after: None,
    }));
    state.set_reaction_users_popup_view_height(3);

    // Paging to the bottom must still fetch the next page, not stall at 100.
    let mut command = None;
    for _ in 0..200 {
        assert!(state.page_active_popup_down());
        if let Some(cmd) = state.reaction_users_popup_take_load_more() {
            command = Some(cmd);
            break;
        }
    }
    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji,
            after: Some(Id::new(100)),
        })
    );
}

#[test]
fn reaction_users_popup_scroll_down_clamps_at_bottom() {
    use crate::tui::keybindings::SelectionAction;

    let mut state = state_with_messages(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 6)]);
    state.activate_reaction_users_popup();
    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji,
        users: (1..=6)
            .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
            .collect(),
        next_after: None,
        after: None,
    }));
    // 6 user rows with a 3-line viewport: the furthest scroll offset is 3.
    state.set_reaction_users_popup_view_height(3);

    for _ in 0..50 {
        state.navigate_reaction_users_popup(SelectionAction::Next);
    }
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.user_scroll()),
        Some(3)
    );

    // A single scroll-up press moves back one row rather than being eaten by an
    // inflated counter.
    state.navigate_reaction_users_popup(SelectionAction::Previous);
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.user_scroll()),
        Some(2)
    );
}

#[test]
fn reaction_users_popup_scroll_requests_next_page_when_more_remain() {
    use crate::tui::keybindings::SelectionAction;

    let mut state = state_with_messages(1);
    let emoji = ReactionEmoji::Unicode("👍".to_owned());
    state.open_reaction_users_popup(Id::new(2), Id::new(1), vec![(emoji.clone(), 150)]);
    state.activate_reaction_users_popup();
    state.push_event(reaction_users_loaded_event(ReactionUsersLoadedFixture {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji: emoji.clone(),
        users: (1..=100)
            .map(|id| ReactionUserInfo::test(Id::new(id), format!("user-{id}")))
            .collect(),
        next_after: Some(Id::new(100)),
        after: None,
    }));
    state.set_reaction_users_popup_view_height(3);

    // A full first page that still has more should be marked as paginable.
    assert_eq!(
        state
            .reaction_users_popup()
            .and_then(|popup| popup.viewed_entry())
            .map(|entry| entry.has_more()),
        Some(true)
    );

    // Scrolling to the bottom asks for the next page continuing after user 100.
    let mut command = None;
    for _ in 0..100 {
        if let Some(cmd) = state.navigate_reaction_users_popup(SelectionAction::Next) {
            command = Some(cmd);
            break;
        }
    }
    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji,
            after: Some(Id::new(100)),
        })
    );
}

#[test]
fn reaction_users_popup_opens_highlighted_reaction() {
    use crate::tui::keybindings::SelectionAction;

    let mut state = state_with_messages(1);
    let first = ReactionEmoji::Unicode("👍".to_owned());
    let second = ReactionEmoji::Unicode("🎉".to_owned());
    state.open_reaction_users_popup(
        Id::new(2),
        Id::new(1),
        vec![(first.clone(), 1), (second.clone(), 1)],
    );

    // Move the reaction-list selection to the second reaction, then open it.
    assert_eq!(
        state.navigate_reaction_users_popup(SelectionAction::Next),
        None
    );
    let command = state.activate_reaction_users_popup();
    assert_eq!(
        command,
        Some(AppCommand::LoadReactionUsers {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: second.clone(),
            after: None,
        })
    );
    assert_eq!(
        state
            .reaction_users_popup()
            .and_then(|popup| popup.viewed_entry())
            .map(|entry| entry.emoji().clone()),
        Some(second)
    );

    // Backing out returns to the reaction list without closing the popup.
    assert!(state.reaction_users_popup_back());
    assert_eq!(
        state
            .reaction_users_popup()
            .map(|popup| popup.is_viewing_users()),
        Some(false)
    );
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ReactionUsers));
}
