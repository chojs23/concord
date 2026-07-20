use std::time::Duration;

use chrono::{DateTime, Utc};

use super::DiscordClient;
use crate::discord::{
    ActionBlockReason, ActionDecision, ApplicationCommandInvocation, DiscordAction,
    DiscordPermission, ForumPostCreate, GuildFolder, MESSAGE_FLAG_SUPPRESS_EMBEDS,
    MessageAttachmentUpload, MessageInfo, PermissionDecision, ReactionEmoji, ReactionUsersPage,
    ReplyReference, UserProfileInfo, UserProfileUpdate,
    commands::ForumPostArchiveState,
    ids::{
        Id,
        marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
    },
    rest::{CreatedForumPost, ForumPostPage, MessageEditRequest},
};
use crate::{AppError, Result};

impl DiscordClient {
    pub async fn validate_token_authentication(&self) -> Result<()> {
        self.rest.validate_token_authentication().await
    }

    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<ReplyReference>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<MessageInfo> {
        self.ensure_can_send_message(channel_id, reply_to.as_ref(), attachments)?;
        let upload_limit = self.attachment_size_limit(channel_id);
        let slow_mode = self.message_slow_mode(channel_id);
        self.rest
            .send_message(
                channel_id,
                content,
                reply_to,
                attachments,
                upload_limit,
                slow_mode,
            )
            .await
    }

