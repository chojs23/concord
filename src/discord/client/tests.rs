use std::sync::Arc;

use crate::{
    AppError,
    discord::{
        ActionBlockReason, ActivityInfo, AppEvent, ApplicationCommandInvocation, ChannelInfo,
        DiscordAction, DiscordPermission, ForumPostCreate, ForumTagInfo, GuildBoostTier,
        GuildOnboardingInfo, GuildParticipationDataGap, GuildParticipationRestriction,
        GuildVerificationLevel, MemberInfo, MentionInfo, MessageAttachmentUpload,
        PermissionDataGap, ReactionEmoji, ReplyReference, RoleInfo, StreamCreateInfo,
        StreamUpdateInfo, ThreadGatewayInfo, ThreadMemberInfo, ThreadMetadataInfo, UserProfileInfo,
        VoiceAudioSettings, VoiceScope, VoiceSoundKind, VoiceStateInfo,
        gateway::GatewayCommand,
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
        },
        member::MEMBER_FLAG_STARTED_ONBOARDING,
        test_builders::{
            MessageCreateFixture, MessageHistoryLoadedFixture, UserProfileLoadFailedFixture,
            guild_message_create_fixture, message_create_event as build_message_create_event,
            message_history_loaded_event, user_profile_load_failed_event,
        },
        voice::VoiceRuntimeEvent,
    },
};
use serde_json::{Value, json};

use super::{
    DiscordClient, MEMBER_SEARCH_MAX_LIMIT, OFFICIAL_WORDLE_APPLICATION_ID, validate_token_header,
};

#[tokio::test]
async fn voice_cleanup_is_queued_before_gateway_shutdown() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut voice_events = client
        .voice_events_rx
        .lock()
        .expect("voice event receiver mutex is not poisoned")
        .take()
        .expect("voice event receiver exists");
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway command receiver exists");
    let gateway_commands_tx = client.gateway_commands_tx.clone();
    let task = tokio::spawn(async move {
        while let Some(event) = voice_events.recv().await {
            if matches!(event, VoiceRuntimeEvent::Shutdown) {
                let _ = gateway_commands_tx.send(GatewayCommand::DeleteStream {
                    stream_key: "guild:1:2:3".to_owned(),
                });
                break;
            }
        }
    });
    *client
        .voice_runtime_task
        .lock()
        .expect("voice runtime task mutex is not poisoned") = Some(task);

    client
        .shutdown_voice_runtime()
        .await
        .expect("voice runtime shuts down");
    client
        .shutdown_gateway()
        .expect("gateway shutdown is queued");

    assert!(matches!(
        gateway_commands.recv().await,
        Some(GatewayCommand::DeleteStream { .. })
    ));
    assert!(matches!(
        gateway_commands.recv().await,
        Some(GatewayCommand::Shutdown { .. })
    ));
}

#[tokio::test]
async fn publish_event_sends_matching_snapshot_and_effect_revisions() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client
        .publish_event(message_history_loaded_event(MessageHistoryLoadedFixture {
            channel_id: Id::new(1),
            ..MessageHistoryLoadedFixture::new()
        }))
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("effect is published");
    let state_snapshot = client.current_discord_snapshot();

    assert_eq!(snapshot.global, 1);
    assert_eq!(snapshot.message, 1);
    assert_eq!(snapshot.navigation, 0);
    assert_eq!(snapshot.detail, 0);
    assert_eq!(effect.revision, 1);
    assert_eq!(state_snapshot.revision.global, 1);
    assert_eq!(state_snapshot.revision.message, 1);

    client
        .publish_event(AppEvent::ThreadMemberUpdate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            member: ThreadMemberInfo {
                thread_id: Some(Id::new(2)),
                user_id: None,
                join_timestamp: None,
                flags: Some(9),
                muted: None,
                mute_end_time: None,
                selected_time_window: None,
                member: None,
                presence: None,
                extra_fields: Default::default(),
            },
        })
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();

    assert_eq!(snapshot.global, 2);
    assert_eq!(snapshot.navigation, 2);
    assert_eq!(snapshot.message, 2);
    assert_eq!(snapshot.detail, 0);

    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();

    assert_eq!(snapshot.global, 3);
    assert_eq!(snapshot.navigation, 3);
    assert_eq!(snapshot.message, 2);
    assert_eq!(snapshot.detail, 0);

    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo::test(Id::new(1), Some(Id::new(2)), Id::new(99)),
        })
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();

    assert_eq!(snapshot.global, 4);
    assert_eq!(snapshot.navigation, 4);
    assert_eq!(snapshot.message, 4);
    assert_eq!(snapshot.detail, 0);

    client
        .publish_event(AppEvent::ChannelDelete {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
        })
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();

    assert_eq!(snapshot.global, 5);
    assert_eq!(snapshot.navigation, 5);
    assert_eq!(snapshot.message, 5);
    assert_eq!(snapshot.detail, 0);

    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("effect is published");

    assert_eq!(snapshot.global, 6);
    assert_eq!(snapshot.navigation, 6);
    assert_eq!(snapshot.message, 5);
    assert_eq!(snapshot.detail, 0);
    assert_eq!(effect.revision, 6);
}

#[tokio::test]
async fn message_create_publishes_matching_snapshot_and_effect_revisions() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client.publish_event(message_create_event(1)).await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("effect is published");

    assert_eq!(snapshot.global, 1);
    assert_eq!(snapshot.navigation, 1);
    assert_eq!(snapshot.message, 1);
    assert_eq!(snapshot.detail, 0);
    assert_eq!(effect.revision, 1);
    assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
}

