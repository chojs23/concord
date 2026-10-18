use super::*;

#[test]
fn left_click_focuses_top_level_pane() {
    let mut state = DashboardState::new();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
}

#[test]
fn left_click_selects_visible_channel_row() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Messages);
    let (column, row) = channel_row_point(1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), 1);
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn double_click_activates_pane_rows_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseInputState::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(state.selected_channel_id(), Some(Id::new(11)));
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );

    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key('/'));
    for value in "random".chars() {
        handle_key(&mut state, char_key(value));
    }
    let mut clicks = MouseInputState::default();
    let (column, row) = channel_row_point(0);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(state.selected_channel_id(), None);
    assert_eq!(state.channel_pane_filter_query(), Some("random"));

    assert_eq!(second.command, None);

    let mut clicks = MouseInputState::default();
    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(state.focus(), FocusPane::Messages);

    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);
    handle_key(&mut state, char_key('/'));
    for value in "second".chars() {
        handle_key(&mut state, char_key(value));
    }
    let mut clicks = MouseInputState::default();
    let event = mouse(MouseEventKind::Down(MouseButton::Left), 1, 2);

    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert!(second.handled);
    assert_eq!(state.guild_pane_filter_query(), Some("second"));
    assert_eq!(second.command, None);

    let mut clicks = MouseInputState::default();
    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert_eq!(second.command, None);
    assert_eq!(state.selected_guild_id(), Some(Id::new(2)));
    assert_eq!(state.focus(), FocusPane::Channels);
}

#[test]
fn left_click_selects_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 6),
        dashboard_area(),
    ));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(
        state.channel_switcher_view().map(|view| view.selected),
        Some(1)
    );
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn double_click_activates_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();
    let mut clicks = MouseInputState::default();
    let event = mouse(MouseEventKind::Down(MouseButton::Left), 50, 6);

    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
}

#[test]
fn channel_switcher_absorbs_backdrop_clicks() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();
    state.focus_pane(FocusPane::Messages);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 21, 2),
        dashboard_area(),
    ));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::ChannelSwitcher));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert_eq!(state.selected_channel(), 0);
}

#[test]
fn wheel_moves_channel_switcher_selection() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 50, 7),
        dashboard_area(),
    ));
    assert_eq!(
        state.channel_switcher_view().map(|view| view.selected),
        Some(1)
    );

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 7),
        dashboard_area(),
    ));
    assert_eq!(
        state.channel_switcher_view().map(|view| view.selected),
        Some(0)
    );
}

#[test]
fn terminal_click_release_sequence_still_double_clicks_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseInputState::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let release = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Up(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert!(release.handled);
    assert!(second.handled);
    assert_eq!(
        second.command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );
}

#[test]
fn scroll_between_clicks_prevents_stale_double_click_activation() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseInputState::default();
    let (column, row) = channel_row_point(1);

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    let scroll = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::ScrollDown, column, row),
        dashboard_area(),
        &mut clicks,
    );
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(first.handled);
    assert!(scroll.handled);
    assert!(second.handled);
    assert_eq!(second.command, None);
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn left_click_on_message_input_starts_composer() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    let (column, row) = composer_point();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert!(state.is_composing());
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn mouse_click_outside_dashboard_panes_does_not_change_focus() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Messages);

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 10, 0),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Right), 10, 0),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn mouse_click_outside_composer_blurs_and_focuses_clicked_pane_without_clearing_draft() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('d'));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
    assert!(!state.is_composing());
    assert_eq!(state.composer_input(), "d");
}

#[test]
fn mouse_click_outside_reply_composer_preserves_draft_and_clears_reply_target() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('R'));
    handle_key(&mut state, char_key('d'));

    assert!(state.reply_target_message_state().is_some());
    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));

    assert!(!state.is_composing());
    assert_eq!(state.composer_input(), "d");
    assert!(state.reply_target_message_state().is_none());
}