    pub async fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.ensure_can_send_tts_message(channel_id)?;
        let slow_mode = self.message_slow_mode(channel_id);
        self.rest
            .send_tts_message(channel_id, content, slow_mode)
            .await
    }

    pub fn trigger_typing(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::ShowTypingIndicator)?;
        self.rest.spawn_typing(channel_id);
        Ok(())
    }

    pub async fn create_forum_post(&self, post: &ForumPostCreate) -> Result<CreatedForumPost> {
        self.ensure_can_create_forum_post(post)?;
        let upload_limit = self.attachment_size_limit(post.channel_id);
        let slow_mode = self.message_slow_mode(post.channel_id);
        self.rest
            .create_forum_post(post, upload_limit, slow_mode)
            .await
    }

    /// Effective attachment upload limit for `channel_id`, resolved from the
    /// current user's Nitro tier and the channel's guild boost level. Reads a
    /// snapshot of the shared Discord state.
    fn attachment_size_limit(&self, channel_id: Id<ChannelMarker>) -> u64 {
        self.state
            .read()
            .expect("discord state lock is not poisoned")
            .attachment_size_limit(channel_id)
    }

    pub(crate) fn message_slow_mode(&self, channel_id: Id<ChannelMarker>) -> Option<Duration> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id)?;
        let seconds = channel.rate_limit_per_user.filter(|seconds| *seconds > 0)?;
        (!state.bypasses_slow_mode(channel)).then(|| Duration::from_secs(seconds))
    }

    pub(super) fn ensure_can_send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        reply_to: Option<&ReplyReference>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::SendMessage,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::SendMessage)?;
        if reply_to.is_some() {
            ensure_permission(
                &state,
                channel,
                DiscordAction::SendMessage,
                DiscordPermission::ReadMessageHistory,
            )?;
        }
        if !attachments.is_empty() {
            ensure_permission(
                &state,
                channel,
                DiscordAction::SendMessage,
                DiscordPermission::AttachFiles,
            )?;
        }
        Ok(())
    }

    pub(super) fn ensure_can_create_forum_post(&self, post: &ForumPostCreate) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(post.channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::CreateForumPost,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        if !channel.is_forum() {
            return Err(AppError::DiscordRequest(
                "cannot create forum post outside a forum channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::CreateForumPost)?;
        if !post.attachments.is_empty() {
            ensure_permission(
                &state,
                channel,
                DiscordAction::CreateForumPost,
                DiscordPermission::AttachFiles,
            )?;
        }
        if channel.requires_forum_tag() && post.applied_tags.is_empty() {
            return Err(AppError::DiscordRequest(
                "forum post requires a tag".to_owned(),
            ));
        }
        if !post.applied_tags.is_empty()
            && post
                .applied_tags
                .iter()
                .any(|tag_id| !channel.available_tags.iter().any(|tag| tag.id == *tag_id))
        {
            return Err(AppError::DiscordRequest(
                "forum post includes an unknown tag".to_owned(),
            ));
        }
        if post.applied_tags.iter().any(|tag_id| {
            channel
                .available_tags
                .iter()
                .any(|tag| tag.id == *tag_id && tag.moderated)
        }) {
            ensure_permission(
                &state,
                channel,
                DiscordAction::ApplyModeratedForumTag,
                DiscordPermission::ManageThreads,
            )?;
        }
        Ok(())
    }

    pub(super) fn ensure_can_send_tts_message(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::SendTtsMessage,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::SendTtsMessage)
    }

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.ensure_can_edit_message(channel_id, message_id)?;
        self.rest
            .edit_message(channel_id, message_id, MessageEditRequest::Content(content))
            .await
    }

    pub async fn remove_message_embeds(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<MessageInfo> {
        let flags = {
            let state = self
                .state
                .read()
                .expect("discord state lock is not poisoned");
            let channel = state.channel(channel_id).ok_or_else(|| {
                action_blocked(
                    DiscordAction::RemoveMessageEmbeds,
                    ActionBlockReason::ChannelDataUnavailable,
                )
            })?;
            ensure_channel_action_policy(&state, channel, DiscordAction::RemoveMessageEmbeds)?;
            let message = state
                .messages_for_channel(channel_id)
                .into_iter()
                .find(|message| message.id == message_id)
                .ok_or_else(|| {
                    AppError::DiscordRequest(format!(
                        "message {} was not found in channel {}",
                        message_id.get(),
                        channel_id.get()
                    ))
                })?;
            let is_author = Some(message.author_id) == state.current_user_id();
            if !is_author {
                ensure_permission(
                    &state,
                    channel,
                    DiscordAction::RemoveMessageEmbeds,
                    DiscordPermission::ManageMessages,
                )?;
            }
            message.flags | MESSAGE_FLAG_SUPPRESS_EMBEDS
        };
        self.rest
            .edit_message(channel_id, message_id, MessageEditRequest::Flags(flags))
            .await
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.ensure_can_delete_message(channel_id, message_id)?;
        self.rest.delete_message(channel_id, message_id).await
    }

    pub async fn leave_guild(&self, guild_id: Id<GuildMarker>) -> Result<()> {
        self.rest.leave_guild(guild_id).await
    }

    pub async fn ack_channel(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.rest.ack_channel(channel_id, message_id).await
    }

    pub async fn set_guild_muted(
        &self,
        guild_id: Id<GuildMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_guild_muted(guild_id, muted, mute_end_time, selected_time_window)
            .await
    }

    pub async fn update_guild_folder_settings(
        &self,
        folder_id: u64,
        name: Option<String>,
        color: Option<u32>,
    ) -> Result<Vec<GuildFolder>> {
        let mut folders = self
            .state
            .read()
            .expect("discord state lock is not poisoned")
            .guild_folders()
            .to_vec();
        let Some(folder) = folders
            .iter_mut()
            .find(|folder| folder.id == Some(folder_id))
        else {
            return Err(AppError::DiscordRequest(format!(
                "guild folder {folder_id} was not found"
            )));
        };
        folder.name = name;
        folder.color = color;
        self.rest.update_guild_folders(&folders).await?;
        Ok(folders)
    }

    pub async fn set_channel_muted(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_channel_muted(
                guild_id,
                channel_id,
                muted,
                mute_end_time,
                selected_time_window,
            )
            .await
    }

    pub async fn set_thread_notification_level(
        &self,
        thread_id: Id<ChannelMarker>,
        flags: u64,
    ) -> Result<()> {
        self.rest
            .set_thread_notification_level(thread_id, flags)
            .await
    }

    pub async fn set_thread_muted(
        &self,
        thread_id: Id<ChannelMarker>,
        muted: bool,
        mute_end_time: Option<DateTime<Utc>>,
        selected_time_window: Option<i64>,
    ) -> Result<()> {
        self.rest
            .set_thread_muted(thread_id, muted, mute_end_time, selected_time_window)
            .await
    }

    pub async fn follow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_change_thread_membership(thread_id, true)?;
        self.rest.follow_thread(thread_id).await
    }

    pub async fn unfollow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_change_thread_membership(thread_id, false)?;
        self.rest.unfollow_thread(thread_id).await
    }

    pub async fn set_thread_archived(
        &self,
        thread_id: Id<ChannelMarker>,
        archived: bool,
    ) -> Result<()> {
        if archived {
            self.ensure_can_manage_thread(thread_id, DiscordAction::ArchiveThread)?;
        } else {
            self.ensure_can_reopen_thread(thread_id)?;
        }
        self.rest.set_thread_archived(thread_id, archived).await
    }

    pub async fn set_thread_locked(
        &self,
        thread_id: Id<ChannelMarker>,
        locked: bool,
    ) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::ChangeThreadLock)?;
        self.rest.set_thread_locked(thread_id, locked).await
    }

    pub async fn set_thread_pinned(
        &self,
        thread_id: Id<ChannelMarker>,
        pinned: bool,
        current_flags: u64,
    ) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::PinForumPost)?;
        self.rest
            .set_thread_pinned(thread_id, pinned, current_flags)
            .await
    }

    pub async fn delete_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_can_manage_thread(thread_id, DiscordAction::DeleteThread)?;
        self.rest.delete_thread(thread_id).await
    }

    pub async fn edit_thread_settings(
        &self,
        thread_id: Id<ChannelMarker>,
        name: &str,
        applied_tags: &[Id<ForumTagMarker>],
        rate_limit_per_user: u64,
        auto_archive_duration: u64,
    ) -> Result<()> {
        let can_manage_threads =
            self.ensure_can_edit_thread_settings(thread_id, applied_tags, rate_limit_per_user)?;
        self.rest
            .edit_thread_settings(
                thread_id,
                name,
                applied_tags,
                can_manage_threads.then_some(rate_limit_per_user),
                auto_archive_duration,
            )
            .await
    }

    pub(super) fn ensure_can_edit_thread_settings(
        &self,
        thread_id: Id<ChannelMarker>,
        applied_tags: &[Id<ForumTagMarker>],
        rate_limit_per_user: u64,
    ) -> Result<bool> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(thread_id).ok_or_else(|| {
            action_blocked(
                DiscordAction::EditThread,
                ActionBlockReason::ChannelDataUnavailable,
            )
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread editing requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::EditThread)?;

        let manage_threads_decision =
            state.channel_permission_decision(channel, DiscordPermission::ManageThreads);
        let can_manage_threads = matches!(manage_threads_decision, PermissionDecision::Allowed);
        let rate_limit_changed = channel.rate_limit_per_user.unwrap_or(0) != rate_limit_per_user;
        if !can_manage_threads
            && (rate_limit_changed || changed_moderated_thread_tags(&state, channel, applied_tags))
        {
            let reason = match manage_threads_decision {
                PermissionDecision::Allowed => unreachable!("manage permission was not allowed"),
                PermissionDecision::Denied(permission) => {
                    ActionBlockReason::PermissionDenied(permission)
                }
                PermissionDecision::Unavailable(gap) => {
                    ActionBlockReason::PermissionDataUnavailable(gap)
                }
            };
            return Err(action_blocked(DiscordAction::EditThread, reason));
        }
        Ok(can_manage_threads)
    }

    pub async fn ack_channels(
        &self,
        targets: &[(Id<ChannelMarker>, Id<MessageMarker>)],
    ) -> Result<()> {
        self.rest.ack_channels(targets).await
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history(channel_id, before, limit)
            .await
    }

    pub async fn load_message_history_around(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history_around(channel_id, message_id, limit)
            .await
    }

    pub async fn load_recent_mentions(
        &self,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest.load_recent_mentions(before, limit).await
    }

    pub async fn delete_recent_mention(&self, message_id: Id<MessageMarker>) -> Result<()> {
        self.rest.delete_recent_mention(message_id).await
    }

    pub async fn load_message_history_after(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_message_history_after(channel_id, message_id, limit)
            .await
    }

    pub async fn search_messages(
        &self,
        query: crate::discord::MessageSearchQuery,
    ) -> Result<crate::discord::MessageSearchPage> {
        if let Some(channel_id) = query.channel_id {
            self.ensure_can_read_message_history(channel_id)?;
        }
        self.rest.search_messages(query).await
    }

    pub async fn load_forum_posts(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) -> Result<ForumPostPage> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_forum_posts(guild_id, channel_id, archive_state, offset)
            .await
    }

    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.ensure_can_add_reaction(channel_id, message_id, emoji)?;
        self.rest.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.ensure_can_remove_current_user_reaction(channel_id)?;
        self.rest
            .remove_current_user_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_reaction_users_page(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
        after: Option<Id<UserMarker>>,
    ) -> Result<ReactionUsersPage> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest
            .load_reaction_users_page(channel_id, message_id, emoji, after)
            .await
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        self.ensure_can_read_message_history(channel_id)?;
        self.rest.load_pinned_messages(channel_id).await
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        self.ensure_can_pin_message(channel_id)?;
        self.rest
            .set_message_pinned(channel_id, message_id, pinned)
            .await
    }

    pub async fn vote_poll(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) -> Result<()> {
        self.ensure_can_vote_poll(channel_id)?;
        self.rest
            .vote_poll(channel_id, message_id, answer_ids)
            .await
    }

    pub async fn load_user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        is_self: bool,
    ) -> Result<UserProfileInfo> {
        self.rest
            .load_user_profile(user_id, guild_id, is_self)
            .await
    }

    pub async fn load_user_note(&self, user_id: Id<UserMarker>) -> Result<Option<String>> {
        self.rest.load_user_note(user_id).await
    }

    pub async fn update_user_profile(&self, update: &UserProfileUpdate) -> Result<()> {
        self.rest.update_user_profile(update).await
    }

    pub(super) fn ensure_can_run_application_command(
        &self,
        invocation: &ApplicationCommandInvocation,
    ) -> Result<()> {
        self.ensure_channel_action(invocation.channel_id, DiscordAction::RunApplicationCommand)
    }

    pub(super) fn ensure_can_remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::RemoveReaction)
    }

    pub(super) fn ensure_can_pin_message(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::PinMessage)
    }

    pub(super) fn ensure_can_vote_poll(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::VotePoll)
    }

    pub(super) fn ensure_can_manage_thread(
        &self,
        thread_id: Id<ChannelMarker>,
        action: DiscordAction,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state
            .channel(thread_id)
            .ok_or_else(|| action_blocked(action, ActionBlockReason::ChannelDataUnavailable))?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread management requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, action)
    }

    pub(super) fn ensure_can_change_thread_membership(
        &self,
        thread_id: Id<ChannelMarker>,
        joining: bool,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(thread_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify thread membership permissions".to_owned())
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread membership can only change for a thread".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::ChangeThreadMembership)?;
        if channel.thread_archived().unwrap_or(false) {
            return Err(AppError::DiscordRequest(
                "thread membership cannot change while the thread is archived".to_owned(),
            ));
        }
        if joining {
            ensure_permission(
                &state,
                channel,
                DiscordAction::ChangeThreadMembership,
                DiscordPermission::ViewChannel,
            )?;
        }
        Ok(())
    }

    pub(super) fn ensure_can_reopen_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(thread_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify thread reopen permissions".to_owned())
        })?;
        if !channel.is_thread() {
            return Err(AppError::DiscordRequest(
                "thread reopen requires a thread channel".to_owned(),
            ));
        }
        ensure_channel_action_policy(&state, channel, DiscordAction::ReopenThread)
    }

    pub(super) fn ensure_can_read_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<()> {
        self.ensure_channel_action(channel_id, DiscordAction::ReadMessageHistory)
    }

    pub(super) fn ensure_can_edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify permission to edit this message".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::EditMessage)?;
        let message = state
            .messages_for_channel(channel_id)
            .into_iter()
            .find(|message| message.id == message_id)
            .ok_or_else(|| {
                AppError::DiscordRequest(format!(
                    "message {} was not found in channel {}",
                    message_id.get(),
                    channel_id.get()
                ))
            })?;
        if Some(message.author_id) == state.current_user_id() {
            Ok(())
        } else {
            Err(AppError::DiscordRequest(
                "only the message author can edit this message".to_owned(),
            ))
        }
    }

    pub(super) fn ensure_can_delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify permission to delete this message".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::DeleteMessage)?;
        let message = state
            .messages_for_channel(channel_id)
            .into_iter()
            .find(|message| message.id == message_id)
            .ok_or_else(|| {
                AppError::DiscordRequest(format!(
                    "message {} was not found in channel {}",
                    message_id.get(),
                    channel_id.get()
                ))
            })?;
        if Some(message.author_id) == state.current_user_id() {
            return Ok(());
        }
        ensure_permission(
            &state,
            channel,
            DiscordAction::DeleteMessage,
            DiscordPermission::ManageMessages,
        )
    }

    pub(super) fn ensure_can_add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state.channel(channel_id).ok_or_else(|| {
            AppError::DiscordRequest("cannot verify reaction channel permissions".to_owned())
        })?;
        ensure_channel_action_policy(&state, channel, DiscordAction::AddReaction)?;
        ensure_permission(
            &state,
            channel,
            DiscordAction::AddReaction,
            DiscordPermission::ReadMessageHistory,
        )?;
        if channel.is_thread() && channel.thread_archived().unwrap_or(false) {
            return Err(AppError::DiscordRequest(
                "cannot add reactions while the thread is archived".to_owned(),
            ));
        }
        let reaction_exists = state
            .messages_for_channel(channel_id)
            .into_iter()
            .find(|message| message.id == message_id)
            .is_some_and(|message| {
                message
                    .reactions
                    .iter()
                    .any(|reaction| reaction.emoji == *emoji)
            });
        if !reaction_exists {
            ensure_permission(
                &state,
                channel,
                DiscordAction::AddReaction,
                DiscordPermission::AddReactions,
            )?;
        }
        if state.reaction_emoji_requires_external_permission(channel, emoji) {
            ensure_permission(
                &state,
                channel,
                DiscordAction::AddReaction,
                DiscordPermission::UseExternalEmojis,
            )?;
        }
        Ok(())
    }

    fn ensure_channel_action(
        &self,
        channel_id: Id<ChannelMarker>,
        action: DiscordAction,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let channel = state
            .channel(channel_id)
            .ok_or_else(|| action_blocked(action, ActionBlockReason::ChannelDataUnavailable))?;
        ensure_channel_action_policy(&state, channel, action)
    }
}