#[tokio::test]
async fn current_user_message_create_advances_detail_revision() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client
        .publish_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(99)),
        })
        .await;
    snapshots
        .changed()
        .await
        .expect("ready snapshot is published");
    drop(snapshots.borrow_and_update());

    client.publish_event(message_create_event(1)).await;

    snapshots
        .changed()
        .await
        .expect("message snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("message effect is published");

    assert_eq!(snapshot.global, 2);
    assert_eq!(snapshot.navigation, 2);
    assert_eq!(snapshot.message, 2);
    assert_eq!(snapshot.detail, 2);
    assert_eq!(effect.revision, 2);
    assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
}

#[tokio::test]
async fn mentioned_message_create_advances_detail_revision() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client
        .publish_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(42)),
        })
        .await;
    snapshots
        .changed()
        .await
        .expect("ready snapshot is published");
    drop(snapshots.borrow_and_update());

    let mut event = message_create_event(1);
    if let AppEvent::MessageCreate { message } = &mut event {
        message.content = Some("hello <@42>".to_owned());
        message
            .mentions
            .push(MentionInfo::test(Id::new(42), "neo".to_owned()));
    }
    client.publish_event(event).await;

    snapshots
        .changed()
        .await
        .expect("message snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("message effect is published");

    assert_eq!(snapshot.global, 2);
    assert_eq!(snapshot.navigation, 2);
    assert_eq!(snapshot.message, 2);
    assert_eq!(snapshot.detail, 2);
    assert_eq!(effect.revision, 2);
    assert!(matches!(effect.event, AppEvent::MessageCreate { .. }));
}

#[tokio::test]
async fn normal_channel_upsert_updates_snapshot_without_effect_delivery() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client.publish_event(channel_upsert_event()).await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();

    assert_eq!(snapshot.global, 1);
    assert_eq!(snapshot.navigation, 1);
    assert_eq!(snapshot.message, 0);
    assert_eq!(snapshot.detail, 0);
    assert!(matches!(
        effects.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn thread_upsert_updates_navigation_and_message_views_with_an_effect() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client
        .publish_event(thread_upsert_event(
            Id::new(3),
            Some(ThreadMetadataInfo::test(false, false)),
            None,
            true,
        ))
        .await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("effect is published");

    assert_eq!(snapshot.global, 1);
    assert_eq!(snapshot.navigation, 1);
    assert_eq!(snapshot.message, 1);
    assert_eq!(snapshot.detail, 0);
    assert_eq!(effect.revision, 1);
    assert!(matches!(effect.event, AppEvent::ThreadUpsert { .. }));
}

#[tokio::test]
async fn concurrent_publishers_emit_ordered_effect_revisions() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    let mut tasks = Vec::new();
    for index in 0..32_u64 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            client
                .publish_event(message_history_loaded_event(MessageHistoryLoadedFixture {
                    channel_id: Id::new(index + 1),
                    ..MessageHistoryLoadedFixture::new()
                }))
                .await;
        }));
    }

    for task in tasks {
        task.await.expect("publish task completes");
    }

    for expected_revision in 1..=32 {
        let effect = effects.recv().await.expect("effect is published");
        assert_eq!(effect.revision, expected_revision);
    }

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    assert_eq!(snapshot.global, 32);
    assert_eq!(snapshot.message, 32);
    assert_eq!(client.current_discord_snapshot().revision.global, 32);
}

#[tokio::test]
async fn effect_only_events_are_delivered_without_snapshots() {
    for event in [
        AppEvent::GatewayError {
            message: "boom".to_owned(),
        },
        AppEvent::GatewayClosed,
    ] {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let snapshots = client.subscribe_snapshots();

        client.publish_event(event.clone()).await;

        let effect = effects.recv().await.expect("effect is published");
        assert_eq!(effect.revision, 0);
        assert_eq!(format!("{:?}", effect.event), format!("{event:?}"));
        assert!(!snapshots.has_changed().expect("snapshot stream is open"));
    }
}

#[tokio::test]
async fn current_user_activities_returns_cached_presence_activity() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let user_id = Id::new(10);
    let activity = ActivityInfo::playing("Concord");

    client
        .publish_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(user_id),
        })
        .await;
    client
        .publish_event(AppEvent::PresenceUpdate {
            guild_id: None,
            presence: crate::discord::PresenceEventFields {
                user_id,
                status: crate::discord::PresenceStatus::Online,
                activities: vec![activity.clone()],
            },
        })
        .await;

    assert_eq!(client.current_user_activities(), vec![activity]);
}