#[test]
fn mouse_click_outside_composer_blurs_and_selects_clicked_row() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Up));
    state.start_composer();
    let (column, row) = channel_row_point(1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert!(!state.is_composing());
    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), 1);
}

#[test]
fn mouse_scroll_outside_composer_does_not_clear_draft() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('d'));

    assert!(!handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 100, 1),
        dashboard_area(),
    ));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "d");
}

#[test]
fn mouse_wheel_scrolls_hovered_channel_viewport_without_moving_selection() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Messages);
    state.set_channel_view_height(2);
    let selected = state.selected_channel();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 21, 1),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Channels);
    assert_eq!(state.selected_channel(), selected);
    assert_eq!(state.channel_scroll(), 1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 21, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_channel(), selected);
    assert_eq!(state.channel_scroll(), 0);
}

#[test]
fn mouse_wheel_scrolls_message_viewport_without_changing_selection() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(2, 16, 3);
    let selected = state.selected_message();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 50, 1),
        dashboard_area(),
    ));
    state.clamp_message_viewport_for_image_previews(2, 16, 3);

    assert_eq!(state.focus(), FocusPane::Messages);
    assert_eq!(state.selected_message(), selected);
    assert!(state.message_line_scroll() > 0);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn mouse_wheel_at_message_top_requests_older_history() {
    let mut state = state_with_messages(3);
    state.focus_pane(FocusPane::Messages);
    state.scroll_message_viewport_top();
    state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let selected = state.selected_message();
    let mut clicks = MouseInputState::default();

    let outcome = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 1),
        dashboard_area(),
        &mut clicks,
    );

    assert!(outcome.handled);
    assert_eq!(state.selected_message(), selected);
    assert_eq!(
        outcome.command,
        Some(AppCommand::LoadMessageHistory {
            channel_id: Id::new(2),
            before: Some(Id::new(1)),
        })
    );
}

#[test]
fn left_click_on_a_reaction_chip_toggles_the_users_own_reaction() {
    use ratatui::{Terminal, backend::TestBackend};

    let single_reaction = &["👍"][..];
    let wrapped_reactions = &["🔥", "❤️", "😂", "🎉", "😮", "😢", "🙏", "👀", "💯", "👍"][..];
    for (count, message_id, selected, focus, emojis) in [
        (1, 1, 0, FocusPane::Messages, single_reaction),
        (3, 2, 2, FocusPane::Messages, single_reaction),
        (3, 2, 2, FocusPane::Messages, wrapped_reactions),
        (3, 2, 0, FocusPane::Channels, single_reaction),
        (3, 2, 1, FocusPane::Channels, single_reaction),
        (4, 3, 1, FocusPane::Channels, single_reaction),
        (3, 2, 0, FocusPane::Channels, wrapped_reactions),
        (3, 2, 1, FocusPane::Channels, wrapped_reactions),
    ] {
        for me in [false, true] {
            let reactions = emojis
                .iter()
                .map(|emoji| ReactionInfo {
                    count: 2,
                    me,
                    ..ReactionInfo::test(ReactionEmoji::Unicode((*emoji).to_owned()))
                })
                .collect::<Vec<_>>();
            let mut state = state_with_reaction_messages(count, message_id, &reactions);
            let area = dashboard_area();
            state.focus_pane(FocusPane::Messages);
            crate::tui::ui::sync_view_heights(area, &mut state);
            let layout = crate::tui::ui::image_preview_layout(area, &state);
            state.clamp_message_viewport_for_image_previews(
                layout.content_width,
                layout.preview_width,
                layout.max_preview_height,
            );
            state.jump_top();
            for _ in 0..selected {
                state.move_down();
            }
            state.focus_pane(focus);

            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))
                .expect("test terminal builds");
            terminal
                .draw(|frame| crate::tui::ui::render(frame, &state, vec![], vec![], vec![], None))
                .expect("test frame renders");
            let buffer = terminal.backend().buffer();
            // Use the rendered chip, not the hit map whose geometry this verifies.
            let (column, row) = (0..area.height)
                .flat_map(|row| (0..area.width).map(move |column| (column, row)))
                .find(|&(column, row)| buffer[(column, row)].symbol() == "👍")
                .expect("the 👍 reaction chip is visible");

            let mut clicks = MouseInputState::default();
            let outcome = handle_mouse_event(
                &mut state,
                mouse(MouseEventKind::Down(MouseButton::Left), column, row),
                area,
                &mut clicks,
            );
            let emoji = ReactionEmoji::Unicode("👍".to_owned());
            let expected = if me {
                AppCommand::RemoveReaction {
                    channel_id: Id::new(2),
                    message_id: Id::new(message_id),
                    emoji,
                }
            } else {
                AppCommand::AddReaction {
                    channel_id: Id::new(2),
                    message_id: Id::new(message_id),
                    emoji,
                }
            };
            assert!(outcome.handled);
            assert_eq!(state.selected_message(), (message_id - 1) as usize);
            assert_eq!(
                outcome.command,
                Some(expected),
                "count={count}, selected={selected}, focus={focus:?}, me={me}, emojis={emojis:?}",
            );
        }
    }
}