fn changed_moderated_thread_tags(
    state: &crate::discord::state::DiscordState,
    thread: &crate::discord::ChannelState,
    applied_tags: &[Id<ForumTagMarker>],
) -> bool {
    let Some(parent) = thread
        .parent_id
        .and_then(|parent_id| state.channel(parent_id))
    else {
        return false;
    };
    parent.available_tags.iter().any(|tag| {
        tag.moderated && (thread.applied_tags.contains(&tag.id) != applied_tags.contains(&tag.id))
    })
}

pub(super) fn ensure_channel_action_policy(
    state: &crate::discord::state::DiscordState,
    channel: &crate::discord::ChannelState,
    action: DiscordAction,
) -> Result<()> {
    match state.channel_action_decision(channel, action) {
        ActionDecision::Allowed => Ok(()),
        ActionDecision::Blocked(reason) => Err(action_blocked(action, reason)),
    }
}

pub(super) fn ensure_permission(
    state: &crate::discord::state::DiscordState,
    channel: &crate::discord::ChannelState,
    action: DiscordAction,
    permission: DiscordPermission,
) -> Result<()> {
    let reason = match state.channel_permission_decision(channel, permission) {
        PermissionDecision::Allowed => return Ok(()),
        PermissionDecision::Denied(permission) => ActionBlockReason::PermissionDenied(permission),
        PermissionDecision::Unavailable(gap) => ActionBlockReason::PermissionDataUnavailable(gap),
    };
    Err(action_blocked(action, reason))
}

fn action_blocked(action: DiscordAction, reason: ActionBlockReason) -> AppError {
    AppError::DiscordActionBlocked { action, reason }
}
