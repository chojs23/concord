use std::collections::BTreeMap;

use crate::{
    AppError, DiscordClient,
    discord::{
        AppEvent, ApplicationCommandAutocompleteInvocation, ApplicationCommandInvocation,
        AttachmentUpdate, ForumPostCreate, MessageAttachmentUpload, MessageInfo,
        MessageUpdateDispatchInfo, MessageUpdateEventFields, ReactionEmoji, ReplyReference,
        ThreadGatewayInfo, ThreadMemberInfo,
        ids::{
            Id,
            marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
        },
    },
};

use super::command_loop::{log_app_error, publish_app_error};

pub(super) async fn send_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
    content: String,
    reply_to: Option<ReplyReference>,
    attachments: Vec<MessageAttachmentUpload>,
) {
    match client
        .send_message(channel_id, nonce, &content, reply_to, &attachments)
        .await
    {
        Ok(mut message) => {
            message.nonce = Some(nonce);
            client.publish_event(message_create_event(message)).await;
        }
        Err(error) => {
            client
                .publish_event(AppEvent::MessageSendFailed { channel_id, nonce })
                .await;
            publish_message_send_error(&client, channel_id, "send message failed", &error).await
        }
    }
}

pub(super) async fn trigger_typing(client: DiscordClient, channel_id: Id<ChannelMarker>) {
    if let Err(error) = client.trigger_typing(channel_id) {
        publish_app_error(&client, "show typing indicator failed", &error).await;
    }
}

pub(super) async fn send_tts_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    nonce: Id<MessageMarker>,
    content: String,
) {
    match client.send_tts_message(channel_id, nonce, &content).await {
        Ok(mut message) => {
            message.nonce = Some(nonce);
            client.publish_event(message_create_event(message)).await;
        }
        Err(error) => {
            client
                .publish_event(AppEvent::MessageSendFailed { channel_id, nonce })
                .await;
            publish_message_send_error(&client, channel_id, "send tts message failed", &error).await
        }
    }
}

pub(super) async fn create_forum_post(client: DiscordClient, post: ForumPostCreate) {
    match client.create_forum_post(&post).await {
        Ok(created) => {
            let slow_mode = client.message_slow_mode(post.channel_id);
            let thread_id = created.thread.channel_id;
            let current_user_member =
                Some(created.current_user_member.unwrap_or(ThreadMemberInfo {
                    thread_id: Some(thread_id),
                    user_id: None,
                    join_timestamp: None,
                    flags: None,
                    muted: None,
                    mute_end_time: None,
                    selected_time_window: None,
                    member: None,
                    presence: None,
                    extra_fields: BTreeMap::new(),
                }));
            client
                .publish_event(AppEvent::ThreadUpsert {
                    thread: ThreadGatewayInfo {
                        channel: created.thread,
                        current_user_member,
                    },
                    created: true,
                })
                .await;
            if let Some(message) = created.first_message {
                client.publish_event(message_create_event(message)).await;
            }
            if let Some(slow_mode) = slow_mode {
                client
                    .publish_event(AppEvent::MessageSendCooldownStarted {
                        channel_id: post.channel_id,
                        duration_millis: u64::try_from(slow_mode.as_millis()).unwrap_or(u64::MAX),
                    })
                    .await;
            }
        }
        Err(error) => {
            publish_message_send_error(&client, post.channel_id, "create forum post failed", &error)
                .await
        }
    }
}

