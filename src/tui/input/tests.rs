use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::Id;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::{MouseClickTracker, handle_key, handle_mouse, handle_mouse_event, handle_paste};
use crate::{
    config::ImagePreviewQualityPreset,
    discord::{
        AppCommand, AppEvent, ChannelInfo, ChannelNotificationOverrideInfo, ChannelRecipientInfo,
        CustomEmojiInfo, DownloadAttachmentSource, GuildFolder, GuildNotificationSettingsInfo,
        MemberInfo, MessageReferenceInfo, NotificationLevel, PollAnswerInfo, PollInfo,
        PresenceStatus, ReactionEmoji, ReactionUserInfo, ReactionUsersInfo,
    },
    tui::state::{ChannelPaneEntry, DashboardState, FocusPane, GuildPaneEntry, MessageActionKind},
};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn char_key(value: char) -> KeyEvent {
    key(KeyCode::Char(value))
}

fn ctrl_key(value: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(value), KeyModifiers::CONTROL)
}

fn shift_enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
}

fn alt_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn channel_row_point(row: u16) -> (u16, u16) {
    (21, 2 + row)
}

fn composer_point() -> (u16, u16) {
    (50, 16)
}

fn message_row_point(row: u16) -> (u16, u16) {
    (50, 2 + row)
}

fn message_action_row_point(row: u16) -> (u16, u16) {
    (46, 8 + row)
}

fn dashboard_area() -> Rect {
    Rect::new(0, 0, 120, 20)
}

fn temp_upload_file(name: &str, contents: &[u8]) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("concord-{unique}"));
    fs::create_dir_all(&directory).expect("temp upload directory can be created");
    let path = directory.join(name);
    fs::write(&path, contents).expect("temp upload file can be written");
    path
}

fn remove_temp_upload_file(path: &PathBuf) {
    let directory = path.parent().map(std::path::Path::to_path_buf);
    let _ = fs::remove_file(path);
    if let Some(directory) = directory {
        let _ = fs::remove_dir(directory);
    }
}

#[test]
fn enter_toggles_selected_folder_and_space_opens_leader() {
    let mut state = state_with_folder();
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, key(KeyCode::Enter));
    assert_selected_folder_collapsed(&state, true);

    handle_key(&mut state, char_key(' '));
    assert!(state.is_leader_active());
    assert_selected_folder_collapsed(&state, true);
}

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
fn movement_waits_for_enter_to_activate_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);

    assert_eq!(state.selected_channel_id(), None);

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_channel_id(), None);

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(11),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(11)));

    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(12),
        })
    );
    assert_eq!(state.selected_channel_id(), Some(Id::new(12)));
}

#[test]
fn enter_on_direct_message_kinds_subscribes_channel() {
    for kind in ["dm", "group-dm"] {
        let mut state = state_with_direct_message(kind);
        state.focus_pane(FocusPane::Channels);

        let command = handle_key(&mut state, key(KeyCode::Enter));

        assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
        assert_eq!(
            command,
            Some(AppCommand::SubscribeDirectMessage {
                channel_id: Id::new(20),
            })
        );
    }
}

#[test]
fn message_keys_use_scroll_controls() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(9);

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_message(), 5);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, key(KeyCode::PageUp));
    assert_eq!(state.selected_message(), 1);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, ctrl_key('d'));
    assert_eq!(state.selected_message(), 5);
    assert!(!state.message_auto_follow());

    handle_key(&mut state, ctrl_key('d'));
    assert_eq!(state.selected_message(), 9);
    // Half-page-down landed the cursor on the latest message, so
    // auto-follow re-engages.
    assert!(state.message_auto_follow());
}

#[test]
fn message_top_scroll_requests_older_history_once() {
    let mut state = state_with_messages(3);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('g'));
    let command = handle_key(&mut state, key(KeyCode::Up));

    assert_eq!(
        command,
        Some(AppCommand::LoadMessageHistory {
            channel_id: Id::new(2),
            before: Some(Id::new(1)),
        })
    );

    let duplicate = handle_key(&mut state, key(KeyCode::Up));

    assert_eq!(duplicate, None);
}

#[test]
fn message_viewport_scroll_keys_do_not_change_selection_or_request_history() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(2, 16, 3);
    let selected = state.selected_message();

    handle_key(&mut state, char_key('J'));
    state.clamp_message_viewport_for_image_previews(2, 16, 3);

    let command = handle_key(&mut state, char_key('K'));

    assert_eq!(command, None);
    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.message_line_scroll(), 0);
}

#[test]
fn message_home_end_scroll_viewport_without_changing_selection() {
    let mut state = state_with_messages(10);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(5);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    let selected = state.selected_message();

    handle_key(&mut state, key(KeyCode::Home));
    assert_eq!(state.selected_message(), selected);
    assert_eq!(state.message_scroll(), 0);

    handle_key(&mut state, key(KeyCode::End));
    assert_eq!(state.selected_message(), selected);
    assert!(state.message_scroll() > 0);
}

