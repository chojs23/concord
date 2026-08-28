use std::collections::BTreeMap;

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{
    DiscordClient,
    discord::{
        AppEvent, ChannelNotificationOverrideInfo, GuildNotificationSettingsInfo, MuteDuration,
        UserGuildSettingsInfo,
    },
};

use super::command_loop::publish_app_error;

type GuildMuteUpdate = (bool, Option<DateTime<Utc>>, Option<i64>);
type ChannelMuteUpdate = (Id<ChannelMarker>, bool, Option<DateTime<Utc>>, Option<i64>);

pub(super) async fn set_guild_muted(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
        .await
    {
        Ok(()) => {
            publish_settings_update(
                &client,
                Some(guild_id),
                Some((muted, mute_end_time, selected_time_window)),
                None,
            )
            .await;
        }
        Err(error) => publish_app_error(&client, "set guild mute failed", &error).await,
    }
}

pub(super) async fn set_channel_muted(
    client: DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_channel_muted(
            guild_id,
            channel_id,
            muted,
            mute_end_time,
            selected_time_window,
        )
        .await
    {
        Ok(()) => {
            publish_settings_update(
                &client,
                guild_id,
                None,
                Some((channel_id, muted, mute_end_time, selected_time_window)),
            )
            .await;
        }
        Err(error) => publish_app_error(&client, "set channel mute failed", &error).await,
    }
}

pub(super) async fn set_thread_muted(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    muted: bool,
    duration: Option<MuteDuration>,
) {
    let mute_end_time = mute_end_time_from_duration(duration, muted);
    let selected_time_window = selected_time_window_from_duration(duration, muted);
    match client
        .set_thread_muted(channel_id, muted, mute_end_time, selected_time_window)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::ThreadMuteUpdate {
                    channel_id,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|end_time| end_time.to_rfc3339_opts(SecondsFormat::Millis, true)),
                    selected_time_window,
                })
                .await;
        }
        Err(error) => publish_app_error(&client, "set post mute failed", &error).await,
    }
}

pub(super) async fn set_thread_notification_level(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    flags: u64,
) {
    match client
        .set_thread_notification_level(channel_id, flags)
        .await
    {
        Ok(()) => {
            client
                .publish_event(AppEvent::ThreadNotificationLevelUpdate { channel_id, flags })
                .await;
        }
        Err(error) => {
            publish_app_error(&client, "set post notifications failed", &error).await;
        }
    }
}

pub(super) async fn set_thread_followed(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    followed: bool,
) {
    let result = if followed {
        client.follow_thread(channel_id).await
    } else {
        client.unfollow_thread(channel_id).await
    };
    // Discord echoes a THREAD_MEMBERS_UPDATE for the join or leave, which
    // updates the current-user member cache, so no optimistic event is needed.
    if let Err(error) = result {
        let context = if followed {
            "follow post failed"
        } else {
            "unfollow post failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

async fn publish_settings_update(
    client: &DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    guild_update: Option<GuildMuteUpdate>,
    channel_override: Option<ChannelMuteUpdate>,
) {
    client
        .publish_event(AppEvent::UserGuildSettingsUpdate {
            settings: UserGuildSettingsInfo {
                notification_settings: guild_notification_settings_update(
                    client,
                    guild_id,
                    guild_update,
                    channel_override,
                ),
                extra_fields: BTreeMap::new(),
            },
        })
        .await;
}

fn mute_end_time_from_duration(
    duration: Option<MuteDuration>,
    muted: bool,
) -> Option<DateTime<Utc>> {
    if !muted {
        return None;
    }
    duration
        .and_then(MuteDuration::minutes)
        .filter(|minutes| *minutes > 0)
        .and_then(|minutes| i64::try_from(minutes).ok())
        .map(|minutes| Utc::now() + ChronoDuration::minutes(minutes))
}

fn selected_time_window_from_duration(duration: Option<MuteDuration>, muted: bool) -> Option<i64> {
    muted.then(|| {
        duration
            .unwrap_or(MuteDuration::Permanent)
            .selected_time_window_seconds()
    })
}

fn guild_notification_settings_update(
    client: &DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
    guild_update: Option<GuildMuteUpdate>,
    channel_override: Option<ChannelMuteUpdate>,
) -> GuildNotificationSettingsInfo {
    let snapshot = client.current_discord_snapshot();
    let mut settings = snapshot
        .to_state()
        .guild_notification_settings_info(guild_id);
    if let Some((muted, mute_end_time, selected_time_window)) = guild_update {
        settings.muted = muted;
        settings.mute_end_time =
            mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
        settings.selected_time_window = selected_time_window;
    }
    if let Some((channel_id, muted, mute_end_time, selected_time_window)) = channel_override {
        if let Some(override_info) = settings
            .channel_overrides
            .iter_mut()
            .find(|override_info| override_info.channel_id == channel_id)
        {
            override_info.muted = muted;
            override_info.mute_end_time =
                mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
            override_info.selected_time_window = selected_time_window;
        } else {
            settings
                .channel_overrides
                .push(ChannelNotificationOverrideInfo {
                    channel_id,
                    message_notifications: None,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)),
                    selected_time_window,
                    collapsed: false,
                    flags: 0,
                });
        }
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_duration_only_produces_an_end_time_while_muting() {
        // (duration, muted) -> (has end time, selected window)
        let cases = [
            (Some(MuteDuration::Minutes(60)), true, true, Some(3600)),
            (Some(MuteDuration::Permanent), true, false, Some(-1)),
            (None, true, false, Some(-1)),
            (Some(MuteDuration::Minutes(0)), true, false, Some(0)),
            (Some(MuteDuration::Minutes(60)), false, false, None),
            (Some(MuteDuration::Permanent), false, false, None),
        ];

        for (duration, muted, expects_end_time, expected_window) in cases {
            let label = format!("{duration:?} muted={muted}");
            assert_eq!(
                mute_end_time_from_duration(duration, muted).is_some(),
                expects_end_time,
                "{label}"
            );
            assert_eq!(
                selected_time_window_from_duration(duration, muted),
                expected_window,
                "{label}"
            );
        }
    }

    #[test]
    fn optimistic_mute_updates_keep_selected_time_windows() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("test token is valid");
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);

        let settings = guild_notification_settings_update(
            &client,
            Some(guild_id),
            Some((true, None, Some(-1))),
            Some((channel_id, true, None, Some(900))),
        );

        assert!(settings.muted);
        assert_eq!(settings.selected_time_window, Some(-1));
        assert_eq!(settings.channel_overrides.len(), 1);
        assert_eq!(settings.channel_overrides[0].channel_id, channel_id);
        assert!(settings.channel_overrides[0].muted);
        assert_eq!(
            settings.channel_overrides[0].selected_time_window,
            Some(900)
        );
    }
}
