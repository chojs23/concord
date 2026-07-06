use std::time::Instant;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::discord::{AppEvent, VoiceConnectionStatus, VoiceScope};
use crate::logging;

use super::{ActiveGuildScope, DashboardState, VoiceConnectionUiState};

struct EventViewportContext {
    was_at_latest: bool,
    was_following_cursor: bool,
    user_just_sent: bool,
    active_new_message: Option<(Id<ChannelMarker>, Id<MessageMarker>)>,
    selected_message_id: Option<Id<MessageMarker>>,
    scroll_message_id: Option<Id<MessageMarker>>,
    channel_cursor_id: Option<Id<ChannelMarker>>,
}

impl EventViewportContext {
    fn capture(state: &DashboardState, event: &AppEvent) -> Self {
        // Two layered behaviours run on every event:
        //
        // * Auto-scroll: when the user is already viewing the latest message
        //   (the bottom of the last message is visible in the viewport, even
        //   if the cursor is parked on an older one), keep the viewport
        //   tracking the latest after the event applies. The cursor is
        //   preserved by message id.
        // * Auto-follow: a superset of auto-scroll that also moves the
        //   cursor to the new latest message. Triggers only when the user
        //   was already following the latest message. Self-sent messages no
        //   longer force-follow. If the user is reading older history,
        //   sending a message keeps the viewport parked.
        //
        // Both modes share `message_auto_follow`. It means the next render
        // should align the viewport to the bottom. Auto-follow also jumps
        // the cursor.
        let was_auto_follow = state.messages.message_auto_follow;
        let was_at_latest = was_auto_follow || state.is_viewport_at_latest_message();
        let was_cursor_on_last = state.cursor_on_last_message();
        let was_following_cursor = was_at_latest && was_cursor_on_last;
        let preserve_selection = !was_following_cursor;
        let preserve_scroll = !(was_at_latest || was_following_cursor);

        Self {
            was_at_latest,
            was_following_cursor,
            user_just_sent: state.event_is_self_message_in_active_channel(event),
            active_new_message: state.active_channel_message_create(event),
            selected_message_id: preserve_selection
                .then(|| {
                    state
                        .messages()
                        .get(state.selected_message())
                        .map(|message| message.id)
                })
                .flatten(),
            scroll_message_id: preserve_scroll
                .then(|| {
                    state
                        .messages()
                        .get(state.messages.message_scroll)
                        .map(|message| message.id)
                })
                .flatten(),
            channel_cursor_id: state.selected_channel_cursor_id(),
        }
    }

    fn repair_after_event(self, state: &mut DashboardState, event: &AppEvent) {
        state.clamp_active_selection();
        state.restore_channel_cursor(self.channel_cursor_id);
        state.clamp_selection_indices();
        state.clear_missing_new_messages_marker();

        let in_message_view = state.message_pane_supports_auto_follow();
        let should_follow = self.was_following_cursor && in_message_view;
        let should_scroll = should_follow || (self.was_at_latest && in_message_view);
        if should_follow {
            state.follow_latest_message();
        } else {
            state.restore_message_position(self.selected_message_id, self.scroll_message_id);
        }

        if should_scroll {
            // Keep the bottom-align intent across to the next render so
            // `clamp_message_viewport_for_image_previews` snaps to the new
            // last message even when only the viewport (not the cursor)
            // moves.
            state.messages.message_auto_follow = true;
            state.clear_new_messages_marker();
            if let Some((channel_id, _)) = self.active_new_message {
                if self.user_just_sent {
                    state.messages.unread_divider_last_acked_id = None;
                    state.messages.pending_unread_anchor_scroll = false;
                } else {
                    state.schedule_channel_ack(channel_id);
                }
            }
        } else if in_message_view
            && !self.was_at_latest
            && !self.user_just_sent
            && state.messages.new_messages_marker_message_id.is_none()
        {
            state.messages.new_messages_marker_message_id =
                self.active_new_message.map(|(_, message_id)| message_id);
        }

        if let AppEvent::MessageHistoryAroundLoaded {
            channel_id,
            message_id,
            ..
        } = event
        {
            state.select_loaded_referenced_message(*channel_id, *message_id);
        }

        state.clamp_list_viewports();
        state.clamp_message_viewport();
        if !should_scroll {
            state.refresh_message_auto_follow();
        }
    }
}