#[test]
fn page_keys_scroll_non_message_panes() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(9);

    handle_key(&mut state, key(KeyCode::PageDown));
    assert_eq!(state.selected_channel(), 2);

    handle_key(&mut state, key(KeyCode::PageUp));
    assert_eq!(state.selected_channel(), 0);
}

#[test]
fn composer_requires_selected_channel() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('i'));
    assert!(!state.is_composing());

    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, char_key('i'));
    assert!(state.is_composing());
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn number_keys_focus_top_level_panes() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('2'));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, char_key('3'));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, char_key('4'));
    assert_eq!(state.focus(), FocusPane::Members);

    handle_key(&mut state, char_key('1'));
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn number_keys_show_hidden_panes_before_focusing() {
    let mut state = DashboardState::new();
    state.toggle_pane_visibility(FocusPane::Guilds);
    state.toggle_pane_visibility(FocusPane::Channels);
    state.toggle_pane_visibility(FocusPane::Members);

    handle_key(&mut state, char_key('1'));
    assert!(state.is_pane_visible(FocusPane::Guilds));
    assert_eq!(state.focus(), FocusPane::Guilds);

    handle_key(&mut state, char_key('2'));
    assert!(state.is_pane_visible(FocusPane::Channels));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, char_key('4'));
    assert!(state.is_pane_visible(FocusPane::Members));
    assert_eq!(state.focus(), FocusPane::Members);
}

#[test]
fn bare_m_no_longer_mutes_focused_channel() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));

    let command = handle_key(&mut state, char_key('m'));

    assert_eq!(command, None);
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
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: false,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: vec![ChannelNotificationOverrideInfo {
                channel_id: Id::new(11),
                message_notifications: None,
                muted: true,
                mute_end_time: None,
            }],
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
    state.push_event(AppEvent::UserGuildNotificationSettingsInit {
        settings: vec![GuildNotificationSettingsInfo {
            guild_id: Some(Id::new(1)),
            message_notifications: Some(NotificationLevel::OnlyMentions),
            muted: true,
            mute_end_time: None,
            suppress_everyone: false,
            suppress_roles: false,
            channel_overrides: Vec::new(),
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
fn alt_arrows_adjust_focused_side_pane_width() {
    let mut state = DashboardState::new();

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, alt_key(KeyCode::Right));
    assert_eq!(state.pane_width(FocusPane::Channels), 25);

    handle_key(&mut state, alt_key(KeyCode::Left));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
    assert_eq!(
        state.take_display_options_save_request(),
        Some(state.display_options())
    );

    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, alt_key(KeyCode::Right));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
    assert_eq!(state.take_display_options_save_request(), None);
}

#[test]
fn alt_h_l_adjust_focused_side_pane_width() {
    let mut state = DashboardState::new();

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, alt_key(KeyCode::Char('l')));
    assert_eq!(state.pane_width(FocusPane::Channels), 25);

    handle_key(&mut state, alt_key(KeyCode::Char('h')));
    assert_eq!(state.pane_width(FocusPane::Channels), 24);
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
    assert!(state.is_channel_switcher_open());

    for ch in "rand".chars() {
        handle_key(&mut state, char_key(ch));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert!(!state.is_channel_switcher_open());
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
fn leader_leader_switcher_ctrl_n_and_ctrl_p_move_selection() {
    let mut state = state_with_channel_tree();

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key(' '));
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
fn tab_cycles_skip_hidden_panes() {
    let mut state = DashboardState::new();
    state.toggle_pane_visibility(FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Messages);

    state.toggle_pane_visibility(FocusPane::Members);
    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Guilds);
}

#[test]
fn tab_and_shift_tab_cycle_focus() {
    let mut state = DashboardState::new();

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus(), FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus(), FocusPane::Channels);

    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus(), FocusPane::Guilds);

    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus(), FocusPane::Members);
}

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
fn double_click_activates_selected_channel_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseClickTracker::default();
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
}

#[test]
fn left_click_selects_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 50, 7),
        dashboard_area(),
    ));

    assert!(state.is_channel_switcher_open());
    assert_eq!(state.selected_channel_switcher_index(), Some(1));
    assert_eq!(state.selected_channel_id(), None);
}

#[test]
fn double_click_activates_channel_switcher_row() {
    let mut state = state_with_channel_tree();
    state.open_channel_switcher();
    let mut clicks = MouseClickTracker::default();
    let event = mouse(MouseEventKind::Down(MouseButton::Left), 50, 7);

    let first = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);
    let second = handle_mouse_event(&mut state, event, dashboard_area(), &mut clicks);

    assert!(first.handled);
    assert_eq!(first.command, None);
    assert!(second.handled);
    assert!(!state.is_channel_switcher_open());
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

    assert!(state.is_channel_switcher_open());
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
    assert_eq!(state.selected_channel_switcher_index(), Some(1));

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 50, 7),
        dashboard_area(),
    ));
    assert_eq!(state.selected_channel_switcher_index(), Some(0));
}

