use chrono::{DateTime, Utc};

use super::DiscordClient;
use crate::discord::{
    ForumPostCreate, GuildFolder, MESSAGE_FLAG_SUPPRESS_EMBEDS, MessageAttachmentUpload,
    MessageInfo, ReactionEmoji, ReactionUserInfo, ReplyReference, UserProfileInfo,
    UserProfileUpdate,
    commands::ForumPostArchiveState,
    ids::{
        Id,
        marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
    },
    rest::{CreatedForumPost, ForumPostPage, MessageEditRequest},
};
use crate::{AppError, Result};

impl DiscordClient {
    pub async fn prime_rest_pool(&self) -> Result<()> {
        self.rest.prime_connection_pool().await
    }

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
        self.ensure_can_send_message(channel_id, attachments)?;
        let upload_limit = self.attachment_size_limit(channel_id);
        self.rest
            .send_message(channel_id, content, reply_to, attachments, upload_limit)
            .await
    }

    pub async fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        self.ensure_can_send_tts_message(channel_id)?;
        self.rest.send_tts_message(channel_id, content).await
    }

    pub async fn create_forum_post(&self, post: &ForumPostCreate) -> Result<CreatedForumPost> {
        self.ensure_can_create_forum_post(post)?;
        let upload_limit = self.attachment_size_limit(post.channel_id);
        self.rest
            .create_forum_post(
                post.channel_id,
                &post.title,
                &post.content,
                &post.applied_tags,
                &post.attachments,
                upload_limit,
            )
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

    pub(super) fn ensure_can_send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        attachments: &[MessageAttachmentUpload],
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let Some(channel) = state.channel(channel_id) else {
            return Ok(());
        };
        if !state.can_send_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot send message in channel".to_owned(),
            ));
        }
        if !attachments.is_empty() && !state.can_attach_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot attach files in channel".to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn ensure_can_create_forum_post(&self, post: &ForumPostCreate) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let Some(channel) = state.channel(post.channel_id) else {
            return Ok(());
        };
        if !channel.is_forum() {
            return Err(AppError::DiscordRequest(
                "cannot create forum post outside a forum channel".to_owned(),
            ));
        }
        if !state.can_send_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot create forum post in channel".to_owned(),
            ));
        }
        if !post.attachments.is_empty() && !state.can_attach_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot attach files in channel".to_owned(),
            ));
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
        Ok(())
    }

    pub(super) fn ensure_can_send_tts_message(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let state = self
            .state
            .read()
            .expect("discord state lock is not poisoned");
        let Some(channel) = state.channel(channel_id) else {
            return Ok(());
        };
        if !state.can_send_tts_in_channel(channel) {
            return Err(AppError::DiscordRequest(
                "cannot send text-to-speech messages in channel".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
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
            state
                .messages_for_channel(channel_id)
                .into_iter()
                .find(|message| message.id == message_id)
                .map(|message| message.flags)
                .ok_or_else(|| {
                    AppError::DiscordRequest(format!(
                        "message {} was not found in channel {}",
                        message_id.get(),
                        channel_id.get()
                    ))
                })?
                | MESSAGE_FLAG_SUPPRESS_EMBEDS
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
        self.rest.follow_thread(thread_id).await
    }

    pub async fn unfollow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.rest.unfollow_thread(thread_id).await
    }

    pub async fn set_thread_archived(
        &self,
        thread_id: Id<ChannelMarker>,
        archived: bool,
    ) -> Result<()> {
        self.rest.set_thread_archived(thread_id, archived).await
    }

    pub async fn set_thread_locked(
        &self,
        thread_id: Id<ChannelMarker>,
        locked: bool,
    ) -> Result<()> {
        self.rest.set_thread_locked(thread_id, locked).await
    }

    pub async fn set_thread_pinned(
        &self,
        thread_id: Id<ChannelMarker>,
        pinned: bool,
        current_flags: u64,
    ) -> Result<()> {
        self.rest
            .set_thread_pinned(thread_id, pinned, current_flags)
            .await
    }

    pub async fn delete_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
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
        self.rest
            .edit_thread_settings(
                thread_id,
                name,
                applied_tags,
                rate_limit_per_user,
                auto_archive_duration,
            )
            .await
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
        self.rest
            .load_message_history_around(channel_id, message_id, limit)
            .await
    }

    pub async fn load_recent_mentions(&self, limit: u16) -> Result<Vec<MessageInfo>> {
        self.rest.load_recent_mentions(limit).await
    }

    pub async fn load_message_history_after(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.rest
            .load_message_history_after(channel_id, message_id, limit)
            .await
    }

    pub async fn search_messages(
        &self,
        query: crate::discord::MessageSearchQuery,
    ) -> Result<crate::discord::MessageSearchPage> {
        self.rest.search_messages(query).await
    }

    pub async fn load_forum_posts(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
    ) -> Result<ForumPostPage> {
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
        self.rest.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.rest
            .remove_current_user_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_reaction_users(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<Vec<ReactionUserInfo>> {
        self.rest
            .load_reaction_users(channel_id, message_id, emoji)
            .await
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        self.rest.load_pinned_messages(channel_id).await
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
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
}