#[tokio::test]
async fn requested_voice_state_tracks_changes_and_skips_duplicate_gateway_updates() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), Some(Id::new(2)), true, false)
        .expect("initial join should queue");
    assert_voice_update(
        &mut gateway_commands,
        Id::new(1),
        Some(Id::new(2)),
        true,
        false,
    );
    let voice = client
        .requested_voice_connection()
        .expect("requested voice state should be tracked");
    assert_eq!(voice.guild_id(), Some(Id::new(1)));
    assert_eq!(voice.channel_id, Id::new(2));
    assert!(voice.self_mute);
    assert!(!voice.self_deaf);

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), Some(Id::new(2)), true, false)
        .expect("duplicate join is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    client
        .request_voice_join(VoiceScope::Guild(Id::new(1)), Id::new(2), true, false)
        .expect("manual join retries the same target");
    assert_voice_update(
        &mut gateway_commands,
        Id::new(1),
        Some(Id::new(2)),
        true,
        false,
    );

    client
        .update_voice_state(
            VoiceScope::Guild(Id::new(1)),
            Some(Id::new(2)),
            false,
            false,
        )
        .expect("mute change should queue");
    assert_voice_update(
        &mut gateway_commands,
        Id::new(1),
        Some(Id::new(2)),
        false,
        false,
    );

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), None, false, false)
        .expect("leave should queue");
    assert_voice_update(&mut gateway_commands, Id::new(1), None, false, false);
    assert_eq!(client.requested_voice_connection(), None);

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), None, false, false)
        .expect("duplicate leave is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn stream_watch_requires_a_live_participant_in_the_current_voice_channel() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                session_id: Some("parent-session".to_owned()),
                ..VoiceStateInfo::test(Id::new(1), Some(Id::new(2)), Id::new(10))
            },
        })
        .await;
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                self_stream: true,
                ..VoiceStateInfo::test(Id::new(1), Some(Id::new(2)), Id::new(99))
            },
        })
        .await;
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");
    let mut voice_events = client
        .voice_events_rx
        .lock()
        .expect("voice event receiver mutex is not poisoned")
        .take()
        .expect("voice events can be taken once");
    while voice_events.try_recv().is_ok() {}

    client
        .request_stream_watch(
            VoiceScope::Guild(Id::new(1)),
            Id::new(2),
            Id::new(99),
            "Streamer".to_owned(),
        )
        .expect("active stream should be watchable");

    assert_eq!(
        gateway_commands.try_recv(),
        Ok(GatewayCommand::WatchStream {
            stream_key: "guild:1:2:99".to_owned(),
        })
    );
    assert!(matches!(
        voice_events.try_recv(),
        Ok(VoiceRuntimeEvent::WatchStreamRequested(request))
            if request.stream_key == "guild:1:2:99"
                && request.owner_id == Id::new(99)
                && request.channel_id == Id::new(2)
    ));
}

#[tokio::test]
async fn send_message_rejects_missing_payload_permissions_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for (permissions, reply, attachments, expected_permission) in [
        (
            VIEW_CHANNEL,
            None,
            Vec::new(),
            DiscordPermission::SendMessages,
        ),
        (
            VIEW_CHANNEL | SEND_MESSAGES,
            None,
            vec![MessageAttachmentUpload::from_bytes(
                "note.txt".to_owned(),
                b"x".to_vec(),
            )],
            DiscordPermission::AttachFiles,
        ),
        (
            VIEW_CHANNEL | SEND_MESSAGES,
            Some(ReplyReference {
                message_id: Id::new(20),
                mention_author: true,
            }),
            Vec::new(),
            DiscordPermission::ReadMessageHistory,
        ),
    ] {
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildText", permissions).await;

        let error = client
            .send_message(Id::new(2), Id::new(99), "hello", reply, &attachments)
            .await
            .expect_err("missing message permission should stop before REST");

        assert_action_blocked_error(
            error,
            DiscordAction::SendMessage,
            ActionBlockReason::PermissionDenied(expected_permission),
        );
    }
}

#[tokio::test]
async fn thread_creator_can_manage_creator_owned_state_without_manage_threads() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL | SEND_MESSAGES_IN_THREADS,
    )
    .await;
    client
        .publish_event(thread_upsert_event(
            Id::new(3),
            Some(ThreadMetadataInfo::test(false, false)),
            Some(Id::new(10)),
            true,
        ))
        .await;

    client
        .ensure_can_manage_thread(Id::new(3), DiscordAction::ArchiveThread)
        .expect("thread creator should be able to archive");
    assert!(
        !client
            .ensure_can_edit_thread_settings(Id::new(3), &[], 0)
            .expect("creator-owned fields should be editable")
    );
    assert_action_blocked(
        client.ensure_can_edit_thread_settings(Id::new(3), &[], 5),
        DiscordAction::EditThread,
        ActionBlockReason::PermissionDenied(DiscordPermission::ManageThreads),
    );

    client
        .publish_event(thread_upsert_event(
            Id::new(3),
            Some(ThreadMetadataInfo::test(true, true)),
            Some(Id::new(10)),
            true,
        ))
        .await;
    client
        .ensure_can_reopen_thread(Id::new(3))
        .expect("thread creator should be able to reopen their locked thread");
}

#[tokio::test]
async fn channel_action_validators_reject_unknown_permission_data() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(AppEvent::ChannelUpsert(permission_fixture_channel(
            Id::new(1),
            Id::new(2),
            "GuildText",
        )))
        .await;
    let invocation = ApplicationCommandInvocation {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        command_identity: None,
        command_name: "test".to_owned(),
        content: "/test".to_owned(),
    };

    let cases = [
        (
            client.ensure_can_read_message_history(Id::new(2)),
            DiscordAction::ReadMessageHistory,
            ActionBlockReason::PermissionDataUnavailable(PermissionDataGap::Guild),
        ),
        (
            client.ensure_can_remove_current_user_reaction(Id::new(2)),
            DiscordAction::RemoveReaction,
            ActionBlockReason::ParticipationDataUnavailable(GuildParticipationDataGap::Guild),
        ),
        (
            client.ensure_can_pin_message(Id::new(2)),
            DiscordAction::PinMessage,
            ActionBlockReason::ParticipationDataUnavailable(GuildParticipationDataGap::Guild),
        ),
        (
            client.ensure_can_vote_poll(Id::new(2)),
            DiscordAction::VotePoll,
            ActionBlockReason::ParticipationDataUnavailable(GuildParticipationDataGap::Guild),
        ),
        (
            client.ensure_can_run_application_command(&invocation),
            DiscordAction::RunApplicationCommand,
            ActionBlockReason::ParticipationDataUnavailable(GuildParticipationDataGap::Guild),
        ),
        (
            client.ensure_can_add_reaction(
                Id::new(2),
                Id::new(20),
                &ReactionEmoji::Unicode("👍".to_owned()),
            ),
            DiscordAction::AddReaction,
            ActionBlockReason::ParticipationDataUnavailable(GuildParticipationDataGap::Guild),
        ),
    ];

    for (result, expected_action, expected_reason) in cases {
        let error = result.expect_err("unknown permission data must fail closed");
        let AppError::DiscordActionBlocked { action, reason } = error else {
            panic!("unexpected error: {error}");
        };
        assert_eq!(action, expected_action);
        assert_eq!(reason, expected_reason);
    }
}