#[test]
fn terminal_click_release_sequence_still_double_clicks_like_enter() {
    let mut state = state_with_channel_tree();
    let mut clicks = MouseClickTracker::default();
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
    let mut clicks = MouseClickTracker::default();
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
fn forum_blank_bottom_rows_do_not_select_hidden_posts() {
    let mut state = state_with_forum_channel_posts();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: Id::new(20),
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 2,
        next_offset: 3,
        posts: vec![ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(29),
            parent_id: Some(Id::new(20)),
            position: Some(2),
            last_message_id: None,
            name: "hidden by remainder rows".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: Some(1),
            total_message_sent: Some(1),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        preview_messages: Vec::new(),
        has_more: false,
    });
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);
    let (column, row) = message_row_point(11);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.selected_forum_post(), 0);
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
        mouse(MouseEventKind::Down(MouseButton::Right), 1, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn mouse_click_outside_composer_blurs_and_focuses_clicked_pane() {
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
    assert_eq!(state.composer_input(), "");
}

#[test]
fn mouse_click_outside_composer_blurs_and_selects_clicked_row() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
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
fn mouse_wheel_scrolls_hovered_member_viewport_without_moving_selection() {
    let mut state = state_with_members(10);
    state.focus_pane(FocusPane::Messages);
    state.set_member_view_height(4);
    let selected = state.selected_member();

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollDown, 100, 1),
        dashboard_area(),
    ));

    assert_eq!(state.focus(), FocusPane::Members);
    assert_eq!(state.selected_member(), selected);
    assert_eq!(state.member_scroll(), 1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::ScrollUp, 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.selected_member(), selected);
    assert_eq!(state.member_scroll(), 0);
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
fn user_profile_popup_absorbs_left_clicks_only_inside_popup() {
    let mut state = DashboardState::new();
    state.focus_pane(FocusPane::Messages);
    state.open_user_profile_popup(Id::new(10), None);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 60, 10),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Messages);
    assert!(state.is_user_profile_popup_open());

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), 100, 1),
        dashboard_area(),
    ));
    assert_eq!(state.focus(), FocusPane::Members);
    assert!(state.is_user_profile_popup_open());
}

#[test]
fn number_keys_type_digits_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('4'));

    assert_eq!(state.focus(), FocusPane::Messages);
    assert_eq!(state.composer_input(), "4");
}

#[test]
fn backtick_toggles_debug_log_popup() {
    let mut state = DashboardState::new();

    handle_key(&mut state, char_key('`'));
    assert!(state.is_debug_log_popup_open());

    handle_key(&mut state, char_key('`'));
    assert!(!state.is_debug_log_popup_open());
}

#[test]
fn esc_closes_debug_log_popup_modally() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.toggle_debug_log_popup();

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_debug_log_popup_open());
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn backtick_types_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    handle_key(&mut state, char_key('`'));

    assert!(state.is_composing());
    assert!(!state.is_debug_log_popup_open());
    assert_eq!(state.composer_input(), "`");
}

#[test]
fn shift_enter_inserts_newline_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('h'));
    handle_key(&mut state, shift_enter());
    handle_key(&mut state, char_key('i'));

    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "h\ni");
}

#[test]
fn composer_cursor_edits_in_middle() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "abcd".chars() {
        handle_key(&mut state, char_key(value));
    }

    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, key(KeyCode::Left));
    handle_key(&mut state, char_key('X'));
    assert_eq!(state.composer_input(), "abXcd");
    assert_eq!(state.composer_cursor_byte_index(), 3);

    handle_key(&mut state, key(KeyCode::Backspace));
    assert_eq!(state.composer_input(), "abcd");
    assert_eq!(state.composer_cursor_byte_index(), 2);

    handle_key(&mut state, key(KeyCode::Delete));
    assert_eq!(state.composer_input(), "abd");
    assert_eq!(state.composer_cursor_byte_index(), 2);

    handle_key(&mut state, key(KeyCode::Home));
    handle_key(&mut state, char_key('>'));
    handle_key(&mut state, key(KeyCode::End));
    handle_key(&mut state, char_key('!'));

    assert_eq!(state.composer_input(), ">abd!");
    assert_eq!(
        state.composer_cursor_byte_index(),
        state.composer_input().len()
    );
}

#[test]
fn paste_inserts_text_while_composing() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(&mut state, "hello\r\nworld"));

    assert_eq!(state.composer_input(), "hello\nworld");
}

#[test]
fn paste_inserts_text_at_composer_cursor() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for value in "helloworld".chars() {
        handle_key(&mut state, char_key(value));
    }
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Left));
    }

    assert!(handle_paste(&mut state, " "));

    assert_eq!(state.composer_input(), "hello world");
    assert_eq!(state.composer_cursor_byte_index(), "hello ".len());
}

