use crate::{
    AppError,
    discord::{
        ActivityInfo, AppEvent, ApplicationCommandInvocation, ChannelInfo, ForumPostCreate,
        ForumTagInfo, GuildVerificationLevel, MemberInfo, MentionInfo, MessageAttachmentUpload,
        ReactionEmoji, ReplyReference, RoleInfo, ThreadMetadataInfo, UserProfileInfo, VoiceScope,
        VoiceSoundKind, VoiceStateInfo,
        gateway::GatewayCommand,
        ids::{
            Id,
            marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
        },
        member::MEMBER_FLAG_STARTED_ONBOARDING,
        test_builders::{
            GuildCreateFixture, MessageCreateFixture, MessageHistoryLoadedFixture,
            UserProfileLoadFailedFixture, guild_create_event, guild_message_create_fixture,
            message_create_event as build_message_create_event, message_history_loaded_event,
            user_profile_load_failed_event,
        },
    },
};
use serde_json::{Value, json};

use super::{
    DiscordClient, MEMBER_SEARCH_MAX_LIMIT, MEMBER_SEARCH_MAX_QUERY_CHARS,
    OFFICIAL_WORDLE_APPLICATION_ID, validate_token_header,
};

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
    assert_eq!(snapshot.message, 1);
    assert_eq!(snapshot.detail, 1);
    assert!(matches!(
        effects.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn thread_channel_upsert_is_delivered_as_effect_for_tui_derived_state() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut effects = client.take_effects();
    let mut snapshots = client.subscribe_snapshots();

    client.publish_event(thread_channel_upsert_event()).await;

    snapshots.changed().await.expect("snapshot is published");
    let snapshot = *snapshots.borrow_and_update();
    let effect = effects.recv().await.expect("effect is published");

    assert_eq!(snapshot.global, 1);
    assert_eq!(snapshot.navigation, 1);
    assert_eq!(snapshot.message, 1);
    assert_eq!(snapshot.detail, 1);
    assert_eq!(effect.revision, 1);
    assert!(matches!(effect.event, AppEvent::ChannelUpsert(_)));
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
        AppEvent::ActivateChannel {
            channel_id: Id::new(42),
        },
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

#[test]
fn selected_rich_presence_round_trips_and_clears() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

    assert_eq!(client.selected_rich_presence(), None);
    client.select_rich_presence(Some("client-123".to_owned()));
    assert_eq!(
        client.selected_rich_presence().as_deref(),
        Some("client-123")
    );
    client.select_rich_presence(None);
    assert_eq!(client.selected_rich_presence(), None);
}

#[tokio::test]
async fn requested_voice_state_tracks_shutdown_fallback() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), Some(Id::new(2)), true, false)
        .expect("gateway command should queue");
    let voice = client
        .requested_voice_connection()
        .expect("requested voice state should be tracked");

    assert_eq!(voice.guild_id(), Some(Id::new(1)));
    assert_eq!(voice.channel_id, Id::new(2));
    assert!(voice.self_mute);
    assert!(!voice.self_deaf);

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), None, false, false)
        .expect("gateway command should queue");

    assert_eq!(client.requested_voice_connection(), None);
}

#[tokio::test]
async fn requested_voice_state_skips_duplicate_gateway_updates() {
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

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), Some(Id::new(2)), true, false)
        .expect("duplicate join is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

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

    client
        .update_voice_state(VoiceScope::Guild(Id::new(1)), None, false, false)
        .expect("duplicate leave is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn send_message_rejects_explicit_missing_send_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL).await;

    let error = client
        .send_message(Id::new(2), "hello", None, &[])
        .await
        .expect_err("missing SEND_MESSAGES should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message == "cannot send message in channel"
    ));
}

#[tokio::test]
async fn send_message_rejects_incomplete_onboarding_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | SEND_MESSAGES).await;
    let mut member = permission_fixture_member(Id::new(10));
    member.flags = Some(MEMBER_FLAG_STARTED_ONBOARDING);
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

    let error = client
        .ensure_can_send_message(Id::new(2), None, &[])
        .expect_err("incomplete onboarding should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("server's onboarding")
    ));
}

