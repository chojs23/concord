use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::discord::MemberModeration;

const ME: Id<UserMarker> = Id::new(10);
const ALICE: Id<UserMarker> = Id::new(20);
const GUILD: Id<GuildMarker> = Id::new(1);
const LOBBY: Id<ChannelMarker> = Id::new(11);
const AFK: Id<ChannelMarker> = Id::new(12);

fn moderation_state(owner: Id<UserMarker>) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(ME),
    });
    state.push_event(crate::discord::test_builders::guild_create_event(
        GuildCreateFixture {
            owner_id: Some(owner),
            channels: vec![
                voice_channel_info(GUILD, LOBBY, "Lobby"),
                ChannelInfo {
                    position: Some(1),
                    ..voice_channel_info(GUILD, AFK, "AFK")
                },
            ],
            members: vec![member_info(ME, "me"), member_info(ALICE, "Alice")],
            roles: vec![role_info(
                Id::new(GUILD.get()),
                "@everyone",
                PERM_VIEW_CHANNEL | PERM_CONNECT,
            )],
            ..GuildCreateFixture::new(GUILD)
        },
    ));
    state.push_event(AppEvent::VoiceStateUpdate {
        state: voice_state(GUILD, Some(LOBBY), ALICE),
    });
    state.push_event(guild_member_list_event(
        GUILD,
        vec![
            GuildMemberListItem::Group {
                id: "online".to_owned(),
                count: 1,
            },
            GuildMemberListItem::Member {
                member: member_info(ALICE, "Alice"),
                presence: None,
            },
        ],
    ));
    state.activate_guild(ActiveGuildScope::Guild(GUILD));
    state
}

fn open_participant_actions(state: &mut DashboardState) {
    state.focus_pane(FocusPane::Channels);
    state.set_channel_view_height(20);
    let row = state
        .channel_pane_entries()
        .iter()
        .position(|entry| matches!(entry, ChannelPaneEntry::VoiceParticipant { .. }))
        .expect("voice participant row");
    assert!(state.select_visible_pane_row(FocusPane::Channels, row));
    state.open_selected_channel_actions();
}

fn press(state: &mut DashboardState, code: KeyCode) -> Option<AppCommand> {
    crate::tui::input::handle_key(state, KeyEvent::new(code, KeyModifiers::NONE))
}

fn moderate(moderation: MemberModeration) -> Option<AppCommand> {
    Some(AppCommand::ModerateMember {
        guild_id: GUILD,
        user_id: ALICE,
        moderation,
    })
}

#[test]
fn voice_participant_menu_offers_moderation_to_permitted_users_only() {
    let mut owner = moderation_state(ME);
    open_participant_actions(&mut owner);
    let kinds: Vec<_> = owner
        .selected_channel_action_items()
        .into_iter()
        .map(|item| item.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            ChannelActionKind::WatchStream,
            ChannelActionKind::ParticipantAudioSettings,
            ChannelActionKind::ServerMute,
            ChannelActionKind::ServerDeafen,
            ChannelActionKind::MoveMember,
            ChannelActionKind::DisconnectMember,
        ]
    );

    let mut member = moderation_state(Id::new(99));
    open_participant_actions(&mut member);
    let kinds: Vec<_> = member
        .selected_channel_action_items()
        .into_iter()
        .map(|item| item.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            ChannelActionKind::WatchStream,
            ChannelActionKind::ParticipantAudioSettings,
        ]
    );
}

#[test]
fn server_mute_toggles_from_current_voice_state() {
    let mut state = moderation_state(ME);
    open_participant_actions(&mut state);
    assert_eq!(
        press(&mut state, KeyCode::Char('M')),
        moderate(MemberModeration::SetMute(true))
    );
    assert_eq!(state.active_modal_popup_kind(), None);

    state.push_event(AppEvent::VoiceStateUpdate {
        state: VoiceStateInfo {
            mute: true,
            deaf: true,
            ..voice_state(GUILD, Some(LOBBY), ALICE)
        },
    });
    open_participant_actions(&mut state);
    let labels: Vec<_> = state
        .selected_channel_action_items()
        .into_iter()
        .map(|item| item.label)
        .collect();
    assert!(labels.contains(&"Server unmute".to_owned()));
    assert!(labels.contains(&"Server undeafen".to_owned()));
    assert_eq!(
        press(&mut state, KeyCode::Char('D')),
        moderate(MemberModeration::SetDeaf(false))
    );
}

#[test]
fn disconnect_member_emits_command_without_confirmation() {
    let mut state = moderation_state(ME);
    open_participant_actions(&mut state);

    assert_eq!(
        press(&mut state, KeyCode::Char('x')),
        moderate(MemberModeration::Disconnect)
    );
    assert_eq!(state.active_modal_popup_kind(), None);
}

#[test]
fn move_member_lists_other_voice_channels_and_moves_to_selection() {
    let mut state = moderation_state(ME);
    open_participant_actions(&mut state);

    assert_eq!(press(&mut state, KeyCode::Char('v')), None);
    assert!(state.is_channel_action_move_target_phase());
    assert_eq!(
        state.selected_member_move_targets(),
        &[(AFK, "AFK".to_owned())]
    );
    assert!(state.back_channel_action_menu());
    assert!(!state.is_channel_action_move_target_phase());

    assert_eq!(press(&mut state, KeyCode::Char('v')), None);
    assert_eq!(
        state.activate_selected_channel_action(),
        moderate(MemberModeration::Move(AFK))
    );
}