#[test]
fn paste_file_path_adds_pending_attachment() {
    let path = temp_upload_file("paste path.txt", b"hello");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(
        &mut state,
        path.to_str().expect("temp path is valid unicode")
    ));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(state.pending_composer_attachments()[0].path, path);
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "paste path.txt"
    );
    assert_eq!(state.pending_composer_attachments()[0].size_bytes, 5);
    remove_temp_upload_file(&path);
}

#[test]
fn paste_single_quoted_file_path_adds_pending_attachment() {
    let path = temp_upload_file("quoted path.txt", b"quoted");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    let pasted = format!("'{}'", path.to_str().expect("temp path is valid unicode"));

    assert!(handle_paste(&mut state, &pasted));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(state.pending_composer_attachments()[0].path, path);
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "quoted path.txt"
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_backslash_escaped_file_path_adds_pending_attachment() {
    let path = temp_upload_file("escaped path.txt", b"escaped");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    let pasted = path
        .to_str()
        .expect("temp path is valid unicode")
        .replace(' ', "\\ ");

    assert!(handle_paste(&mut state, &pasted));

    assert_eq!(state.composer_input(), "");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(state.pending_composer_attachments()[0].path, path);
    assert_eq!(
        state.pending_composer_attachments()[0].filename,
        "escaped path.txt"
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_file_uri_list_can_submit_attachment_only_message() {
    let path = temp_upload_file("uri path.txt", b"upload");
    let uri = format!(
        "x-special/gnome-copied-files\ncopy\nfile://{}",
        path.to_string_lossy().replace(' ', "%20")
    );
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));

    assert!(handle_paste(&mut state, &uri));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.pending_composer_attachments(), &[]);
    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: String::new(),
            reply_to: None,
            attachments: vec![crate::discord::MessageAttachmentUpload {
                path: path.clone(),
                filename: "uri path.txt".to_owned(),
                size_bytes: 6,
            }],
        })
    );
    remove_temp_upload_file(&path);
}

#[test]
fn ctrl_backspace_removes_last_pending_attachment() {
    let first = temp_upload_file("first.txt", b"first");
    let second = temp_upload_file("second.txt", b"second");
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('x'));
    let pasted = format!(
        "{}\n{}",
        first.to_str().expect("temp path is valid unicode"),
        second.to_str().expect("temp path is valid unicode")
    );

    assert!(handle_paste(&mut state, &pasted));
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
    );

    assert_eq!(state.composer_input(), "x");
    assert_eq!(state.pending_composer_attachments().len(), 1);
    assert_eq!(state.pending_composer_attachments()[0].path, first);
    remove_temp_upload_file(&first);
    remove_temp_upload_file(&second);
}

#[test]
fn paste_file_path_while_editing_inserts_text_instead_of_attachment() {
    let path = temp_upload_file("edit paste.txt", b"no attach");
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('e'));

    assert!(handle_paste(
        &mut state,
        path.to_str().expect("temp path is valid unicode")
    ));

    assert!(state.pending_composer_attachments().is_empty());
    assert!(
        state
            .composer_input()
            .contains(path.to_str().expect("temp path is valid unicode"))
    );
    remove_temp_upload_file(&path);
}

#[test]
fn paste_is_ignored_when_not_composing() {
    let mut state = state_with_channel_tree();

    assert!(!handle_paste(&mut state, "hello"));

    assert_eq!(state.composer_input(), "");
}

#[test]
fn enter_submits_multiline_composer() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('h'));
    handle_key(&mut state, shift_enter());
    handle_key(&mut state, char_key('i'));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    // Composer stays open after submit so the user can keep typing
    // back-to-back messages.
    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");
    assert_eq!(
        command,
        Some(crate::discord::AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: "h\ni".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn enter_confirms_emoji_picker_before_submit() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":heart".chars() {
        handle_key(&mut state, char_key(ch));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert_eq!(state.composer_input(), "❤️ ");
    assert!(state.is_composing());
}

#[test]
fn enter_submits_no_match_emoji_query_without_hidden_picker() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":qq".chars() {
        handle_key(&mut state, char_key(ch));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(11),
            content: ":qq".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn tab_confirms_emoji_picker() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":heart".chars() {
        handle_key(&mut state, char_key(ch));
    }

    handle_key(&mut state, key(KeyCode::Tab));

    assert_eq!(state.composer_input(), "❤️ ");
}

#[test]
fn emoji_picker_escape_returns_to_composer_text() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('i'));
    for ch in ":he".chars() {
        handle_key(&mut state, char_key(ch));
    }

    handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(state.composer_input(), ":he");
    assert!(state.is_composing());
}

#[test]
fn options_popup_toggles_selected_setting() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.is_options_popup_open());
    assert!(!state.display_options().show_avatars);
    assert_eq!(
        state.take_display_options_save_request(),
        Some(state.display_options())
    );
}