#[tokio::test]
async fn channel_actions_reject_each_missing_guild_authorization_input() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cases = [
        (
            None,
            Some(Vec::new()),
            Some(permission_fixture_roles(VIEW_CHANNEL | SEND_MESSAGES)),
            ActionBlockReason::ParticipationDataUnavailable(
                GuildParticipationDataGap::VerificationLevel,
            ),
        ),
        (
            Some(GuildVerificationLevel::None),
            None,
            Some(permission_fixture_roles(VIEW_CHANNEL | SEND_MESSAGES)),
            ActionBlockReason::ParticipationDataUnavailable(
                GuildParticipationDataGap::GuildFeatures,
            ),
        ),
        (
            Some(GuildVerificationLevel::None),
            Some(Vec::new()),
            None,
            ActionBlockReason::PermissionDataUnavailable(PermissionDataGap::GuildRoles),
        ),
        (
            Some(GuildVerificationLevel::None),
            Some(Vec::new()),
            Some(vec![permission_fixture_role(
                Id::new(50),
                "staff",
                VIEW_CHANNEL | SEND_MESSAGES,
            )]),
            ActionBlockReason::PermissionDataUnavailable(PermissionDataGap::GuildRoles),
        ),
    ];
    for (verification_level, features, roles, expected_reason) in cases {
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_authorization_fixture(
            &client,
            "GuildText",
            verification_level,
            Some(0),
            features,
            roles,
        )
        .await;

        assert_action_blocked(
            client.ensure_can_send_message(Id::new(2), None, &[]),
            DiscordAction::SendMessage,
            expected_reason,
        );
    }

    for (mfa_level, current_user_mfa_enabled, expected_gap) in [
        (None, Some(true), PermissionDataGap::GuildMfaLevel),
        (Some(1), None, PermissionDataGap::CurrentUserMfa),
    ] {
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_authorization_fixture(
            &client,
            "GuildText",
            Some(GuildVerificationLevel::None),
            mfa_level,
            Some(Vec::new()),
            Some(permission_fixture_roles(
                VIEW_CHANNEL | SEND_MESSAGES | MANAGE_MESSAGES,
            )),
        )
        .await;
        client
            .publish_event(AppEvent::CurrentUserVerification {
                email_verified: Some(true),
                phone_verified: Some(true),
                mfa_enabled: current_user_mfa_enabled,
            })
            .await;
        client
            .publish_event(build_message_create_event(MessageCreateFixture {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(2),
                message_id: Id::new(20),
                author_id: Id::new(99),
                ..guild_message_create_fixture()
            }))
            .await;
        assert_action_blocked(
            client.ensure_can_delete_message(Id::new(2), Id::new(20)),
            DiscordAction::DeleteMessage,
            ActionBlockReason::PermissionDataUnavailable(expected_gap),
        );
    }
}

#[tokio::test]
async fn voice_join_fails_closed_before_gateway_command() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(AppEvent::ChannelUpsert(permission_fixture_channel(
            Id::new(1),
            Id::new(2),
            "GuildVoice",
        )))
        .await;
    assert_voice_join_rejected(&client, "server is not loaded");

    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL).await;
    assert_voice_join_rejected(&client, "Connect is required");

    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;
    publish_incomplete_community_onboarding(&client).await;
    assert_voice_join_rejected(&client, "server's onboarding");
}

#[tokio::test]
async fn forum_post_rejects_unmet_guild_verification() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildForum", VIEW_CHANNEL | SEND_MESSAGES).await;
    client
        .publish_event(AppEvent::CurrentUserVerification {
            email_verified: Some(false),
            phone_verified: Some(false),
            mfa_enabled: None,
        })
        .await;
    client
        .publish_event(AppEvent::GuildUpdate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            icon_hash: None,
            owner_id: Some(Id::new(99)),
            boost_tier: None,
            boost_count: None,
            verification_level: Some(GuildVerificationLevel::Low),
            mfa_level: None,
            features: None,
            onboarding: None,
            roles: None,
            emojis: None,
        })
        .await;
    let post = ForumPostCreate {
        channel_id: Id::new(2),
        title: "subject".to_owned(),
        content: "body".to_owned(),
        applied_tags: Vec::new(),
        attachments: Vec::new(),
    };

    let error = client
        .ensure_can_create_forum_post(&post)
        .expect_err("verification should stop forum creation before REST");

    assert_action_blocked_error(
        error,
        DiscordAction::CreateForumPost,
        ActionBlockReason::ParticipationRestricted(
            GuildParticipationRestriction::EmailVerificationRequired,
        ),
    );
}