#[test]
fn right_click_selects_dashboard_item_and_opens_its_action_menu() {
    let mut guild_state = state_with_channel_tree();
    let guild = interaction_point(
        &guild_state,
        dashboard_area(),
        crate::tui::ui::InteractionTarget::PaneItem {
            pane: FocusPane::Guilds,
            row: 0,
        },
    );
    assert!(handle_mouse(
        &mut guild_state,
        mouse(MouseEventKind::Down(MouseButton::Right), guild.0, guild.1),
        dashboard_area(),
    ));
    assert!(guild_state.is_guild_action_menu_active());

    let mut channel_state = state_with_channel_tree();
    let (column, row) = channel_row_point(1);

    assert!(handle_mouse(
        &mut channel_state,
        mouse(MouseEventKind::Down(MouseButton::Right), column, row),
        dashboard_area(),
    ));
    assert_eq!(channel_state.focus(), FocusPane::Channels);
    assert_eq!(channel_state.selected_channel(), 1);
    assert!(channel_state.is_channel_action_menu_active());

    let mut message_state = state_with_messages(1);
    message_state.focus_pane(FocusPane::Messages);
    message_state.set_message_view_height(10);
    message_state.clamp_message_viewport_for_image_previews(80, 16, 3);
    let (column, row) = (0..dashboard_area().height)
        .flat_map(|row| (0..dashboard_area().width).map(move |column| (column, row)))
        .find(|(column, row)| {
            matches!(
                crate::tui::ui::InteractionMap::new(dashboard_area(), &message_state)
                    .target_at(*column, *row),
                Some(crate::tui::ui::InteractionTarget::PaneItem {
                    pane: FocusPane::Messages,
                    ..
                })
            )
        })
        .expect("message row is interactive");

    assert!(handle_mouse(
        &mut message_state,
        mouse(MouseEventKind::Down(MouseButton::Right), column, row),
        dashboard_area(),
    ));
    assert!(message_state.is_message_action_menu_active());

    let mut member_state = state_with_members(1);
    let member = interaction_point(
        &member_state,
        dashboard_area(),
        crate::tui::ui::InteractionTarget::PaneItem {
            pane: FocusPane::Members,
            row: 1,
        },
    );
    assert!(handle_mouse(
        &mut member_state,
        mouse(MouseEventKind::Down(MouseButton::Right), member.0, member.1),
        dashboard_area(),
    ));
    assert!(member_state.is_member_action_menu_active());
}

#[test]
fn user_profile_popup_absorbs_all_backdrop_clicks() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Messages);
    state.open_user_profile_popup(Id::new(10), None);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 60, 10),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::UserProfile));
}