#[test]
fn options_popup_cycles_image_preview_quality() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    for _ in 0..3 {
        handle_key(&mut state, key(KeyCode::Down));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        state.display_options().image_preview_quality,
        ImagePreviewQualityPreset::High
    );
    assert_eq!(
        state.take_display_options_save_request(),
        Some(state.display_options())
    );
}

#[test]
fn options_popup_esc_closes_popup() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_options_popup_open());
}

#[test]
fn uppercase_h_l_scroll_focused_side_panes_horizontally() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key('L'));
    assert_eq!(state.guild_horizontal_scroll(), 1);

    handle_key(&mut state, char_key('H'));
    handle_key(&mut state, char_key('H'));
    assert_eq!(state.guild_horizontal_scroll(), 0);

    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.channel_horizontal_scroll(), 1);

    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.member_horizontal_scroll(), 1);

    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, char_key('L'));
    assert_eq!(state.member_horizontal_scroll(), 1);
}

#[test]
fn enter_opens_message_action_menu_and_space_opens_leader() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.is_message_action_menu_open());
    state.close_message_action_menu();

    handle_key(&mut state, char_key(' '));

    assert!(state.is_leader_active());
    assert!(!state.is_message_action_menu_open());
}

#[test]
fn mouse_click_selects_message_action_row() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let (column, row) = message_action_row_point(1);

    assert!(handle_mouse(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
    ));

    assert_eq!(state.selected_message_action_index(), Some(1));
}

#[test]
fn mouse_double_click_activates_message_action_row_like_enter() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let mut clicks = MouseClickTracker::default();
    let (column, row) = message_action_row_point(1);

    handle_mouse_event(
        &mut state,
        mouse(MouseEventKind::Down(MouseButton::Left), column, row),
        dashboard_area(),
        &mut clicks,
    );
    handle_mouse_event(
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

    assert_eq!(second.command, None);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_emoji_reaction_picker_open());
}

#[test]
fn mouse_wheel_moves_message_action_selection() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    let (column, row) = message_action_row_point(0);

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

#[test]
fn a_key_no_longer_opens_actions_directly() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key('a'));

    assert!(!state.is_message_action_menu_open());
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn leader_a_p_loads_pinned_messages_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    let command = handle_key(&mut state, char_key('p'));

    assert_eq!(
        command,
        Some(AppCommand::LoadPinnedMessages {
            channel_id: Id::new(2),
        })
    );
    assert!(state.is_pinned_message_view());
    assert!(!state.is_leader_active());
}

#[test]
fn leader_a_opens_selected_channel_actions_from_channel_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Channels);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_channel_leader_action_active());
}

#[test]
fn leader_channel_subphase_esc_returns_to_channel_actions() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Channels);
    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));
    handle_key(&mut state, char_key('t'));
    assert!(state.is_channel_action_threads_phase());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(state.is_leader_action_mode());
    assert!(state.is_channel_leader_action_active());
    assert!(!state.is_channel_action_threads_phase());
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

    assert!(state.is_leader_action_mode());
    assert!(state.is_guild_leader_action_active());
    assert!(!state.is_guild_action_mute_duration_phase());
}

#[test]
fn leader_a_opens_message_actions_from_message_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_message_action_menu_open());
    assert!(!state.is_channel_leader_action_active());
}

#[test]
fn leader_a_opens_server_actions_from_guild_pane() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Guilds);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_guild_leader_action_active());
}

#[test]
fn leader_a_opens_member_actions_from_member_pane() {
    let mut state = state_with_members(1);
    state.focus_pane(FocusPane::Members);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('a'));

    assert!(state.is_leader_action_mode());
    assert!(state.is_member_leader_action_active());
    let actions = state.selected_member_action_items();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].label, "Show profile");
    assert!(actions[0].enabled);
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
    assert!(state.is_user_profile_popup_open());
    assert!(!state.is_leader_active());
}

#[test]
fn enter_opens_selected_forum_post_from_message_pane() {
    let mut state = state_with_forum_channel_posts();
    state.focus_pane(FocusPane::Messages);
    state.move_down();

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(state.selected_channel_id(), Some(Id::new(30)));
    assert_eq!(
        command,
        Some(AppCommand::SubscribeGuildChannel {
            guild_id: Id::new(1),
            channel_id: Id::new(30),
        })
    );
}

#[test]
fn message_action_menu_navigation_is_modal() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    handle_key(&mut state, key(KeyCode::Down));

    assert_eq!(state.selected_message(), 1);
    assert_eq!(
        state.selected_message_action().map(|action| action.kind),
        Some(MessageActionKind::AddReaction)
    );

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_message_action_menu_open());
}

#[test]
fn esc_returns_from_message_opened_thread() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn esc_returns_from_pinned_message_view() {
    let mut state = state_with_messages(3);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Up));
    let expected_selected = state.selected_message();

    state.push_event(AppEvent::MessagePinnedUpdate {
        channel_id: Id::new(2),
        message_id: Id::new(2),
        pinned: true,
    });
    state.enter_pinned_message_view(Id::new(2));
    assert!(state.is_pinned_message_view());

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_pinned_message_view());
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert_eq!(state.selected_message(), expected_selected);
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn esc_closes_modal_before_returning_from_opened_thread() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, char_key('`'));
    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_debug_log_popup_open());
    assert_eq!(state.selected_channel_id(), Some(Id::new(10)));

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
}