#[tokio::test]
async fn forum_post_rejects_moderated_tag_without_manage_threads() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildForum", VIEW_CHANNEL | SEND_MESSAGES).await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            name: "guarded".to_owned(),
            available_tags: vec![ForumTagInfo {
                id: Id::new(100),
                name: "Staff only".to_owned(),
                moderated: true,
                emoji_id: None,
                emoji_name: None,
            }],
            ..ChannelInfo::test(Id::new(2), "GuildForum")
        }))
        .await;
    let post = ForumPostCreate {
        channel_id: Id::new(2),
        title: "subject".to_owned(),
        content: "body".to_owned(),
        applied_tags: vec![Id::new(100)],
        attachments: Vec::new(),
    };

    let error = client
        .ensure_can_create_forum_post(&post)
        .expect_err("missing MANAGE_THREADS should reject a moderated tag");

    assert_action_blocked_error(
        error,
        DiscordAction::ApplyModeratedForumTag,
        ActionBlockReason::PermissionDenied(DiscordPermission::ManageThreads),
    );
}

#[test]
fn send_message_guard_rejects_unknown_channels_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

    let error = client
        .ensure_can_send_message(Id::new(99), None, &[])
        .expect_err("unknown channel should fail closed");

    assert_action_blocked_error(
        error,
        DiscordAction::SendMessage,
        ActionBlockReason::ChannelDataUnavailable,
    );
}

#[tokio::test]
async fn microphone_transmit_requires_speak_and_voice_activity_permissions() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for (permissions, expected_error) in [
        (VIEW_CHANNEL | CONNECT, "Speak"),
        (VIEW_CHANNEL | CONNECT | SPEAK, "Use Voice Activity"),
    ] {
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        publish_permission_fixture(&client, "GuildVoice", permissions).await;
        client
            .update_voice_state(
                VoiceScope::Guild(Id::new(1)),
                Some(Id::new(2)),
                false,
                false,
            )
            .expect("CONNECT should allow listen-only join");

        let error = client
            .update_voice_capture_permission(
                VoiceScope::Guild(Id::new(1)),
                Id::new(2),
                VoiceAudioSettings {
                    allow_microphone_transmit: true,
                    ..VoiceAudioSettings::default()
                },
            )
            .expect_err("missing voice permission should keep microphone disabled");

        assert!(error.contains(expected_error));
        assert!(
            !client
                .requested_voice_connection()
                .expect("voice request")
                .allow_microphone_transmit
        );
    }
}

#[tokio::test]
#[cfg(feature = "voice-playback")]
async fn push_to_talk_transmit_does_not_require_voice_activity_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT | SPEAK).await;
    client
        .update_voice_state(
            VoiceScope::Guild(Id::new(1)),
            Some(Id::new(2)),
            false,
            false,
        )
        .expect("CONNECT should allow voice join");
    client.set_push_to_talk_enabled(true);

    client
        .update_voice_capture_permission(
            VoiceScope::Guild(Id::new(1)),
            Id::new(2),
            VoiceAudioSettings {
                allow_microphone_transmit: true,
                ..VoiceAudioSettings::default()
            },
        )
        .expect("push to talk requires SPEAK but not USE_VOICE_ACTIVITY");

    assert!(
        client
            .requested_voice_connection()
            .expect("voice request")
            .allow_microphone_transmit
    );
}

#[tokio::test]
async fn voice_state_update_allows_current_channel_mute_change_without_connect_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL).await;
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                session_id: Some("current-voice-session".to_owned()),
                ..VoiceStateInfo::test(Id::new(1), Some(Id::new(2)), Id::new(10))
            },
        })
        .await;
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), Some(Id::new(2)), true, true)
        .expect("current channel mute and deaf changes should still queue");

    assert_voice_update(
        &mut gateway_commands,
        Id::new(1),
        Some(Id::new(2)),
        true,
        true,
    );
}

#[test]
fn application_command_requests_dedupe_in_flight_but_allow_refresh() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let guild_id = Some(Id::new(1));

    let request_id = client
        .begin_application_command_request(guild_id)
        .expect("first request should start");
    assert!(client.begin_application_command_request(guild_id).is_none());

    assert!(
        client
            .finish_application_command_request(guild_id, request_id, Vec::new())
            .is_some()
    );
    assert!(client.begin_application_command_request(guild_id).is_some());

    let retry_guild_id = Some(Id::new(2));
    let retry_request_id = client
        .begin_application_command_request(retry_guild_id)
        .expect("retry request should start");
    assert!(
        client
            .begin_application_command_request(retry_guild_id)
            .is_none()
    );
    client.clear_application_command_request(retry_guild_id, retry_request_id);
    assert!(
        client
            .begin_application_command_request(retry_guild_id)
            .is_some()
    );

    let direct_request_id = client
        .begin_application_command_request(None)
        .expect("first direct message request should start");
    assert!(client.begin_application_command_request(None).is_none());
    assert!(
        client
            .finish_application_command_request(None, direct_request_id, Vec::new())
            .is_some()
    );
    assert!(client.begin_application_command_request(None).is_some());
}

#[tokio::test]
async fn application_command_index_update_invalidates_in_flight_generation() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let guild = Id::new(1);
    let guild_id = Some(guild);
    let stale_request_id = client
        .begin_application_command_request(guild_id)
        .expect("initial request should start");

    client
        .publish_event(AppEvent::ApplicationCommandIndexUpdated { guild_id: guild })
        .await;

    let fresh_request_id = client
        .begin_application_command_request(guild_id)
        .expect("index invalidation should allow a fresh request");
    client.clear_application_command_request(guild_id, stale_request_id);
    assert!(
        client
            .finish_application_command_request(
                guild_id,
                stale_request_id,
                vec![application_command("stale")],
            )
            .is_none(),
        "an invalidated response must not repopulate the command cache"
    );
    assert!(
        client
            .finish_application_command_request(
                guild_id,
                fresh_request_id,
                vec![application_command("fresh")],
            )
            .is_some()
    );
    let commands = client
        .application_commands
        .lock()
        .expect("application command cache lock is not poisoned");
    assert_eq!(commands[&guild_id][0].name, "fresh");
}

