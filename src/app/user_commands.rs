use crate::{
    DiscordClient,
    discord::{AppCommand, AppEvent, PresenceEventFields},
};

use super::command_loop::log_app_error;

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::LoadUserProfile { user_id, guild_id } => {
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
        }
        AppCommand::LoadUserNote { user_id } => {
            let Some(user_id) = client.next_user_note_request(user_id) else {
                return;
            };
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
        AppCommand::UpdateUserProfile { update } => {
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
        AppCommand::UpdateCurrentUserStatus { status } => {
            match client.update_presence_status(status).await {
                Ok(activities) => {
                    if let Some(user_id) = client.current_user_id() {
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
                }
                Err(error) => {
                    log_app_error("update presence status failed", &error);
                    client
                        .publish_event(AppEvent::GatewayError {
                            message: error.to_string(),
                        })
                        .await;
                }
            }
        }
        AppCommand::UpdateCurrentUserActivity { status, activities } => {
            if let Err(error) = client.update_presence_activity(status, activities.clone()) {
                log_app_error("update presence activity failed", &error);
                client
                    .publish_event(AppEvent::GatewayError {
                        message: error.to_string(),
                    })
                    .await;
            } else if let Some(user_id) = client.current_user_id() {
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
        }
        _ => unreachable!("non-user command routed to user handler"),
    }
}