#[test]
fn message_action_menu_reply_opens_composer() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_composing());
    assert_eq!(state.composer_input(), "");

    handle_key(&mut state, char_key('h'));
    handle_key(&mut state, char_key('i'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "hi".to_owned(),
            reply_to: Some(Id::new(1)),
            attachments: Vec::new(),
        })
    );
}

#[test]
fn message_action_shortcuts_edit_and_delete_own_message() {
    let mut edit_state = state_with_own_message();
    edit_state.focus_pane(FocusPane::Messages);
    handle_key(&mut edit_state, key(KeyCode::Enter));

    let command = handle_key(&mut edit_state, char_key('e'));

    assert_eq!(command, None);
    assert!(!edit_state.is_message_action_menu_open());
    assert!(edit_state.is_composing());

    let mut delete_state = state_with_own_message();
    delete_state.focus_pane(FocusPane::Messages);
    handle_key(&mut delete_state, key(KeyCode::Enter));

    let command = handle_key(&mut delete_state, char_key('d'));

    assert_eq!(
        command,
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
    assert!(!delete_state.is_message_action_menu_open());
}

#[test]
fn message_action_shortcuts_ignore_control_modified_keys() {
    let mut state = state_with_own_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, ctrl_key('d'));

    assert_eq!(command, None);
    assert!(state.is_message_action_menu_open());
    assert_eq!(state.selected_message_action_index(), Some(0));
}

#[test]
fn canceling_reply_composer_clears_reply_target() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Esc));

    handle_key(&mut state, char_key('i'));
    handle_key(&mut state, char_key('n'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "n".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn message_action_menu_view_image_opens_viewer_and_esc_closes_viewer() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_image_viewer_open());
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, char_key('l'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('k'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, key(KeyCode::Left));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Right));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(2)
    );

    handle_key(&mut state, char_key('h'));
    assert_eq!(
        state.selected_image_viewer_item().map(|item| item.index),
        Some(1)
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_image_viewer_open());
}

#[test]
fn image_viewer_d_shortcut_downloads_image() {
    let mut state = state_with_image_message();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('v'));
    handle_key(&mut state, key(KeyCode::Enter));

    let command = handle_key(&mut state, char_key('d'));

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            url: "https://cdn.discordapp.com/cat.png".to_owned(),
            filename: "cat.png".to_owned(),
            source: DownloadAttachmentSource::ImageViewer,
        })
    );
    assert_eq!(
        state.image_viewer_download_message(),
        Some("Downloading image...")
    );
}

#[test]
fn message_action_menu_add_reaction_opens_emoji_picker() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Down));

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(command, None);
    assert!(!state.is_message_action_menu_open());
    assert!(state.is_emoji_reaction_picker_open());
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("👍".to_owned()))
    );
}

#[test]
fn emoji_picker_selection_returns_reaction_command() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("🎉".to_owned()),
        })
    );
    assert!(!state.is_emoji_reaction_picker_open());
}

#[test]
fn emoji_picker_selection_removes_existing_own_reaction() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(AppEvent::CurrentUserReactionAdd {
        channel_id: Id::new(2),
        message_id: Id::new(1),
        emoji: ReactionEmoji::Unicode("👍".to_owned()),
    });
    open_emoji_picker(&mut state);

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::RemoveReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        })
    );
    assert!(!state.is_emoji_reaction_picker_open());
}

#[test]
fn emoji_picker_number_shortcut_selects_reaction() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    let command = handle_key(&mut state, char_key('2'));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Unicode("❤️".to_owned()),
        })
    );
    assert!(!state.is_emoji_reaction_picker_open());
}

#[test]
fn emoji_picker_slash_filter_matches_name_and_implementation_case_insensitively() {
    let mut state = state_with_custom_emoji_message();
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('T'));
    handle_key(&mut state, char_key('s'));

    assert_eq!(state.emoji_reaction_filter(), Some("Ts"));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Custom {
            id: Id::new(51),
            name: Some("this".to_owned()),
            animated: false,
        })
    );

    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Custom {
                id: Id::new(51),
                name: Some("this".to_owned()),
                animated: false,
            },
        })
    );
}

#[test]
fn emoji_picker_filter_treats_vim_keys_as_text() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('j'));

    assert_eq!(state.emoji_reaction_filter(), Some("j"));
    assert_ne!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );
}

#[test]
fn emoji_picker_filter_matches_remaining_unicode_emojis() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('/'));
    for value in "rocket".chars() {
        handle_key(&mut state, char_key(value));
    }

    assert_eq!(state.emoji_reaction_filter(), Some("rocket"));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("🚀".to_owned()))
    );
}

