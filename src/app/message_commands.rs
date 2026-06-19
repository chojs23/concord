use std::collections::BTreeMap;

use crate::{
    DiscordClient,
    discord::{
        AppCommand, AppEvent, AttachmentUpdate, MessageInfo, MessageUpdateDispatchInfo,
        MessageUpdateEventFields, ReactionUsersInfo,
    },
};

use super::command_loop::{log_app_error, publish_app_error};

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::SendMessage {
            channel_id,
            content,
            reply_to,
            attachments,
        } => match client
            .send_message(channel_id, &content, reply_to, &attachments)
            .await
        {
            Ok(message) => client.publish_event(message_create_event(message)).await,
            Err(error) => publish_app_error(&client, "send message failed", &error).await,
        },
        AppCommand::SendTtsMessage {
            channel_id,
            content,
        } => match client.send_tts_message(channel_id, &content).await {
            Ok(message) => client.publish_event(message_create_event(message)).await,
            Err(error) => publish_app_error(&client, "send tts message failed", &error).await,
        },
        AppCommand::LoadApplicationCommands { guild_id } => {
            match client.load_application_commands(guild_id).await {
                Ok(Some(commands)) => {
                    client
                        .publish_event(AppEvent::ApplicationCommandsLoaded { guild_id, commands })
                        .await;
                }
                Ok(None) => {}
                Err(error) => log_app_error("load application commands failed", &error),
            }
        }
        AppCommand::RunApplicationCommand { invocation } => {
            if let Err(error) = client.run_application_command(&invocation).await {
                publish_app_error(&client, "run application command failed", &error).await;
            }
        }
        AppCommand::EditMessage {
            channel_id,
            message_id,
            content,
        } => match client.edit_message(channel_id, message_id, &content).await {
            Ok(message) => {
                client.publish_event(message_update_event(message)).await;
            }
            Err(error) => publish_app_error(&client, "edit message failed", &error).await,
        },
        AppCommand::DeleteMessage {
            channel_id,
            message_id,
        } => match client.delete_message(channel_id, message_id).await {
            Ok(()) => {
                client
                    .publish_event(AppEvent::MessageDelete {
                        guild_id: None,
                        channel_id,
                        message_id,
                    })
                    .await;
            }
            Err(error) => publish_app_error(&client, "delete message failed", &error).await,
        },
        AppCommand::LeaveGuild { guild_id, label } => match client.leave_guild(guild_id).await {
            Ok(()) => {
                client
                    .publish_event(AppEvent::GuildDelete { guild_id })
                    .await;
            }
            Err(error) => {
                log_app_error("leave guild failed", &error);
                client
                    .publish_event(AppEvent::GatewayError {
                        message: format!("leave server {label} failed: {error}"),
                    })
                    .await;
            }
        },
        AppCommand::AddReaction {
            channel_id,
            message_id,
            emoji,
        } => match client.add_reaction(channel_id, message_id, &emoji).await {
            Ok(()) => {
                client
                    .publish_event(AppEvent::CurrentUserReactionAdd {
                        channel_id,
                        message_id,
                        emoji: emoji.clone(),
                    })
                    .await;
            }
            Err(error) => publish_app_error(&client, "add reaction failed", &error).await,
        },
        AppCommand::RemoveReaction {
            channel_id,
            message_id,
            emoji,
        } => match client
            .remove_current_user_reaction(channel_id, message_id, &emoji)
            .await
        {
            Ok(()) => {
                client
                    .publish_event(AppEvent::CurrentUserReactionRemove {
                        channel_id,
                        message_id,
                        emoji: emoji.clone(),
                    })
                    .await;
            }
            Err(error) => publish_app_error(&client, "remove reaction failed", &error).await,
        },
        AppCommand::LoadReactionUsers {
            channel_id,
            message_id,
            reactions,
        } => {
            let mut loaded_reactions = Vec::with_capacity(reactions.len());
            let mut failed = false;
            for emoji in reactions {
                match client
                    .load_reaction_users(channel_id, message_id, &emoji)
                    .await
                {
                    Ok(users) => loaded_reactions.push(ReactionUsersInfo { emoji, users }),
                    Err(error) => {
                        publish_app_error(&client, "load reaction users failed", &error).await;
                        failed = true;
                        break;
                    }
                }
            }
            if !failed {
                client
                    .publish_event(AppEvent::ReactionUsersLoaded {
                        channel_id,
                        message_id,
                        reactions: loaded_reactions,
                    })
                    .await;
            }
        }
        AppCommand::LoadPinnedMessages { channel_id } => {
            match client.load_pinned_messages(channel_id).await {
                Ok(messages) => {
                    client
                        .publish_event(AppEvent::PinnedMessagesLoaded {
                            channel_id,
                            messages,
                        })
                        .await;
                }
                Err(error) => {
                    log_app_error("load pinned messages failed", &error);
                    client
                        .publish_event(AppEvent::PinnedMessagesLoadFailed {
                            channel_id,
                            message: format!("load pinned messages failed: {error}"),
                        })
                        .await;
                }
            }
        }
        AppCommand::SetMessagePinned {
            channel_id,
            message_id,
            pinned,
        } => match client
            .set_message_pinned(channel_id, message_id, pinned)
            .await
        {
            Ok(()) => {
                client
                    .publish_event(AppEvent::MessagePinnedUpdate {
                        channel_id,
                        message_id,
                        pinned,
                    })
                    .await;
            }
            Err(error) => publish_app_error(&client, "set pin failed", &error).await,
        },
        AppCommand::VotePoll {
            channel_id,
            message_id,
            answer_ids,
        } => match client.vote_poll(channel_id, message_id, &answer_ids).await {
            Ok(()) => {
                client
                    .publish_event(AppEvent::CurrentUserPollVoteUpdate {
                        channel_id,
                        message_id,
                        answer_ids,
                    })
                    .await;
            }
            Err(error) => publish_app_error(&client, "poll vote failed", &error).await,
        },
        _ => unreachable!("non-message command routed to message handler"),
    }
}

fn message_create_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageCreate { message }
}

fn message_update_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageUpdateDispatch {
        update: MessageUpdateDispatchInfo {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            fields: MessageUpdateEventFields {
                poll: message.poll,
                content: message.content,
                sticker_names: Some(message.sticker_names),
                mentions: Some(message.mentions),
                mention_everyone: Some(message.mention_everyone),
                mention_roles: Some(message.mention_roles),
                flags: Some(message.flags),
                attachments: AttachmentUpdate::Replace(message.attachments),
                embeds: Some(message.embeds),
                edited_timestamp: message.edited_timestamp,
            },
            extra_fields: BTreeMap::new(),
        },
    }
}
