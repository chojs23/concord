use super::*;
use crate::discord::{
    StreamCreateInfo, StreamDeleteInfo, StreamUpdateInfo, VoiceScope, VoiceSoundKind,
};

#[test]
fn stream_presence_lists_broadcaster_first_and_tracks_viewers() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(10);
    let owner_id = Id::new(20);
    let viewer_id = Id::new(30);
    let second_viewer_id = Id::new(40);
    let stream_key = "guild:1:10:20".to_owned();
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_voice_channel(guild_id, channel_id)],
        members: vec![
            member_info(owner_id, "Broadcaster"),
            member_info(viewer_id, "Viewer one"),
            member_info(second_viewer_id, "Viewer two"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    for user_id in [owner_id, viewer_id, second_viewer_id] {
        state.apply_event(&AppEvent::VoiceStateUpdate {
            state: voice_state(guild_id, Some(channel_id), user_id),
        });
    }

    state.apply_event(&AppEvent::StreamCreate {
        stream: StreamCreateInfo {
            stream_key: stream_key.clone(),
            rtc_server_id: "100".to_owned(),
            rtc_channel_id: Id::new(101),
            viewer_ids: vec![viewer_id],
            paused: false,
        },
    });
    state.apply_event(&AppEvent::StreamUpdate {
        stream: StreamUpdateInfo {
            stream_key: stream_key.clone(),
            viewer_ids: vec![viewer_id, second_viewer_id],
            paused: true,
        },
    });

    let stream = state.stream_participants(VoiceScope::Guild(guild_id), channel_id, owner_id);
    assert!(stream.paused);
    assert_eq!(stream.broadcaster, "Broadcaster");
    assert_eq!(
        stream.viewers,
        vec!["Viewer one".to_owned(), "Viewer two".to_owned()]
    );

    state.apply_event(&AppEvent::StreamDelete {
        stream: StreamDeleteInfo {
            stream_key,
            reason: "stream_ended".to_owned(),
            unavailable: false,
        },
    });
    assert_eq!(
        state
            .stream_participants(VoiceScope::Guild(guild_id), channel_id, owner_id)
            .viewers,
        Vec::<String>::new()
    );
}

#[test]
fn stream_update_recovers_presence_when_create_was_missed() {
    let guild_id = Id::new(1);
    let channel_id = Id::new(10);
    let owner_id = Id::new(20);
    let viewer_id = Id::new(30);
    let mut state = DiscordState::default();
    state.apply_event(&guild_create_event(GuildCreateFixture {
        channels: vec![guild_voice_channel(guild_id, channel_id)],
        members: vec![
            member_info(owner_id, "Broadcaster"),
            member_info(viewer_id, "Viewer"),
        ],
        ..GuildCreateFixture::new(guild_id)
    }));

    state.apply_event(&AppEvent::StreamUpdate {
        stream: StreamUpdateInfo {
            stream_key: "guild:1:10:20".to_owned(),
            viewer_ids: vec![viewer_id],
            paused: true,
        },
    });

    let stream = state.stream_participants(VoiceScope::Guild(guild_id), channel_id, owner_id);
    assert!(stream.paused);
    assert_eq!(stream.broadcaster, "Broadcaster");
    assert_eq!(stream.viewers, vec!["Viewer".to_owned()]);
}

#[test]
fn relevant_stream_viewer_changes_emit_join_and_leave_sounds() {
    let current_user_id = Id::new(30);
    let viewer_id = Id::new(40);
    let second_viewer_id = Id::new(50);
    let stream_key = "guild:1:10:20".to_owned();
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::Ready {
        user: "Me".to_owned(),
        user_id: Some(current_user_id),
    });
    state.apply_event(&AppEvent::StreamCreate {
        stream: StreamCreateInfo {
            stream_key: stream_key.clone(),
            rtc_server_id: "100".to_owned(),
            rtc_channel_id: Id::new(101),
            viewer_ids: vec![current_user_id, viewer_id],
            paused: false,
        },
    });

    let joined = StreamUpdateInfo {
        stream_key: stream_key.clone(),
        viewer_ids: vec![current_user_id, viewer_id, second_viewer_id],
        paused: false,
    };
    assert_eq!(
        state.stream_viewer_sounds_for_update(&joined),
        vec![VoiceSoundKind::StreamViewerJoin]
    );
    state.apply_event(&AppEvent::StreamUpdate { stream: joined });

    let left = StreamUpdateInfo {
        stream_key,
        viewer_ids: vec![current_user_id, second_viewer_id],
        paused: false,
    };
    assert_eq!(
        state.stream_viewer_sounds_for_update(&left),
        vec![VoiceSoundKind::StreamViewerLeave]
    );
}
