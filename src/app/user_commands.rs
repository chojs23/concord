use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::{
    DiscordClient,
    discord::{
        ActivityInfo, AppEvent, PresenceEventFields, PresenceStatus, UserProfileUpdate,
        UserSettingsInfo,
    },
};

use super::command_loop::log_app_error;

pub(super) async fn load_profile(
    client: DiscordClient,
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
) {
    let profile_request = client.next_user_profile_request(user_id, guild_id);
    let note_request = client.next_user_note_request(user_id);
    if let Some((user_id, guild_id, is_self)) = profile_request {
        match client.load_user_profile(user_id, guild_id, is_self).await {
            Ok(profile) => {
                client
                    .publish_event(AppEvent::UserProfileLoaded { guild_id, profile })
                    .await;
            }
            Err(error) => {
                log_app_error("load user profile failed", &error);
                client
                    .publish_event(AppEvent::UserProfileLoadFailed {
                        user_id,
                        guild_id,
                        message: error.to_string(),
                    })
                    .await;
            }
        }
    }
    if let Some(user_id) = note_request {
        publish_user_note(&client, user_id).await;
    }
}

async fn publish_user_note(client: &DiscordClient, user_id: Id<UserMarker>) {
    match client.load_user_note(user_id).await {
        Ok(note) => {
            client
                .publish_event(AppEvent::UserNoteLoaded { user_id, note })
                .await;
        }
        Err(error) => {
            client.mark_user_note_request_failed(user_id);
            log_app_error("load user note failed", &error);
        }
    }
}

pub(super) async fn update_profile(client: DiscordClient, update: UserProfileUpdate) {
    let user_id = update.user_id;
    let guild_id = update.guild_id;
    if client.current_user_id() != Some(user_id) {
        client
            .publish_event(AppEvent::UserProfileUpdateFailed {
                user_id,
                guild_id,
                message: "profile update can only edit the current user".to_owned(),
            })
            .await;
        return;
    }
    match client.update_user_profile(&update).await {
        Ok(()) => match client.load_user_profile(user_id, guild_id, true).await {
            Ok(profile) => {
                client
                    .publish_event(AppEvent::UserProfileLoaded { guild_id, profile })
                    .await;
            }
            Err(error) => {
                log_app_error("reload user profile after update failed", &error);
                client
                    .publish_event(AppEvent::UserProfileLoadFailed {
                        user_id,
                        guild_id,
                        message: error.to_string(),
                    })
                    .await;
            }
        },
        Err(error) => {
            log_app_error("update user profile failed", &error);
            client
                .publish_event(AppEvent::UserProfileUpdateFailed {
                    user_id,
                    guild_id,
                    message: error.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn update_status(client: DiscordClient, status: PresenceStatus) {
    match client.update_presence_status(status).await {
        Ok(activities) => publish_self_presence(&client, status, activities).await,
        Err(error) => {
            log_app_error("update presence status failed", &error);
            publish_gateway_error(&client, &error).await;
        }
    }
}

pub(super) async fn update_guild_folder_settings(
    client: DiscordClient,
    folder_id: u64,
    name: Option<String>,
    color: Option<u32>,
) {
    match client
        .update_guild_folder_settings(folder_id, name, color)
        .await
    {
        Ok(folders) => {
            client
                .publish_event(AppEvent::UserSettingsUpdate {
                    settings: UserSettingsInfo {
                        guild_folders: Some(folders),
                        ..UserSettingsInfo::default()
                    },
                })
                .await;
        }
        Err(error) => {
            log_app_error("update guild folder settings failed", &error);
            publish_gateway_error(&client, &error).await;
        }
    }
}

pub(super) async fn update_activity(
    client: DiscordClient,
    status: PresenceStatus,
    mut activities: Vec<ActivityInfo>,
    track_client_id: Option<String>,
) {
    client.select_rich_presence(track_client_id);
    for activity in &mut activities {
        client.resolve_activity_external_assets(activity).await;
    }
    if let Err(error) = client.update_presence_activity(status, activities.clone()) {
        log_app_error("update presence activity failed", &error);
        publish_gateway_error(&client, &error).await;
    } else {
        publish_self_presence(&client, status, activities).await;
    }
}

async fn publish_self_presence(
    client: &DiscordClient,
    status: PresenceStatus,
    activities: Vec<ActivityInfo>,
) {
    let Some(user_id) = client.current_user_id() else {
        return;
    };
    client
        .publish_event(AppEvent::PresenceUpdate {
            guild_id: None,
            presence: PresenceEventFields {
                user_id,
                status,
                activities,
            },
        })
        .await;
}

async fn publish_gateway_error(client: &DiscordClient, error: &crate::AppError) {
    client
        .publish_event(AppEvent::GatewayError {
            message: error.to_string(),
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::{GlobalUserProfileUpdate, UserProfileUpdate};

    #[tokio::test]
    async fn profile_update_for_another_user_is_rejected_before_any_request() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();
        let user_id = Id::new(42);

        update_profile(
            client.clone(),
            UserProfileUpdate {
                user_id,
                guild_id: None,
                global: GlobalUserProfileUpdate::default(),
                guild: None,
            },
        )
        .await;

        let effect = effects.try_recv().expect("rejection is published");
        assert!(matches!(
            effect.event,
            AppEvent::UserProfileUpdateFailed {
                user_id: failed_user_id,
                ..
            } if failed_user_id == user_id
        ));
    }
}
