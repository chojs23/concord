use std::collections::BTreeMap;

use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{
    DiscordClient,
    discord::{
        AppCommand, AppEvent, ChannelNotificationOverrideInfo, GuildNotificationSettingsInfo,
        MuteDuration, UserGuildSettingsInfo,
    },
};

use super::command_loop::publish_app_error;

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::SetGuildMuted {
            guild_id,
            muted,
            duration,
            label: _,
        } => {
            let mute_end_time = mute_end_time_from_duration(duration, muted);
            let selected_time_window = selected_time_window_from_duration(duration, muted);
            match client
                .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
                .await
            {
                Ok(()) => {
                    client
                        .publish_event(AppEvent::UserGuildSettingsUpdate {
                            settings: UserGuildSettingsInfo {
                                notification_settings: guild_notification_settings_update(
                                    &client,
                                    Some(guild_id),
                                    Some((muted, mute_end_time)),
                                    None,
                                ),
                                extra_fields: BTreeMap::new(),
                            },
                        })
                        .await;
                }
                Err(error) => publish_app_error(&client, "set guild mute failed", &error).await,
            }
        }
        AppCommand::SetChannelMuted {
            guild_id,
            channel_id,
            muted,
            duration,
            label: _,
        } => {
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
                    client
                        .publish_event(AppEvent::UserGuildSettingsUpdate {
                            settings: UserGuildSettingsInfo {
                                notification_settings: guild_notification_settings_update(
                                    &client,
                                    guild_id,
                                    None,
                                    Some((channel_id, muted, mute_end_time)),
                                ),
                                extra_fields: BTreeMap::new(),
                            },
                        })
                        .await;
                }
                Err(error) => publish_app_error(&client, "set channel mute failed", &error).await,
            }
        }
        AppCommand::SetThreadMuted {
            guild_id,
            channel_id,
            muted,
            duration,
            label: _,
        } => {
            let mute_end_time = mute_end_time_from_duration(duration, muted);
            let selected_time_window = selected_time_window_from_duration(duration, muted);
            match client
                .set_thread_muted(channel_id, muted, mute_end_time, selected_time_window)
                .await
            {
                Ok(()) => {
                    // The real mute lives on the thread member, but the unread
                    // logic reads channel_overrides, so mirror the same optimistic
                    // override locally to flip the label immediately.
                    client
                        .publish_event(AppEvent::UserGuildSettingsUpdate {
                            settings: UserGuildSettingsInfo {
                                notification_settings: guild_notification_settings_update(
                                    &client,
                                    guild_id,
                                    None,
                                    Some((channel_id, muted, mute_end_time)),
                                ),
                                extra_fields: BTreeMap::new(),
                            },
                        })
                        .await;
                }
                Err(error) => publish_app_error(&client, "set post mute failed", &error).await,
            }
        }
        AppCommand::SetThreadNotificationLevel {
            channel_id,
            flags,
            label: _,
        } => {
            match client
                .set_thread_notification_level(channel_id, flags)
                .await
            {
                Ok(()) => {
                    client
                        .publish_event(AppEvent::ThreadNotificationLevelUpdate {
                            channel_id,
                            flags,
                        })
                        .await;
                }
                Err(error) => {
                    publish_app_error(&client, "set post notifications failed", &error).await;
                }
            }
        }
        AppCommand::SetThreadFollowed {
            channel_id,
            followed,
            label: _,
        } => {
            let result = if followed {
                client.follow_thread(channel_id).await
            } else {
                client.unfollow_thread(channel_id).await
            };
            // Discord echoes a THREAD_MEMBERS_UPDATE for the join/leave, which
            // updates `current_user_joined_thread`, so no optimistic event here.
            if let Err(error) = result {
                let context = if followed {
                    "follow post failed"
                } else {
                    "unfollow post failed"
                };
                publish_app_error(&client, context, &error).await;
            }
        }
        _ => unreachable!("non-notification command routed to notification handler"),
    }
}

fn mute_end_time_from_duration(
    duration: Option<MuteDuration>,
    muted: bool,
) -> Option<chrono::DateTime<Utc>> {
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
    guild_update: Option<(bool, Option<chrono::DateTime<Utc>>)>,
    channel_override: Option<(Id<ChannelMarker>, bool, Option<chrono::DateTime<Utc>>)>,
) -> GuildNotificationSettingsInfo {
    let snapshot = client.current_discord_snapshot();
    let mut settings = snapshot
        .to_state()
        .guild_notification_settings_info(guild_id);
    if let Some((muted, mute_end_time)) = guild_update {
        settings.muted = muted;
        settings.mute_end_time =
            mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    if let Some((channel_id, muted, mute_end_time)) = channel_override {
        if let Some(override_info) = settings
            .channel_overrides
            .iter_mut()
            .find(|override_info| override_info.channel_id == channel_id)
        {
            override_info.muted = muted;
            override_info.mute_end_time =
                mute_end_time.map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true));
        } else {
            settings
                .channel_overrides
                .push(ChannelNotificationOverrideInfo {
                    channel_id,
                    message_notifications: None,
                    muted,
                    mute_end_time: mute_end_time
                        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)),
                });
        }
    }
    settings
}