#[test]
fn current_user_profile_tabs_and_fields_accept_clicks() {
    let user_id = Id::new(10);
    let mut state = DashboardState::new();
    push_test_ready(&mut state, user_id);
    state.push_event(AppEvent::UserProfileLoaded {
        guild_id: None,
        profile: UserProfileInfo::test(user_id, "neo"),
    });
    state.open_current_user_profile_popup();
    let area = Rect::new(0, 0, 120, 40);
    crate::tui::ui::sync_view_heights(area, &mut state);

    let guild_tab = interaction_point(
        &state,
        area,
        crate::tui::ui::InteractionTarget::UserProfileControl(
            crate::tui::ui::UserProfileControl::Tab(UserProfileSettingsTab::Guild),
        ),
    );
    assert!(handle_mouse(
        &mut state,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            guild_tab.0,
            guild_tab.1
        ),
        area,
    ));
    assert_eq!(
        state.user_profile_settings_tab(),
        UserProfileSettingsTab::Guild
    );

    let global_tab = interaction_point(
        &state,
        area,
        crate::tui::ui::InteractionTarget::UserProfileControl(
            crate::tui::ui::UserProfileControl::Tab(UserProfileSettingsTab::Global),
        ),
    );
    handle_mouse(
        &mut state,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            global_tab.0,
            global_tab.1,
        ),
        area,
    );
    assert_eq!(
        state.user_profile_settings_tab(),
        UserProfileSettingsTab::Global
    );

    let pronouns = interaction_point(
        &state,
        area,
        crate::tui::ui::InteractionTarget::UserProfileControl(
            crate::tui::ui::UserProfileControl::Field(UserProfileSettingsField::GlobalPronouns),
        ),
    );
    assert!(handle_mouse(
        &mut state,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            pronouns.0,
            pronouns.1
        ),
        area,
    ));
    assert_eq!(
        state.user_profile_settings_active_field(),
        Some(UserProfileSettingsField::GlobalPronouns)
    );
    assert!(state.is_user_profile_popup_editing());
}

#[test]
fn popup_controls_accept_direct_clicks() {
    let area = dashboard_area();

    let mut confirmation = DashboardState::new();
    confirmation.open_quit_confirmation();
    let cancel = interaction_point(
        &confirmation,
        area,
        crate::tui::ui::InteractionTarget::ConfirmationButton(
            crate::tui::state::ConfirmationButton::Cancel,
        ),
    );
    handle_mouse(
        &mut confirmation,
        mouse(MouseEventKind::Down(MouseButton::Left), cancel.0, cancel.1),
        area,
    );
    assert_eq!(confirmation.active_modal_popup_kind(), None);

    let mut search = DashboardState::new();
    search.open_message_search_popup();
    let field = interaction_point(
        &search,
        area,
        crate::tui::ui::InteractionTarget::SearchField(3),
    );
    handle_mouse(
        &mut search,
        mouse(MouseEventKind::Down(MouseButton::Left), field.0, field.1),
        area,
    );
    assert!(
        search
            .search_popup_view()
            .and_then(|view| view.fields.get(3).map(|field| field.active))
            .unwrap_or(false)
    );

    let mut inbox = DashboardState::new();
    inbox.open_notification_inbox();
    let mentions = interaction_point(
        &inbox,
        area,
        crate::tui::ui::InteractionTarget::NotificationInboxTab(NotificationInboxTab::Mentions),
    );
    handle_mouse(
        &mut inbox,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            mentions.0,
            mentions.1,
        ),
        area,
    );
    assert_eq!(
        inbox.notification_inbox_tab(),
        Some(NotificationInboxTab::Mentions)
    );

    let mut folder = state_with_folder();
    folder.focus_pane(FocusPane::Guilds);
    handle_key(&mut folder, char_key(' '));
    handle_key(&mut folder, char_key('a'));
    handle_key(&mut folder, char_key('r'));
    let color = interaction_point(
        &folder,
        area,
        crate::tui::ui::InteractionTarget::FolderSettingsField(FolderSettingsField::Color),
    );
    handle_mouse(
        &mut folder,
        mouse(MouseEventKind::Down(MouseButton::Left), color.0, color.1),
        area,
    );
    assert!(folder.folder_settings_color_active());
    assert!(folder.is_folder_settings_editing());
}