#[tokio::test]
async fn message_mutation_validators_reject_incomplete_onboarding() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL
            | SEND_MESSAGES
            | READ_MESSAGE_HISTORY
            | ADD_REACTIONS
            | PIN_MESSAGES
            | USE_APPLICATION_COMMANDS,
    )
    .await;
    client
        .publish_event(build_message_create_event(MessageCreateFixture {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(20),
            author_id: Id::new(10),
            ..guild_message_create_fixture()
        }))
        .await;
    publish_incomplete_community_onboarding(&client).await;
    let invocation = ApplicationCommandInvocation {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        command_identity: None,
        command_name: "test".to_owned(),
        content: "/test".to_owned(),
    };

    let errors = [
        client.ensure_can_edit_message(Id::new(2), Id::new(20)),
        client.ensure_can_delete_message(Id::new(2), Id::new(20)),
        client.ensure_can_add_reaction(
            Id::new(2),
            Id::new(20),
            &ReactionEmoji::Unicode("👍".to_owned()),
        ),
        client.ensure_can_remove_current_user_reaction(Id::new(2)),
        client.ensure_can_pin_message(Id::new(2)),
        client.ensure_can_vote_poll(Id::new(2)),
        client.ensure_can_run_application_command(&invocation),
    ];
    for result in errors {
        assert!(matches!(
            result,
            Err(AppError::DiscordRequest(message)) if message.contains("server's onboarding")
        ));
    }
}

#[tokio::test]
async fn voice_join_rejects_incomplete_onboarding_before_gateway_command() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;
    publish_incomplete_community_onboarding(&client).await;

    let error = client
        .update_voice_state(
            VoiceScope::Guild(Id::new(1)),
            Some(Id::new(2)),
            false,
            false,
        )
        .expect_err("incomplete onboarding should stop a voice join");

    assert!(error.contains("server's onboarding"));
}

#[tokio::test]
async fn thread_mutation_validator_rejects_incomplete_onboarding() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | MANAGE_THREADS).await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            name: "thread".to_owned(),
            current_user_joined_thread: Some(true),
            thread_metadata: Some(ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;
    publish_incomplete_community_onboarding(&client).await;

    let error = client
        .ensure_can_manage_thread(Id::new(3), "edit this thread")
        .expect_err("incomplete onboarding should stop a thread mutation");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("server's onboarding")
    ));
}

#[tokio::test]
async fn send_message_rejects_explicit_missing_attach_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | SEND_MESSAGES).await;
    let attachment = MessageAttachmentUpload::from_bytes("note.txt".to_owned(), b"x".to_vec());

    let error = client
        .send_message(Id::new(2), "hello", None, &[attachment])
        .await
        .expect_err("missing ATTACH_FILES should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message == "cannot attach files in channel"
    ));
}

#[tokio::test]
async fn reply_rejects_missing_read_message_history_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | SEND_MESSAGES).await;

    let error = client
        .send_message(
            Id::new(2),
            "hello",
            Some(ReplyReference {
                message_id: Id::new(20),
                mention_author: true,
            }),
            &[],
        )
        .await
        .expect_err("missing READ_MESSAGE_HISTORY should stop a reply before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Read Message History")
    ));
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

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("verify the Discord account email")
    ));
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

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Manage Threads")
    ));
}

#[tokio::test]
async fn thread_message_rejects_missing_send_messages_in_threads() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | SEND_MESSAGES).await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            name: "thread".to_owned(),
            current_user_joined_thread: Some(true),
            thread_metadata: Some(ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;

    let error = client
        .send_message(Id::new(3), "hello", None, &[])
        .await
        .expect_err("missing SEND_MESSAGES_IN_THREADS should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message == "cannot send message in channel"
    ));
}

#[tokio::test]
async fn message_pin_rejects_missing_pin_messages_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL | READ_MESSAGE_HISTORY).await;

    let error = client
        .set_message_pinned(Id::new(2), Id::new(20), true)
        .await
        .expect_err("missing PIN_MESSAGES should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Pin Messages")
    ));
}