#[test]
fn application_command_metadata_keeps_raw_backend_owned() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let guild_id = Some(Id::new(1));
    let command = application_command("echo");
    let selected_command = application_command_with_ids("echo", "TestBot", 101, 201);
    let third_party_play = application_command_with_ids("play", "MusicBot", 102, 202);
    let third_party_wordle = application_command_with_ids("wordle", "WordleBot", 103, 203);
    let discord_play = application_command_with_ids("play", "Discord", 104, 204);
    let official_wordle =
        application_command_with_ids("wordle", "Wordle", 105, OFFICIAL_WORDLE_APPLICATION_ID);

    let request_id = client
        .begin_application_command_request(guild_id)
        .expect("application command request should start");
    let tui_commands = client
        .finish_application_command_request(
            guild_id,
            request_id,
            vec![
                command,
                selected_command.clone(),
                third_party_play,
                third_party_wordle,
                discord_play,
                official_wordle,
            ],
        )
        .expect("current application command request should finish");

    assert_eq!(tui_commands[0].raw, Value::Null);
    assert_eq!(
        command_sources(&tui_commands),
        vec![
            ("echo", Some("TestBot")),
            ("echo", Some("TestBot")),
            ("play", Some("MusicBot")),
            ("wordle", Some("WordleBot")),
        ]
    );
    let commands = client
        .application_commands
        .lock()
        .expect("application command cache lock is not poisoned");
    let cached_commands = commands.get(&guild_id).expect("backend cache");
    assert_eq!(cached_commands[0].raw["name"], "echo");
    assert_eq!(
        command_sources(cached_commands),
        vec![
            ("echo", Some("TestBot")),
            ("echo", Some("TestBot")),
            ("play", Some("MusicBot")),
            ("wordle", Some("WordleBot")),
        ]
    );
    drop(commands);

    let interaction = client
        .application_command_interaction(&crate::discord::ApplicationCommandInvocation {
            guild_id,
            channel_id: Id::new(2),
            command_identity: Some(selected_command.identity()),
            command_name: "echo".to_owned(),
            content: "/echo".to_owned(),
        })
        .expect("selected command identity should resolve");
    assert_eq!(interaction.command.identity(), selected_command.identity());
}

#[tokio::test]
async fn user_profile_requests_are_gated_by_backend_lifecycle_and_cache() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let user_id = Id::new(10);
    let guild_id = Some(Id::new(1));

    assert_eq!(
        client.next_user_profile_request(user_id, guild_id),
        Some((user_id, guild_id, false))
    );
    assert_eq!(client.next_user_profile_request(user_id, guild_id), None);

    client
        .publish_event(user_profile_load_failed_event(
            UserProfileLoadFailedFixture {
                user_id,
                guild_id,
                message: "temporary failure".to_owned(),
            },
        ))
        .await;
    assert_eq!(
        client.next_user_profile_request(user_id, guild_id),
        Some((user_id, guild_id, false))
    );

    client
        .publish_event(AppEvent::UserProfileLoaded {
            guild_id,
            profile: user_profile(user_id),
        })
        .await;
    assert_eq!(client.next_user_profile_request(user_id, guild_id), None);
}

#[tokio::test]
async fn user_note_requests_are_gated_by_backend_lifecycle_and_cache() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let user_id = Id::new(10);

    assert_eq!(client.next_user_note_request(user_id), Some(user_id));
    assert_eq!(client.next_user_note_request(user_id), None);

    client.mark_user_note_request_failed(user_id);
    assert_eq!(client.next_user_note_request(user_id), Some(user_id));

    client
        .publish_event(AppEvent::UserNoteLoaded {
            user_id,
            note: Some("note".to_owned()),
        })
        .await;
    assert_eq!(client.next_user_note_request(user_id), None);
}

#[test]
fn guild_member_search_queues_a_bounded_private_request() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    client
        .search_guild_members(Id::new(1), "   ".to_owned(), 10)
        .expect("blank search is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    client
        .search_guild_members(Id::new(1), " Private-Query ".to_owned(), 101)
        .expect("valid prefix should queue");
    let command = gateway_commands
        .try_recv()
        .expect("search command should be queued");
    let GatewayCommand::SearchGuildMembers {
        guild_id,
        query,
        limit,
        presences,
        nonce,
    } = command
    else {
        panic!("expected guild member search command");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(query, "private-query");
    assert_eq!(limit, MEMBER_SEARCH_MAX_LIMIT);
    assert!(presences);
    assert!(nonce.starts_with("member-search-"));
    assert!(nonce.len() <= 32);
    assert!(!nonce.contains(&query));
}

#[test]
fn guild_member_request_by_ids_queues_gateway_command() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    client
        .request_guild_members_by_ids(Id::new(1), Vec::new())
        .expect("empty request is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    client
        .request_guild_members_by_ids(Id::new(1), vec![Id::new(20), Id::new(30)])
        .expect("valid request should queue");

    let command = gateway_commands
        .try_recv()
        .expect("member request should be queued");
    let GatewayCommand::RequestGuildMembersByIds {
        guild_id,
        user_ids,
        presences,
    } = command
    else {
        panic!("expected guild member id request command");
    };
    assert_eq!(guild_id, Id::new(1));
    assert_eq!(user_ids, vec![Id::new(20), Id::new(30)]);
    assert!(!presences);
}

#[tokio::test]
async fn requested_voice_state_ignores_observed_other_client_voice() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                session_id: Some("other-client-voice-session".to_owned()),
                ..VoiceStateInfo::test(Id::new(1), Some(Id::new(10)), Id::new(10))
            },
        })
        .await;

    assert_eq!(client.requested_voice_connection(), None);
    assert!(client.current_or_requested_voice_connection().is_some());
}