impl DashboardState {
    pub(super) fn push_event_inner(&mut self, event: AppEvent, apply_discord: bool) {
        let mut viewport = EventViewportContext::capture(self, &event);

        self.apply_event_ui_effects(&event, &mut viewport.channel_cursor_id);
        if apply_discord {
            self.apply_event_to_discord_cache(&event);
        }
        self.refresh_event_derived_ui(&event);
        viewport.repair_after_event(self, &event);
    }

    fn apply_event_ui_effects(
        &mut self,
        event: &AppEvent,
        channel_cursor_id: &mut Option<Id<ChannelMarker>>,
    ) {
        match event {
            AppEvent::Ready { user, user_id } => {
                self.discord.current_user = Some(user.clone());
                self.discord.current_user_id = *user_id;
                self.runtime.gateway_error = None;
            }
            AppEvent::GatewayError { message } => {
                logging::error("tui", message);
                self.runtime.gateway_error = Some(message.clone());
                self.show_error_toast(message, Instant::now());
            }
            AppEvent::CaptchaRequired { action } => {
                self.show_captcha_toast(
                    format!(
                        "Discord needs a CAPTCHA to {action}. Finish it in the official Discord app, then try again."
                    ),
                    Instant::now(),
                );
            }
            AppEvent::MediaPlaybackWindowReady { request_id, .. } => {
                self.clear_media_playback_preparing(*request_id);
            }
            AppEvent::CurrentUserCapabilities { premium_tier } => {
                self.discord.current_user_premium_tier = Some(*premium_tier);
            }
            AppEvent::DmEstablished { channel_id } => {
                self.record_dm_established(*channel_id);
            }
            AppEvent::ApplicationCommandsLoaded { guild_id, commands } => {
                self.discord
                    .application_commands
                    .insert(*guild_id, commands.clone());
                self.refresh_active_mention_query();
            }
            AppEvent::AttachmentDownloadStarted {
                id,
                filename,
                total_bytes,
                source,
            } => {
                self.record_attachment_download_started(
                    *id,
                    filename.clone(),
                    *total_bytes,
                    *source,
                );
            }
            AppEvent::AttachmentDownloadProgress {
                id,
                downloaded_bytes,
                total_bytes,
            } => {
                self.record_attachment_download_progress(*id, *downloaded_bytes, *total_bytes);
            }
            AppEvent::AttachmentDownloadCompleted { id, path, .. } => {
                self.remove_attachment_download(*id);
                self.show_success_toast(format!("Downloaded to {path}"), Instant::now());
            }
            AppEvent::AttachmentDownloadFailed {
                id,
                filename,
                message,
                ..
            } => {
                let filename = self
                    .remove_attachment_download(*id)
                    .unwrap_or_else(|| filename.clone());
                self.show_error_toast(
                    format!("Download {filename} failed: {message}"),
                    Instant::now(),
                );
            }
            AppEvent::UpdateAvailable { latest_version } => {
                self.discord.update_available_version = Some(latest_version.clone());
            }
            AppEvent::ReactionUsersLoaded {
                channel_id,
                message_id,
                emoji,
                users,
                next_after,
                after,
            } => {
                if let Some(popup) = self.popups.reaction_users_popup_mut() {
                    popup.apply_loaded(
                        *channel_id,
                        *message_id,
                        emoji,
                        users.clone(),
                        *next_after,
                        *after,
                    );
                }
            }
            AppEvent::ReactionUsersLoadFailed {
                channel_id,
                message_id,
                emoji,
            } => {
                if let Some(popup) = self.popups.reaction_users_popup_mut() {
                    popup.apply_load_failed(*channel_id, *message_id, emoji);
                }
            }
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset,
                next_offset: _,
                threads,
                has_more,
                ..
            } => {
                self.record_forum_posts_loaded(
                    *channel_id,
                    *archive_state,
                    *offset,
                    threads,
                    *has_more,
                );
            }
            AppEvent::MessageSearchLoaded { .. } | AppEvent::MessageSearchLoadFailed { .. } => {
                self.record_search_event(event);
            }
            AppEvent::MessageHistoryLoaded { .. } | AppEvent::MessageHistoryAfterLoaded { .. } => {}
            AppEvent::InboxMentionsLoaded {
                request_id,
                messages,
            } => {
                self.apply_inbox_mentions_loaded(*request_id, messages);
            }
            AppEvent::InboxMentionsLoadFailed { request_id } => {
                self.apply_inbox_mentions_load_failed(*request_id);
            }
            AppEvent::InboxChannelMessagesLoaded {
                request_id,
                channel_id,
                messages,
            } => {
                self.apply_inbox_channel_messages_loaded(*request_id, *channel_id, messages);
            }
            AppEvent::InboxChannelMessagesLoadFailed {
                request_id,
                channel_id,
            } => {
                self.apply_inbox_channel_messages_load_failed(*request_id, *channel_id);
            }
            AppEvent::MessageHistoryRefreshed { channel_id, .. } => {
                self.record_message_history_refreshed(*channel_id);
            }
            AppEvent::UserProfileLoaded { guild_id, profile } => {
                self.record_user_profile_update_succeeded(profile.user_id, *guild_id);
            }
            AppEvent::UserProfileLoadFailed {
                user_id,
                guild_id,
                message,
            } => {
                if let Some(popup) = self.popups.user_profile_popup_mut()
                    && popup.user_id == *user_id
                    && popup.guild_id == *guild_id
                {
                    popup.load_error = Some(message.clone());
                    if popup.settings.saving {
                        popup.settings.saving = false;
                        popup.settings.status = Some(format!(
                            "Save succeeded, but profile reload failed: {message}"
                        ));
                    }
                }
            }
            AppEvent::UserProfileUpdateFailed {
                user_id,
                guild_id,
                message,
            } => {
                self.record_user_profile_update_failed(*user_id, *guild_id, message);
            }
            AppEvent::ActivateChannel { channel_id } => {
                self.activate_event_channel(*channel_id, channel_cursor_id);
            }
            AppEvent::VoiceConnectionStatusChanged {
                scope,
                channel_id,
                status,
                message,
            } => {
                self.record_voice_connection_status(*scope, *channel_id, *status, message);
            }
            AppEvent::ChannelUpsert(channel) => {
                self.record_thread_channel_upserted(channel);
            }
            _ => {}
        }
    }

    fn activate_event_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
        channel_cursor_id: &mut Option<Id<ChannelMarker>>,
    ) {
        let scope = self
            .discord
            .cache
            .channel(channel_id)
            .map(|channel| match channel.guild_id {
                Some(guild_id) => ActiveGuildScope::Guild(guild_id),
                None => ActiveGuildScope::DirectMessages,
            });
        if let Some(scope) = scope {
            self.activate_guild(scope);
            self.activate_channel(channel_id);
            self.navigation.channels.list.keep_selection_visible();
            *channel_cursor_id = Some(channel_id);
        }
    }

    fn record_voice_connection_status(
        &mut self,
        scope: VoiceScope,
        channel_id: Option<Id<ChannelMarker>>,
        status: VoiceConnectionStatus,
        message: &Option<String>,
    ) {
        match status {
            VoiceConnectionStatus::Connecting => {
                self.runtime.voice_connection = Some(VoiceConnectionUiState { scope, channel_id });
                self.show_success_toast(
                    message.as_deref().unwrap_or("Voice join requested"),
                    Instant::now(),
                );
            }
            VoiceConnectionStatus::Connected => {
                self.runtime.voice_connection = Some(VoiceConnectionUiState { scope, channel_id });
                self.show_success_toast(
                    message.as_deref().unwrap_or("Voice connected"),
                    Instant::now(),
                );
            }
            VoiceConnectionStatus::Disconnected => {
                if self
                    .runtime
                    .voice_connection
                    .is_some_and(|voice| voice.scope == scope)
                {
                    self.runtime.voice_connection = None;
                }
                self.show_success_toast(
                    message.as_deref().unwrap_or("Voice leave requested"),
                    Instant::now(),
                );
            }
            VoiceConnectionStatus::Failed => {
                if self
                    .runtime
                    .voice_connection
                    .is_some_and(|voice| voice.scope == scope)
                {
                    self.runtime.voice_connection = None;
                }
                self.show_error_toast(
                    message.as_deref().unwrap_or("Voice request failed"),
                    Instant::now(),
                );
            }
        }
    }

    fn apply_event_to_discord_cache(&mut self, event: &AppEvent) {
        let discord_event = self.discord_event_for_apply(event);
        self.discord.cache.apply_event(&discord_event);
        if Self::event_affects_message_row_content_metrics(&discord_event) {
            self.clear_message_row_content_metrics_cache();
        }
    }

    fn refresh_event_derived_ui(&mut self, event: &AppEvent) {
        if matches!(
            event,
            AppEvent::CurrentUserCapabilities { .. } | AppEvent::GuildEmojisUpdate { .. }
        ) {
            self.refresh_composer_emoji_candidates_for_current_query();
        }
    }
}