#[test]
fn emoji_picker_selection_returns_custom_reaction_command() {
    let mut state = state_with_custom_emoji_message();
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    for _ in 0..8 {
        handle_key(&mut state, key(KeyCode::Down));
    }
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::AddReaction {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            emoji: ReactionEmoji::Custom {
                id: Id::new(50),
                name: Some("party".to_owned()),
                animated: false,
            },
        })
    );
}

#[test]
fn emoji_picker_vim_and_arrow_keys_move_selection() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );

    handle_key(&mut state, char_key('j'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("😂".to_owned()))
    );

    handle_key(&mut state, char_key('k'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("👍".to_owned()))
    );

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("❤️".to_owned()))
    );

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(
        state.selected_emoji_reaction().map(|item| item.emoji),
        Some(ReactionEmoji::Unicode("👍".to_owned()))
    );
}

#[test]
fn escape_closes_emoji_picker_without_reacting() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
    open_emoji_picker(&mut state);

    handle_key(&mut state, key(KeyCode::Down));
    let command = handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(command, None);
    assert!(!state.is_emoji_reaction_picker_open());
    assert_eq!(state.selected_message(), 1);
}

#[test]
fn reaction_users_popup_is_modal_and_escape_closes_it() {
    let mut state = state_with_messages(2);
    state.focus_pane(FocusPane::Messages);
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

    handle_key(&mut state, key(KeyCode::Down));

    assert_eq!(state.selected_message(), 1);
    assert!(state.is_reaction_users_popup_open());
    assert_eq!(
        state.reaction_users_popup().map(|popup| popup.scroll()),
        Some(1)
    );

    let command = handle_key(&mut state, key(KeyCode::Esc));

    assert_eq!(command, None);
    assert!(!state.is_reaction_users_popup_open());
}

#[test]
fn multiselect_poll_picker_toggles_and_submits_selected_answers() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Down));
    }

    let command = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(command, None);
    assert!(state.is_poll_vote_picker_open());

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, char_key(' '));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1, 2],
        })
    );
    assert!(!state.is_poll_vote_picker_open());
}

#[test]
fn poll_picker_number_shortcut_toggles_answer() {
    let mut state = state_with_multiselect_poll();
    state.focus_pane(FocusPane::Messages);
    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, char_key('c'));

    handle_key(&mut state, char_key('2'));
    let command = handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1, 2],
        })
    );
}

fn state_with_folder() -> DashboardState {
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    let mut state = DashboardState::new();

    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: name.to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
    }
    state.push_event(AppEvent::GuildFoldersUpdate {
        folders: vec![GuildFolder {
            id: Some(42),
            name: Some("folder".to_owned()),
            color: None,
            guild_ids: vec![first_guild, second_guild],
        }],
    });
    state
}
fn assert_selected_folder_collapsed(state: &DashboardState, expected: bool) {
    let entries = state.guild_pane_entries();
    assert!(matches!(
        entries[1],
        GuildPaneEntry::FolderHeader { collapsed, .. } if collapsed == expected
    ));
}

fn assert_selected_channel_category_collapsed(state: &DashboardState, expected: bool) {
    let entries = state.channel_pane_entries();
    assert!(matches!(
        entries[0],
        ChannelPaneEntry::CategoryHeader { collapsed, .. } if collapsed == expected
    ));
}

#[test]
fn h_l_and_left_right_open_close_tree_nodes() {
    let mut guild_state = state_with_folder();
    guild_state.focus_pane(FocusPane::Guilds);

    handle_key(&mut guild_state, char_key('h'));
    assert_selected_folder_collapsed(&guild_state, true);

    handle_key(&mut guild_state, char_key('l'));
    assert_selected_folder_collapsed(&guild_state, false);

    handle_key(&mut guild_state, key(KeyCode::Left));
    assert_selected_folder_collapsed(&guild_state, true);

    handle_key(&mut guild_state, key(KeyCode::Right));
    assert_selected_folder_collapsed(&guild_state, false);

    let mut channel_state = state_with_channel_tree();
    channel_state.focus_pane(FocusPane::Channels);

    handle_key(&mut channel_state, char_key('h'));
    assert_selected_channel_category_collapsed(&channel_state, true);

    handle_key(&mut channel_state, char_key('l'));
    assert_selected_channel_category_collapsed(&channel_state, false);

    handle_key(&mut channel_state, key(KeyCode::Left));
    assert_selected_channel_category_collapsed(&channel_state, true);

    handle_key(&mut channel_state, key(KeyCode::Right));
    assert_selected_channel_category_collapsed(&channel_state, false);
}