#[tokio::test]
async fn external_reaction_rejects_missing_use_external_emojis_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL | READ_MESSAGE_HISTORY | ADD_REACTIONS,
    )
    .await;
    let emoji = ReactionEmoji::Custom {
        id: Id::new(999),
        name: Some("foreign".to_owned()),
        animated: false,
    };

    let error = client
        .add_reaction(Id::new(2), Id::new(20), &emoji)
        .await
        .expect_err("missing USE_EXTERNAL_EMOJIS should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Use External Emoji")
    ));
}

#[tokio::test]
async fn application_command_rejects_missing_channel_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildText", VIEW_CHANNEL).await;
    let invocation = ApplicationCommandInvocation {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        command_identity: None,
        command_name: "test".to_owned(),
        content: "/test".to_owned(),
    };

    let error = client
        .ensure_can_run_application_command(&invocation)
        .expect_err("missing USE_APPLICATION_COMMANDS should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Use Application Commands")
    ));
}

#[tokio::test]
async fn thread_mutation_rejects_manage_channels_without_manage_threads() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL | SEND_MESSAGES_IN_THREADS | MANAGE_CHANNELS,
    )
    .await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            name: "thread".to_owned(),
            current_user_joined_thread: Some(true),
            thread_metadata: Some(ThreadMetadataInfo::test(false, false)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;

    let error = client
        .set_thread_locked(Id::new(3), true)
        .await
        .expect_err("missing MANAGE_THREADS should stop before REST");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("Manage Threads")
    ));
}

#[tokio::test]
async fn archived_thread_rejects_membership_changes_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL | SEND_MESSAGES_IN_THREADS,
    )
    .await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            name: "thread".to_owned(),
            current_user_joined_thread: Some(true),
            thread_metadata: Some(ThreadMetadataInfo::test(true, false)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;

    for joining in [true, false] {
        let error = client
            .ensure_can_change_thread_membership(Id::new(3), joining)
            .expect_err("archived thread membership should not change");
        assert!(matches!(
            error,
            AppError::DiscordRequest(message) if message.contains("archived")
        ));
    }
}

#[tokio::test]
async fn archived_thread_rejects_new_reaction_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(
        &client,
        "GuildText",
        VIEW_CHANNEL | READ_MESSAGE_HISTORY | ADD_REACTIONS,
    )
    .await;
    client
        .publish_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            parent_id: Some(Id::new(2)),
            name: "thread".to_owned(),
            current_user_joined_thread: Some(true),
            thread_metadata: Some(ThreadMetadataInfo::test(true, false)),
            ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
        }))
        .await;

    let error = client
        .ensure_can_add_reaction(
            Id::new(3),
            Id::new(20),
            &ReactionEmoji::Unicode("👍".to_owned()),
        )
        .expect_err("archived thread should reject new reactions");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("archived")
    ));
}

#[test]
fn send_message_guard_rejects_unknown_channels_before_rest() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");

    let error = client
        .ensure_can_send_message(Id::new(99), None, &[])
        .expect_err("unknown channel should fail closed");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message) if message.contains("cannot verify")
    ));
}

#[tokio::test]
async fn voice_join_rejects_explicit_missing_connect_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL).await;
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
        .expect_err("missing CONNECT should stop before gateway command");

    assert_eq!(error, "cannot connect to voice channel");
    assert_eq!(client.requested_voice_connection(), None);
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn microphone_transmit_rejects_missing_speak_permission() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    publish_permission_fixture(&client, "GuildVoice", VIEW_CHANNEL | CONNECT).await;
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
            true,
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .expect_err("missing SPEAK should keep microphone disabled");

    assert!(error.contains("Speak permission"));
    assert!(
        !client
            .requested_voice_connection()
            .expect("voice request")
            .allow_microphone_transmit
    );
}

