use std::{collections::HashSet, time::Instant};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::discord::{
    AppEvent, MessageHistoryLoadTarget, MessageInfo, VoiceAudioSourceOptions,
    VoiceConnectionStatus, VoiceScope,
};
use crate::logging;

use super::popups::{ChannelActionMenuState, ModalPopup};
use super::{
    ChannelPaneCursor, DashboardState, MINIMUM_ESTABLISHED_DM_MESSAGES, VoiceConnectionUiState,
};

struct EventViewportContext {
    was_at_latest: bool,
    was_following_cursor: bool,
    user_just_sent: bool,
    active_new_message: Option<(Id<ChannelMarker>, Id<MessageMarker>)>,
    selected_message_id: Option<Id<MessageMarker>>,
    scroll_message_id: Option<Id<MessageMarker>>,
    channel_cursor: Option<ChannelPaneCursor>,
}

fn interaction_failure_reason(reason_code: u64) -> &'static str {
    match reason_code {
        1 => "Discord returned an unknown interaction error",
        2 => "the command timed out",
        3 => "the application is unknown",
        16 => "the activity is not released on this platform",
        17 => "the activity failed to launch",
        18 => "the user does not have access to the activity",
        19 => "the activity cannot launch from this location",
        20 => "the activity is not supported in this region",
        _ => "Discord returned an interaction error",
    }
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
            channel_cursor: state.selected_channel_cursor(),
        }
    }

    fn repair_after_event(self, state: &mut DashboardState, event: &AppEvent) {
        state.clamp_active_selection();
        state.restore_channel_pane_cursor(self.channel_cursor);
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
    pub(super) fn push_event_inner(&mut self, event: AppEvent) {
        let viewport = EventViewportContext::capture(self, &event);

        self.apply_event_ui_effects(&event);
        self.close_composer_for_safety_lock();
        self.refresh_event_derived_ui(&event);
        viewport.repair_after_event(self, &event);
    }

    fn apply_event_ui_effects(&mut self, event: &AppEvent) {
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
            AppEvent::MessageSendRateLimited {
                channel_id,
                retry_after_millis,
            } => {
                self.record_slow_mode_deadline(
                    *channel_id,
                    std::time::Duration::from_millis(*retry_after_millis),
                );
                self.show_error_toast(
                    "Discord rate limited this channel. Wait for the composer timer.",
                    Instant::now(),
                );
            }
            AppEvent::MessageSendCooldownStarted {
                channel_id,
                duration_millis,
            } => {
                self.record_slow_mode_deadline(
                    *channel_id,
                    std::time::Duration::from_millis(*duration_millis),
                );
            }
            AppEvent::MessageSendFailed { channel_id, nonce } => {
                self.remove_pending_message(*channel_id, *nonce);
            }
            AppEvent::MediaPlaybackWindowReady { request_id, .. } => {
                self.clear_media_playback_preparing(*request_id);
            }
            AppEvent::StreamPlaybackWindowReady {
                scope,
                channel_id,
                user_id,
            } => {
                self.clear_stream_playback_preparing(*scope, *channel_id, *user_id);
            }
            AppEvent::StreamPlaybackEnded {
                scope,
                channel_id,
                user_id,
                reconnecting,
            } => {
                self.record_stream_playback_ended(*scope, *channel_id, *user_id, *reconnecting);
            }
            AppEvent::StreamCaptureTargetsLoaded {
                request_id,
                scope,
                channel_id,
                targets,
                error,
            } => {
                let matches_request = self
                    .runtime
                    .stream_capture_targets_request
                    .as_ref()
                    .is_some_and(|request| {
                        request.request_id == *request_id
                            && request.target.matches(*scope, *channel_id)
                    });
                if !matches_request {
                    return;
                }
                self.runtime.stream_capture_targets_request = None;
                self.clear_stream_capture_targets_loading_toast();
                if let Some(error) = error {
                    self.show_error_toast(error, Instant::now());
                } else if targets.is_empty() {
                    self.show_error_toast(
                        "No displays or shareable windows were found.",
                        Instant::now(),
                    );
                } else if self.popups.modal.is_none()
                    && self.runtime.voice_connection.is_some_and(|voice| {
                        voice.scope == *scope && voice.channel_id == Some(*channel_id)
                    })
                {
                    self.popups.set_modal(ModalPopup::ChannelActionMenu(
                        ChannelActionMenuState::StreamTargets {
                            scope: *scope,
                            channel_id: *channel_id,
                            targets: targets.clone(),
                            selection: Default::default(),
                        },
                    ));
                }
            }
            AppEvent::VoiceAudioSourcesLoaded {
                request_id,
                inputs,
                outputs,
                error,
            } => {
                if self.options.voice_audio_sources_request_id != Some(*request_id) {
                    return;
                }
                self.options.voice_audio_sources_request_id = None;
                if let Some(error) = error {
                    // Keep no list rather than the previous one, so rows cannot
                    // show device names from an enumeration that just failed.
                    self.options.voice_audio_source_options = VoiceAudioSourceOptions::default();
                    self.show_error_toast(error, Instant::now());
                } else {
                    self.options.voice_audio_source_options =
                        VoiceAudioSourceOptions::from_parts(inputs.clone(), outputs.clone());
                }
            }
            AppEvent::VoiceAudioSourcesApplyFailed {
                requested_input_source,
                requested_output_source,
                active_input_source,
                active_output_source,
                message,
            } => {
                let failed_request_is_current = self.options.voice_options.input_source
                    == *requested_input_source
                    && self.options.voice_options.output_source == *requested_output_source;
                if failed_request_is_current {
                    self.options.voice_options.input_source = active_input_source.clone();
                    self.options.voice_options.output_source = active_output_source.clone();
                    self.options.config_save_pending = true;
                }
                self.show_error_toast(message, Instant::now());
            }
            AppEvent::StreamBroadcastStarted { scope, channel_id } => {
                self.clear_stream_broadcast_preparing(*scope, *channel_id);
            }
            AppEvent::StreamBroadcastAudioUnavailable { message } => {
                self.show_error_toast(message, Instant::now());
            }
            AppEvent::StreamBroadcastStartFailed { scope, channel_id } => {
                self.cancel_stream_broadcast_preparing(*scope, *channel_id);
            }
            AppEvent::StreamBroadcastEnded { scope, channel_id } => {
                self.record_stream_broadcast_ended(*scope, *channel_id);
            }
            AppEvent::CurrentUserCapabilities { premium_tier } => {
                self.discord.current_user_premium_tier = Some(*premium_tier);
            }
            AppEvent::CurrentUserVerification { .. } => {}
            AppEvent::ApplicationCommandsLoaded { guild_id, commands } => {
                self.discord
                    .application_commands
                    .insert(*guild_id, commands.clone());
                self.refresh_active_mention_query();
            }
            AppEvent::ApplicationCommandIndexUpdated { guild_id } => {
                self.discord.application_commands.remove(&Some(*guild_id));
                self.invalidate_application_command_autocomplete();
                self.queue_application_command_load(Some(*guild_id));
            }
            AppEvent::InteractionFailed {
                reason_code,
                correlated: true,
                ..
            } => {
                self.show_error_toast(
                    format!(
                        "Discord could not complete the application command: {}.",
                        interaction_failure_reason(*reason_code)
                    ),
                    Instant::now(),
                );
            }
            AppEvent::InteractionFailed { .. } => {}
            AppEvent::InteractionSucceeded { .. } => {}
            AppEvent::ApplicationCommandAutocompleteResponse { nonce, choices } => {
                self.apply_application_command_autocomplete_response(nonce.as_deref(), choices);
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
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                target: MessageHistoryLoadTarget::Latest,
                ..
            } => {
                self.record_latest_message_history_failed(*channel_id);
            }
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset,
                next_offset: _,
                threads,
                first_messages,
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
                if *archive_state == crate::discord::ForumPostArchiveState::Active && *offset == 0 {
                    self.apply_inbox_forum_posts_loaded(*channel_id, threads, first_messages);
                }
            }
            AppEvent::ForumPostsLoadFailed {
                channel_id,
                archive_state,
                offset,
                ..
            } => {
                if *archive_state == crate::discord::ForumPostArchiveState::Active && *offset == 0 {
                    self.apply_inbox_forum_posts_load_failed(*channel_id);
                }
            }
            AppEvent::MessageSearchLoaded { .. } | AppEvent::MessageSearchLoadFailed { .. } => {
                self.record_search_event(event);
            }
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before: None,
                messages,
            } => {
                self.record_latest_message_history_loaded(*channel_id);
                self.record_dm_established_from_messages(*channel_id, messages);
            }
            AppEvent::MessageHistoryLoaded { .. } | AppEvent::MessageHistoryAfterLoaded { .. } => {}
            AppEvent::InboxMentionsLoaded {
                request_id,
                before,
                messages,
                has_more,
            } => {
                self.apply_inbox_mentions_loaded(*request_id, *before, messages, *has_more);
            }
            AppEvent::InboxMentionsLoadFailed { request_id, before } => {
                self.apply_inbox_mentions_load_failed(*request_id, *before);
            }
            AppEvent::InboxRecentMentionDeleted { message_id } => {
                self.apply_inbox_recent_mention_deleted(*message_id);
            }
            AppEvent::InboxRecentMentionDeleteFailed { message, .. } => {
                self.show_error_toast(message, Instant::now());
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
            AppEvent::MessageHistoryRefreshed {
                channel_id,
                messages,
            } => {
                self.record_latest_message_history_loaded(*channel_id);
                self.record_dm_established_from_messages(*channel_id, messages);
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
            AppEvent::MessageCreate { message } => {
                if let Some(nonce) = message.nonce {
                    self.remove_pending_message(message.channel_id, nonce);
                }
                self.record_dm_established_from_messages(
                    message.channel_id,
                    std::slice::from_ref(message),
                );
                self.record_slow_mode_after_self_message(message);
            }
            _ => {}
        }
    }

    fn record_slow_mode_after_self_message(&mut self, message: &MessageInfo) {
        if self.current_user_id() != Some(message.author_id) {
            return;
        }
        let slow_mode = self
            .discord
            .cache
            .channel(message.channel_id)
            .filter(|channel| !self.discord.cache.bypasses_slow_mode(channel))
            .and_then(|channel| channel.rate_limit_per_user)
            .filter(|seconds| *seconds > 0)
            .map(std::time::Duration::from_secs);
        if let Some(slow_mode) = slow_mode {
            self.record_slow_mode_deadline(message.channel_id, slow_mode);
        }
    }

    fn record_dm_established_from_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        messages: &[MessageInfo],
    ) {
        if self
            .navigation
            .channels
            .established_dms
            .contains(&channel_id)
            || !self
                .discord
                .cache
                .channel(channel_id)
                .is_some_and(|channel| channel.is_dm())
        {
            return;
        }
        let Some(current_user_id) = self.current_user_id() else {
            return;
        };
        let mut current_user_message_ids: HashSet<_> = self
            .discord
            .cache
            .messages_for_channel(channel_id)
            .into_iter()
            .filter(|message| message.author_id == current_user_id)
            .map(|message| message.id)
            .collect();
        current_user_message_ids.extend(
            messages
                .iter()
                .filter(|message| message.author_id == current_user_id)
                .map(|message| message.message_id),
        );
        if current_user_message_ids.len() >= MINIMUM_ESTABLISHED_DM_MESSAGES {
            self.record_dm_established(channel_id);
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
                self.runtime.stream_capture_targets_request = None;
                self.runtime.stream_playback_preparing = None;
                self.runtime.active_stream_playback = None;
                self.runtime.stream_broadcast_preparing = None;
                self.runtime.active_stream_broadcast = None;
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
                self.runtime.stream_capture_targets_request = None;
                self.runtime.stream_playback_preparing = None;
                self.runtime.active_stream_playback = None;
                self.runtime.stream_broadcast_preparing = None;
                self.runtime.active_stream_broadcast = None;
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

    fn refresh_event_derived_ui(&mut self, event: &AppEvent) {
        if matches!(
            event,
            AppEvent::CurrentUserCapabilities { .. } | AppEvent::GuildEmojisUpdate { .. }
        ) {
            self.refresh_composer_emoji_candidates_for_current_query();
        }
    }
}