#[test]
fn mouse_double_click_activates_message_action_row_like_enter() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let mut clicks = MouseInputState::default();
    let poll_row = state
        .selected_message_action_items()
        .iter()
        .position(|action| action.kind == MessageActionKind::OpenPollVotePicker)
        .expect("poll action should exist");
    let count = state.selected_message_action_items().len() as u16;
    let (column, row) = message_action_row_point(
        count,
        u16::try_from(poll_row).expect("message action row fits in test area"),
    );

    let first = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    assert!(first.handled, "mouse_click_selects_message_action_row");
    assert_eq!(
        first.command, None,
        "mouse_click_selects_message_action_row"
    );
    assert_eq!(
        state.selected_message_action_index(),
        Some(poll_row),
        "mouse_click_selects_message_action_row"
    );
    assert!(
        state.is_message_action_menu_active(),
        "mouse_click_selects_message_action_row"
    );

    let release = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Up(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    assert!(release.handled);
    assert!(state.is_message_action_menu_active());
    let second = handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );

    assert!(second.handled);
    assert_eq!(second.command, None);
    assert!(!state.is_message_action_menu_active());
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));
}

#[test]
fn mouse_wheel_moves_active_popup_lists() {
    {
        let mut state = state_with_thread_created_message();
        state.focus_pane(FocusPane::Messages);
        handle_key(&mut state, key(KeyCode::Enter));
        let count = state.selected_message_action_items().len() as u16;
        let (column, row) = message_action_row_point(count, 0);

        assert!(handle_mouse(
            &mut state,
            mouse(MouseEventKind::ScrollDown, column, row),
            dashboard_area(),
        ));
        assert_eq!(state.selected_message_action_index(), Some(1));

        assert!(handle_mouse(
            &mut state,
            mouse(MouseEventKind::ScrollUp, column, row),
            dashboard_area(),
        ));
        assert_eq!(state.selected_message_action_index(), Some(0));
    }

    {
        let mut state = state_with_messages(1);
        state.open_options_popup();

        assert!(handle_mouse(
            &mut state,
            mouse(MouseEventKind::ScrollDown, 60, 10),
            dashboard_area(),
        ));
        assert_eq!(state.selected_option_index(), Some(1));

        assert!(handle_mouse(
            &mut state,
            mouse(MouseEventKind::ScrollUp, 60, 10),
            dashboard_area(),
        ));
        assert_eq!(state.selected_option_index(), Some(0));

        let (column, row) = (0..dashboard_area().height)
            .flat_map(|row| (0..dashboard_area().width).map(move |column| (column, row)))
            .find(|(column, row)| {
                crate::tui::ui::InteractionMap::new(dashboard_area(), &state)
                    .target_at(*column, *row)
                    == Some(crate::tui::ui::InteractionTarget::PopupItem {
                        target: SelectablePopupTarget::Options,
                        row: 0,
                    })
            })
            .expect("options row is clickable");
        let mut clicks = MouseInputState::default();
        let event = mouse(MouseEventKind::Down(MouseButton::Left), column, row);
        handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
        handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
        assert!(!state.is_options_category_picker_open());
    }
}

fn interaction_point(
    state: &DashboardState,
    area: Rect,
    expected: crate::tui::ui::InteractionTarget,
) -> (u16, u16) {
    (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .find(|(column, row)| {
            crate::tui::ui::InteractionMap::new(area, state).target_at(*column, *row)
                == Some(expected)
        })
        .expect("interaction target should be visible")
}