#[tokio::test]
async fn microphone_transmit_rejects_missing_voice_activity_permission() {
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
        .expect("CONNECT should allow listen-only join");

    let error = client
        .update_voice_capture_permission(
            VoiceScope::Guild(Id::new(1)),
            Id::new(2),
            true,
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .expect_err("missing USE_VOICE_ACTIVITY should keep microphone disabled");

    assert!(error.contains("Use Voice Activity"));
    assert!(
        !client
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
fn application_command_requests_are_deduped_until_loaded() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let guild_id = Some(Id::new(1));

    assert!(client.begin_application_command_request(guild_id));
    assert!(!client.begin_application_command_request(guild_id));

    client.record_application_commands_loaded(guild_id);
    assert!(!client.begin_application_command_request(guild_id));

    let retry_guild_id = Some(Id::new(2));
    assert!(client.begin_application_command_request(retry_guild_id));
    assert!(!client.begin_application_command_request(retry_guild_id));
    client.clear_application_command_request(retry_guild_id);
    assert!(client.begin_application_command_request(retry_guild_id));

    assert!(client.begin_application_command_request(None));
    assert!(!client.begin_application_command_request(None));
    client.record_application_commands_loaded(None);
    assert!(!client.begin_application_command_request(None));
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

    let tui_commands = client.record_application_commands_for_tui(
        guild_id,
        vec![
            command,
            selected_command.clone(),
            third_party_play,
            third_party_wordle,
            discord_play,
            official_wordle,
        ],
    );

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
fn guild_member_search_validates_query_and_caps_limit() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
    let mut gateway_commands = client
        .gateway_commands_rx
        .lock()
        .expect("gateway command receiver mutex is not poisoned")
        .take()
        .expect("gateway commands can be taken once");

    client
        .search_guild_members(Id::new(1), " a ".to_owned(), 10)
        .expect("short search is ignored without closing channel");
    assert!(matches!(
        gateway_commands.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    let long_query = "İ".repeat(MEMBER_SEARCH_MAX_QUERY_CHARS + 10);
    client
        .search_guild_members(Id::new(1), long_query, 99)
        .expect("valid search should queue");

    let command = gateway_commands
        .try_recv()
        .expect("search command should be queued");
    let GatewayCommand::RequestGuildMembers {
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
    assert_eq!(query.chars().count(), MEMBER_SEARCH_MAX_QUERY_CHARS);
    assert_eq!(limit, MEMBER_SEARCH_MAX_LIMIT);
    assert!(presences);
    let nonce = nonce.expect("member search should include nonce");
    assert!(nonce.starts_with("mention-ac-1-"));
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

const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
const READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
const CONNECT: u64 = 0x0000_0000_0010_0000;
const SPEAK: u64 = 0x0000_0000_0020_0000;
const MANAGE_CHANNELS: u64 = 0x0000_0000_0000_0010;
const USE_APPLICATION_COMMANDS: u64 = 0x0000_0000_8000_0000;
const MANAGE_THREADS: u64 = 0x0000_0004_0000_0000;
const PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;
const SEND_MESSAGES_IN_THREADS: u64 = 0x0000_0040_0000_0000;

async fn publish_permission_fixture(
    client: &DiscordClient,
    channel_kind: &str,
    everyone_permissions: u64,
) {
    client
        .publish_event(AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    client
        .publish_event(guild_create_event(GuildCreateFixture {
            member_count: Some(1),
            owner_id: Some(Id::new(99)),
            channels: vec![permission_fixture_channel(
                Id::new(1),
                Id::new(2),
                channel_kind,
            )],
            members: vec![permission_fixture_member(Id::new(10))],
            roles: vec![permission_fixture_role(
                Id::new(1),
                "@everyone",
                everyone_permissions,
            )],
            ..GuildCreateFixture::new(Id::new(1))
        }))
        .await;
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

fn thread_channel_upsert_event() -> AppEvent {
    AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(Id::new(2)),
        name: "new-thread".to_owned(),
        ..ChannelInfo::test(Id::new(3), "GuildPublicThread")
    })
}