fn state_with_channel_tree() -> DashboardState {
    let guild_id = Id::new(1);
    let category_id = Id::new(10);
    let general_id = Id::new(11);
    let random_id = Id::new(12);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: category_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "Text Channels".to_owned(),
                kind: "category".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: general_id,
                parent_id: Some(category_id),
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: random_id,
                parent_id: Some(category_id),
                position: Some(1),
                last_message_id: None,
                name: "random".to_owned(),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

fn state_with_direct_message(kind: &str) -> DashboardState {
    let channel_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: None,
        channel_id,
        parent_id: None,
        position: None,
        last_message_id: None,
        name: "alice".to_owned(),
        kind: kind.to_owned(),
        message_count: None,
        total_message_sent: None,
        thread_archived: None,
        thread_locked: None,
        thread_pinned: None,
        recipients: Some(vec![ChannelRecipientInfo {
            user_id: Id::new(30),
            display_name: "alice".to_owned(),
            username: None,
            is_bot: false,
            avatar_url: None,
            status: Some(PresenceStatus::Online),
        }]),
        permission_overwrites: Vec::new(),
    }));
    state.confirm_selected_guild();
    state
}

fn state_with_messages(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    for id in 1..=count {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(format!("msg {id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }
    state
}

fn state_with_own_message() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state
}

fn state_with_members(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();
    let members = (1..=count)
        .map(|id| MemberInfo {
            user_id: Id::new(id),
            display_name: format!("member {id}"),
            username: None,
            is_bot: false,
            avatar_url: None,
            role_ids: Vec::new(),
        })
        .collect();
    let presences = (1..=count)
        .map(|id| (Id::new(id), PresenceStatus::Online))
        .collect();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members,
        presences,
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state
}

fn state_with_thread_created_message() -> DashboardState {
    let guild_id = Id::new(1);
    let parent_id = Id::new(2);
    let thread_id = Id::new(10);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: parent_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                parent_id: Some(parent_id),
                position: None,
                last_message_id: None,
                name: "release notes".to_owned(),
                kind: "thread".to_owned(),
                message_count: Some(12),
                total_message_sent: Some(14),
                thread_archived: Some(false),
                thread_locked: Some(false),
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id: parent_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::new(18),
        reference: Some(MessageReferenceInfo {
            guild_id: Some(guild_id),
            channel_id: Some(thread_id),
            message_id: None,
        }),
        reply: None,
        poll: None,
        content: Some("release notes".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_multiselect_poll() -> DashboardState {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: Some(PollInfo {
            question: "Pick foods".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "Soup".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "Noodles".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect: true,
            results_finalized: Some(false),
            total_votes: Some(3),
        }),
        content: Some("msg 1".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_custom_emoji_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: vec![
            CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            },
            CustomEmojiInfo {
                id: Id::new(51),
                name: "this".to_owned(),
                animated: false,
                available: true,
            },
        ],
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some("msg 1".to_owned()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}

fn state_with_forum_channel_posts() -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id: forum_id,
            parent_id: None,
            position: Some(0),
            last_message_id: None,
            name: "announcements".to_owned(),
            kind: "forum".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    // Discord's `/threads/search` returns posts newest-first. Emit them in
    // descending channel-id order so the test sees the same layout.
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 0,
        next_offset: 2,
        posts: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(31),
                parent_id: Some(forum_id),
                position: Some(1),
                last_message_id: None,
                name: "release notes".to_owned(),
                kind: "GuildPublicThread".to_owned(),
                message_count: Some(2),
                total_message_sent: Some(2),
                thread_archived: Some(false),
                thread_locked: Some(false),
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(30),
                parent_id: Some(forum_id),
                position: Some(0),
                last_message_id: None,
                name: "welcome".to_owned(),
                kind: "GuildPublicThread".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_archived: Some(false),
                thread_locked: Some(false),
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            },
        ],
        preview_messages: Vec::new(),
        has_more: false,
    });
    state
}

fn state_with_image_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "general".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::MessageCreate {
        guild_id: Some(guild_id),
        channel_id,
        message_id: Id::new(1),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_role_ids: Vec::new(),
        message_kind: crate::discord::MessageKind::regular(),
        reference: None,
        reply: None,
        poll: None,
        content: Some(String::new()),
        sticker_names: Vec::new(),
        mentions: Vec::new(),
        attachments: vec![
            crate::discord::AttachmentInfo {
                id: Id::new(3),
                filename: "cat.png".to_owned(),
                url: "https://cdn.discordapp.com/cat.png".to_owned(),
                proxy_url: "https://media.discordapp.net/cat.png?format=webp&width=160&height=90"
                    .to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
            },
            crate::discord::AttachmentInfo {
                id: Id::new(4),
                filename: "dog.png".to_owned(),
                url: "https://cdn.discordapp.com/dog.png".to_owned(),
                proxy_url: "https://media.discordapp.net/dog.png".to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 2048,
                width: Some(640),
                height: Some(480),
                description: None,
            },
        ],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
    });
    state
}
fn open_emoji_picker(state: &mut DashboardState) {
    handle_key(state, key(KeyCode::Enter));
    handle_key(state, key(KeyCode::Down));
    handle_key(state, key(KeyCode::Enter));
    assert!(state.is_emoji_reaction_picker_open());
}