/// The archive/lock/pin/delete results arrive over the gateway
/// (THREAD_UPDATE / THREAD_DELETE), which updates the cached thread, so
/// these only need to report failures.
pub(super) async fn set_thread_archived(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    archived: bool,
    _label: String,
) {
    if let Err(error) = client.set_thread_archived(channel_id, archived).await {
        let context = if archived {
            "archive thread failed"
        } else {
            "reopen thread failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn set_thread_locked(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    locked: bool,
    _label: String,
) {
    if let Err(error) = client.set_thread_locked(channel_id, locked).await {
        let context = if locked {
            "lock thread failed"
        } else {
            "unlock thread failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn set_thread_pinned(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    pinned: bool,
    current_flags: u64,
    _label: String,
) {
    if let Err(error) = client
        .set_thread_pinned(channel_id, pinned, current_flags)
        .await
    {
        let context = if pinned {
            "pin post failed"
        } else {
            "unpin post failed"
        };
        publish_app_error(&client, context, &error).await;
    }
}

pub(super) async fn delete_thread(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    _label: String,
) {
    if let Err(error) = client.delete_thread(channel_id).await {
        publish_app_error(&client, "delete thread failed", &error).await;
    }
}

pub(super) async fn edit_thread(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    name: String,
    applied_tags: Vec<Id<ForumTagMarker>>,
    rate_limit_per_user: u64,
    auto_archive_duration: u64,
    _label: String,
) {
    if let Err(error) = client
        .edit_thread_settings(
            channel_id,
            &name,
            &applied_tags,
            rate_limit_per_user,
            auto_archive_duration,
        )
        .await
    {
        publish_app_error(&client, "edit thread failed", &error).await;
    }
}

pub(super) async fn load_application_commands(
    client: DiscordClient,
    guild_id: Option<Id<GuildMarker>>,
) {
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

pub(super) async fn run_application_command(
    client: DiscordClient,
    invocation: ApplicationCommandInvocation,
) {
    if let Err(error) = client.run_application_command(&invocation).await {
        publish_app_error(&client, "run application command failed", &error).await;
    }
}

pub(super) async fn request_application_command_autocomplete(
    client: DiscordClient,
    invocation: ApplicationCommandAutocompleteInvocation,
) {
    if let Err(error) = client
        .request_application_command_autocomplete(&invocation)
        .await
    {
        log_app_error("application command autocomplete failed", &error);
    }
}

pub(super) async fn edit_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    content: String,
) {
    match client.edit_message(channel_id, message_id, &content).await {
        Ok(message) => {
            client.publish_event(message_update_event(message)).await;
        }
        Err(error) => publish_app_error(&client, "edit message failed", &error).await,
    }
}

pub(super) async fn delete_message(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    match client.delete_message(channel_id, message_id).await {
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
    }
}

pub(super) async fn remove_message_embeds(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
) {
    match client.remove_message_embeds(channel_id, message_id).await {
        Ok(message) => {
            client.publish_event(message_update_event(message)).await;
        }
        Err(error) => publish_app_error(&client, "remove message embeds failed", &error).await,
    }
}

pub(super) async fn leave_guild(client: DiscordClient, guild_id: Id<GuildMarker>, label: String) {
    match client.leave_guild(guild_id).await {
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
    }
}

pub(super) async fn add_reaction(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    match client.add_reaction(channel_id, message_id, &emoji).await {
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
    }
}

pub(super) async fn remove_reaction(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    match client
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
    }
}

pub(super) async fn load_reaction_users(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
    after: Option<Id<UserMarker>>,
) {
    match client
        .load_reaction_users_page(channel_id, message_id, &emoji, after)
        .await
    {
        Ok(page) => {
            client
                .publish_event(AppEvent::ReactionUsersLoaded {
                    channel_id,
                    message_id,
                    emoji,
                    users: page.users,
                    next_after: page.next_after,
                    after,
                })
                .await;
        }
        Err(error) => {
            publish_app_error(&client, "load reaction users failed", &error).await;
            // Clears the popup's in-flight flag so the emoji can be retried.
            client
                .publish_event(AppEvent::ReactionUsersLoadFailed {
                    channel_id,
                    message_id,
                    emoji,
                })
                .await;
        }
    }
}

pub(super) async fn load_pinned_messages(client: DiscordClient, channel_id: Id<ChannelMarker>) {
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

pub(super) async fn set_message_pinned(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    pinned: bool,
) {
    match client
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
    }
}

pub(super) async fn vote_poll(
    client: DiscordClient,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    answer_ids: Vec<u8>,
) {
    match client.vote_poll(channel_id, message_id, &answer_ids).await {
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
    }
}

fn message_create_event(message: MessageInfo) -> AppEvent {
    AppEvent::MessageCreate { message }
}

async fn publish_message_send_error(
    client: &DiscordClient,
    channel_id: Id<ChannelMarker>,
    context: &str,
    error: &AppError,
) {
    let retry_after_millis = match error {
        AppError::DiscordRateLimited {
            retry_after_millis, ..
        }
        | AppError::MessageSlowModeActive { retry_after_millis } => Some(*retry_after_millis),
        _ => None,
    };
    if let Some(retry_after_millis) = retry_after_millis {
        log_app_error(context, error);
        client
            .publish_event(AppEvent::MessageSendRateLimited {
                channel_id,
                retry_after_millis,
            })
            .await;
        return;
    }
    publish_app_error(client, context, error).await;
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
                stickers: Some(message.stickers),
                mentions: Some(message.mentions),
                mention_everyone: Some(message.mention_everyone),
                mention_roles: Some(message.mention_roles),
                flags: Some(message.flags),
                pinned: Some(message.pinned),
                attachments: AttachmentUpdate::Replace(message.attachments),
                embeds: Some(message.embeds),
                edited_timestamp: message.edited_timestamp,
            },
            extra_fields: BTreeMap::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn typing_in_an_uncached_channel_reports_the_block_instead_of_sending() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut effects = client.take_effects();

        trigger_typing(client.clone(), Id::new(1)).await;

        let effect = effects.try_recv().expect("block reason is published");
        let AppEvent::GatewayError { message } = effect.event else {
            panic!("expected a gateway error, got {:?}", effect.event);
        };
        assert!(
            message.contains("show typing indicator failed"),
            "unexpected message: {message}"
        );
    }
}