#[tokio::test]
async fn voice_state_transitions_publish_join_and_leave_sound_effects() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();

    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: voice_state(10, Some(11)),
        })
        .await;
    assert_voice_sound(&mut effects, VoiceSoundKind::Join).await;

    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: voice_state(20, Some(11)),
        })
        .await;
    assert_voice_sound(&mut effects, VoiceSoundKind::Join).await;

    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: voice_state(20, None),
        })
        .await;
    assert_voice_sound(&mut effects, VoiceSoundKind::Leave).await;

    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: voice_state(10, None),
        })
        .await;
    assert_voice_sound(&mut effects, VoiceSoundKind::Leave).await;
}

#[tokio::test]
async fn stream_viewer_updates_publish_stream_sound_effects() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();

    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(20)),
        })
        .await;
    client
        .publish_event(AppEvent::StreamCreate {
            stream: StreamCreateInfo {
                stream_key: "guild:1:10:20".to_owned(),
                rtc_server_id: "100".to_owned(),
                rtc_channel_id: Id::new(101),
                viewer_ids: vec![Id::new(30)],
                paused: false,
            },
        })
        .await;
    client
        .publish_event(AppEvent::StreamUpdate {
            stream: StreamUpdateInfo {
                stream_key: "guild:1:10:20".to_owned(),
                viewer_ids: vec![Id::new(30), Id::new(40)],
                paused: false,
            },
        })
        .await;

    assert_voice_sound(&mut effects, VoiceSoundKind::StreamViewerJoin).await;
}

#[test]
fn validates_token_header_values() {
    validate_token_header("raw-user-token").expect("raw user token must be accepted");
    validate_token_header("invalid\nuser-token")
        .expect_err("newlines are not valid authorization header values");
}

fn message_create_event(message_id: u64) -> AppEvent {
    build_message_create_event(MessageCreateFixture {
        message_id: Id::new(message_id),
        content: Some(format!("msg {message_id}")),
        ..guild_message_create_fixture()
    })
}

fn thread_upsert_event(
    thread_id: Id<ChannelMarker>,
    metadata: Option<ThreadMetadataInfo>,
    owner_id: Option<Id<UserMarker>>,
    joined: bool,
) -> AppEvent {
    AppEvent::ThreadUpsert {
        thread: ThreadGatewayInfo {
            channel: ChannelInfo {
                guild_id: Some(Id::new(1)),
                parent_id: Some(Id::new(2)),
                owner_id,
                name: "thread".to_owned(),
                thread_metadata: metadata,
                ..ChannelInfo::test(thread_id, "GuildPublicThread")
            },
            current_user_member: joined.then(|| ThreadMemberInfo {
                thread_id: Some(thread_id),
                user_id: Some(Id::new(10)),
                join_timestamp: None,
                flags: None,
                muted: Some(false),
                mute_end_time: None,
                selected_time_window: None,
                member: None,
                presence: None,
                extra_fields: Default::default(),
            }),
        },
        created: false,
    }
}

fn assert_action_blocked<T>(
    result: crate::Result<T>,
    expected_action: DiscordAction,
    expected_reason: ActionBlockReason,
) {
    let error = match result {
        Ok(_) => panic!("expected Discord action to be blocked"),
        Err(error) => error,
    };
    assert_action_blocked_error(error, expected_action, expected_reason);
}

fn assert_action_blocked_error(
    error: AppError,
    expected_action: DiscordAction,
    expected_reason: ActionBlockReason,
) {
    let AppError::DiscordActionBlocked { action, reason } = error else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(action, expected_action);
    assert_eq!(reason, expected_reason);
}

const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
const CONNECT: u64 = 0x0000_0000_0010_0000;
const SPEAK: u64 = 0x0000_0000_0020_0000;
const SEND_MESSAGES_IN_THREADS: u64 = 0x0000_0040_0000_0000;

async fn publish_permission_fixture(
    client: &DiscordClient,
    channel_kind: &str,
    everyone_permissions: u64,
) {
    publish_permission_authorization_fixture(
        client,
        channel_kind,
        Some(GuildVerificationLevel::None),
        Some(0),
        Some(Vec::new()),
        Some(permission_fixture_roles(everyone_permissions)),
    )
    .await;
}

async fn publish_permission_authorization_fixture(
    client: &DiscordClient,
    channel_kind: &str,
    verification_level: Option<GuildVerificationLevel>,
    mfa_level: Option<u64>,
    features: Option<Vec<String>>,
    roles: Option<Vec<RoleInfo>>,
) {
    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(AppEvent::GuildCreate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            icon_hash: None,
            member_count: Some(1),
            owner_id: Some(Id::new(99)),
            boost_tier: GuildBoostTier::None,
            boost_count: 0,
            verification_level,
            mfa_level,
            features,
            onboarding: None,
            channels: vec![permission_fixture_channel(
                Id::new(1),
                Id::new(2),
                channel_kind,
            )],
            thread_snapshot_complete: true,
            current_user_thread_members: Vec::new(),
            members: vec![permission_fixture_member(Id::new(10))],
            presences: Vec::new(),
            roles,
            emojis: Vec::new(),
        })
        .await;
}

