use interprocess::local_socket::{tokio::Stream, traits::tokio::Stream as _};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::*;
use crate::discord::{AppEvent, ChannelInfo, ChannelRecipientInfo, Id, VoiceScope, VoiceStateInfo};

#[test]
fn voice_snapshot_describes_the_current_guild_or_private_call() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let self_id = Id::new(9_007_199_254_740_993);
    let other_id = Id::new(30);
    for scope in [VoiceScope::Guild(guild_id), VoiceScope::Private(channel_id)] {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "Self".to_owned(),
            user_id: Some(self_id),
        });
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: scope.guild_id(),
            name: "Call".to_owned(),
            recipients: Some(vec![ChannelRecipientInfo {
                avatar_url: Some("https://cdn.discordapp.com/avatars/30/avatar.png".to_owned()),
                ..ChannelRecipientInfo::test(other_id, "Other")
            }]),
            ..ChannelInfo::test(channel_id, "Group")
        }));
        for user_id in [self_id, other_id] {
            state.apply_event(&AppEvent::VoiceStateUpdate {
                state: VoiceStateInfo {
                    guild_id: scope.guild_id(),
                    session_id: Some("private-session-id".to_owned()),
                    mute: user_id == other_id,
                    deaf: user_id == other_id,
                    self_mute: user_id == other_id,
                    self_deaf: user_id == other_id,
                    self_stream: user_id == other_id,
                    ..VoiceStateInfo::test(guild_id, Some(channel_id), user_id)
                },
            });
        }
        state.apply_event(&AppEvent::VoiceSpeakingUpdate {
            scope,
            channel_id,
            user_id: other_id,
            speaking: true,
        });

        let value = serde_json::to_value(VoiceSnapshot::from_state(&state))
            .expect("voice snapshot serializes");
        assert_eq!(value["v"], 1);
        assert_eq!(value["self_id"], self_id.to_string());
        assert_eq!(value["channel"]["id"], "2");
        assert_eq!(value["channel"]["name"], "Call");
        assert_eq!(value["channel"]["guild_id"], json!(scope.guild_id()));
        let members = value["members"].as_array().expect("members is an array");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["id"], "30");
        assert_eq!(members[1]["id"], self_id.to_string());
        for flag in [
            "speaking",
            "mute",
            "deaf",
            "self_mute",
            "self_deaf",
            "stream",
        ] {
            assert_eq!(members[0][flag], true, "{scope:?}: {flag}");
            assert_eq!(members[1][flag], false, "{scope:?}: {flag}");
        }
        if scope.guild_id().is_none() {
            assert_eq!(members[0]["name"], "Other");
            assert_eq!(
                members[0]["avatar"],
                "https://cdn.discordapp.com/avatars/30/avatar.png"
            );
        }
        assert_eq!(value.as_object().expect("snapshot is an object").len(), 4);
        assert_eq!(
            members[0].as_object().expect("member is an object").len(),
            9
        );
    }
}

#[tokio::test]
async fn voice_export_streams_initial_and_changed_state_and_closes_on_shutdown() {
    let client = DiscordClient::new("test-token".to_owned()).expect("valid test token");
    let (listener, name) = socket::bind_for_test().await.expect("test listener binds");
    let mut tasks = JoinSet::new();
    let server_client = client.clone();
    let (enabled, mut settings) = watch::channel(true);
    tasks.spawn(async move { serve(&server_client, listener, &mut settings).await });
    let stream = Stream::connect(name.clone())
        .await
        .expect("overlay connects");
    let mut reader = BufReader::new(stream);
    assert_eq!(
        read_state(&mut reader).await,
        json!({"v": 1, "self_id": null, "channel": null, "members": []})
    );

    client
        .publish_event(AppEvent::Ready {
            user: "Self".to_owned(),
            user_id: Some(Id::new(10)),
        })
        .await;
    assert_eq!(read_state(&mut reader).await["self_id"], "10");
    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo::test(Id::new(1), Some(Id::new(2)), Id::new(10)),
        })
        .await;
    let joined = read_state(&mut reader).await;
    assert_eq!(joined["channel"]["id"], "2");
    assert_eq!(joined["members"][0]["id"], "10");

    let second = Stream::connect(name)
        .await
        .expect("second overlay connects");
    let mut second = BufReader::new(second);
    assert_eq!(read_state(&mut second).await, joined);
    client
        .publish_event(AppEvent::VoiceSpeakingUpdate {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            user_id: Id::new(10),
            speaking: true,
        })
        .await;
    assert_eq!(
        read_state(&mut reader).await["members"][0]["speaking"],
        true
    );
    assert_eq!(
        read_state(&mut second).await["members"][0]["speaking"],
        true
    );

    client
        .publish_event(AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo::test(Id::new(1), None, Id::new(10)),
        })
        .await;
    assert_eq!(read_state(&mut reader).await["channel"], Value::Null);
    assert_eq!(read_state(&mut second).await["members"], json!([]));

    enabled.send(false).expect("export task observes settings");
    timeout(WRITE_TIMEOUT, tasks.join_next())
        .await
        .expect("disabled export stops promptly")
        .expect("export task exists")
        .expect("export task completes")
        .expect("export stops cleanly");
    let mut line = String::new();
    assert_eq!(
        timeout(WRITE_TIMEOUT, reader.read_line(&mut line))
            .await
            .expect("overlay closes promptly")
            .expect("overlay reads EOF"),
        0
    );
}

#[test]
fn voice_export_coalesces_identical_state_and_keeps_only_the_latest_update() {
    let (updates, mut receiver) = watch::channel(Arc::from(&b"initial\n"[..]));
    publish(&updates, Arc::from(&b"initial\n"[..]));
    assert!(!receiver.has_changed().expect("export is running"));
    publish(&updates, Arc::from(&b"first\n"[..]));
    publish(&updates, Arc::from(&b"second\n"[..]));
    assert_eq!(&**receiver.borrow_and_update(), b"second\n");
}

#[tokio::test]
async fn voice_export_disconnects_clients_whose_writes_stall() {
    let (stream, _stalled_client) = tokio::io::duplex(16);
    let (_updates, receiver) = watch::channel(Arc::from(vec![b'x'; 1024]));
    timeout(
        WRITE_TIMEOUT + Duration::from_secs(1),
        serve_client(stream, receiver),
    )
    .await
    .expect("stalled overlay is disconnected within the write deadline");
}

async fn read_state(reader: &mut BufReader<Stream>) -> Value {
    let mut line = String::new();
    let count = timeout(WRITE_TIMEOUT, reader.read_line(&mut line))
        .await
        .expect("snapshot arrives promptly")
        .expect("snapshot is readable");
    assert!(count > 0);
    assert!(line.ends_with('\n'));
    serde_json::from_str(&line).expect("snapshot is JSON")
}