fn permission_fixture_roles(everyone_permissions: u64) -> Vec<RoleInfo> {
    vec![permission_fixture_role(
        Id::new(1),
        "@everyone",
        everyone_permissions,
    )]
}

async fn publish_incomplete_community_onboarding(client: &DiscordClient) {
    let mut member = permission_fixture_member(Id::new(10));
    member.flags = Some(MEMBER_FLAG_STARTED_ONBOARDING);
    member.pending = Some(false);
    client
        .publish_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member,
        })
        .await;
    client
        .publish_event(AppEvent::GuildUpdate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            icon_hash: None,
            owner_id: None,
            boost_tier: None,
            boost_count: None,
            verification_level: None,
            mfa_level: None,
            features: Some(vec!["COMMUNITY".to_owned()]),
            onboarding: None,
            roles: None,
            emojis: None,
        })
        .await;
    client
        .publish_event(AppEvent::GuildOnboardingUpdate {
            guild_id: Id::new(1),
            onboarding: GuildOnboardingInfo {
                guild_id: Id::new(1),
                enabled: Some(true),
                mode: None,
                default_channel_ids: Vec::new(),
                raw: Arc::new(Value::Null),
            },
        })
        .await;
}

fn permission_fixture_channel(
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    kind: &str,
) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(guild_id),
        position: Some(0),
        name: "guarded".to_owned(),
        ..ChannelInfo::test(channel_id, kind)
    }
}

fn permission_fixture_member(user_id: Id<UserMarker>) -> MemberInfo {
    MemberInfo {
        username: Some("me".to_owned()),
        ..MemberInfo::test(user_id, "me")
    }
}

fn permission_fixture_role(id: Id<RoleMarker>, name: &str, permissions: u64) -> RoleInfo {
    RoleInfo {
        permissions,
        ..RoleInfo::test(id, name)
    }
}

fn user_profile(user_id: Id<UserMarker>) -> UserProfileInfo {
    UserProfileInfo::test(user_id, "neo")
}

fn application_command(name: &str) -> crate::discord::ApplicationCommandInfo {
    application_command_with_app_name(name, "TestBot")
}

fn application_command_with_app_name(
    name: &str,
    application_name: &str,
) -> crate::discord::ApplicationCommandInfo {
    application_command_with_ids(name, application_name, 100, 200)
}

fn application_command_with_ids(
    name: &str,
    application_name: &str,
    command_id: u64,
    application_id: u64,
) -> crate::discord::ApplicationCommandInfo {
    crate::discord::ApplicationCommandInfo {
        application_id: Id::new(application_id),
        version: "1".to_owned(),
        application_name: Some(application_name.to_owned()),
        description: format!("{name} command"),
        raw: json!({
            "id": command_id.to_string(),
            "application_id": application_id.to_string(),
            "version": "1",
            "name": name,
        }),
        ..crate::discord::ApplicationCommandInfo::test(Id::new(command_id), name)
    }
}

fn command_sources(
    commands: &[crate::discord::ApplicationCommandInfo],
) -> Vec<(&str, Option<&str>)> {
    commands
        .iter()
        .map(|command| (command.name.as_str(), command.application_name.as_deref()))
        .collect()
}

fn channel_upsert_event() -> AppEvent {
    AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(Id::new(10)),
        name: "general".to_owned(),
        ..ChannelInfo::test(Id::new(2), "GuildText")
    })
}

fn voice_state(user_id: u64, channel_id: Option<u64>) -> VoiceStateInfo {
    VoiceStateInfo::test(Id::new(1), channel_id.map(Id::new), Id::new(user_id))
}

async fn assert_voice_sound(
    effects: &mut tokio::sync::mpsc::Receiver<crate::discord::SequencedAppEvent>,
    expected: VoiceSoundKind,
) {
    let effect = effects
        .recv()
        .await
        .expect("voice sound effect is published");
    assert!(matches!(effect.event, AppEvent::VoiceSound { kind } if kind == expected));
}

fn assert_voice_update(
    gateway_commands: &mut tokio::sync::mpsc::UnboundedReceiver<GatewayCommand>,
    expected_guild_id: Id<crate::discord::ids::marker::GuildMarker>,
    expected_channel_id: Option<Id<crate::discord::ids::marker::ChannelMarker>>,
    expected_self_mute: bool,
    expected_self_deaf: bool,
) {
    let command = gateway_commands
        .try_recv()
        .expect("voice command should be queued");
    let GatewayCommand::UpdateVoiceState {
        guild_id,
        channel_id,
        self_mute,
        self_deaf,
    } = command
    else {
        panic!("expected voice update command");
    };

    assert_eq!(guild_id, Some(expected_guild_id));
    assert_eq!(channel_id, expected_channel_id);
    assert_eq!(self_mute, expected_self_mute);
    assert_eq!(self_deaf, expected_self_deaf);
}

fn assert_voice_join_rejected(client: &DiscordClient, expected_error: &str) {
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    let error = client
        .update_voice_state(
            VoiceScope::Guild(Id::new(1)),
            Some(Id::new(2)),
            false,
            false,
        )
        .expect_err("blocked voice join should not reach the gateway");

    assert!(error.contains(expected_error), "{error}");
    assert_eq!(client.requested_voice_connection(), None);
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}
