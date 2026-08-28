use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::Semaphore;
use tokio::task::AbortHandle;

use crate::{
    DiscordClient,
    discord::{AppCommand, VoiceAudioSettings},
};

use super::{
    gateway_commands, history_commands, inbox_commands, media_commands, message_commands,
    notification_commands, read_state_commands, session_commands, user_commands, voice_commands,
};

const MAX_CONCURRENT_ATTACHMENT_PREVIEWS: usize = 4;
const MAX_CONCURRENT_ATTACHMENT_DOWNLOADS: usize = 2;
const APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone, Default)]
struct AutocompleteRequestScheduler {
    pending: Arc<Mutex<Option<AbortHandle>>>,
}

impl AutocompleteRequestScheduler {
    fn replace(&self, request: impl Future<Output = ()> + Send + 'static) {
        let mut pending = self
            .pending
            .lock()
            .expect("autocomplete request lock is not poisoned");
        if let Some(previous) = pending.take() {
            previous.abort();
        }
        let task = tokio::spawn(async move {
            tokio::time::sleep(APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE).await;
            request.await;
        });
        *pending = Some(task.abort_handle());
    }
}

#[derive(Clone)]
pub(super) struct CommandDispatcher {
    client: DiscordClient,
    attachment_preview_permits: Arc<Semaphore>,
    attachment_download_permits: Arc<Semaphore>,
    autocomplete_requests: AutocompleteRequestScheduler,
}

impl CommandDispatcher {
    pub(super) fn new(client: DiscordClient) -> Self {
        Self {
            client,
            attachment_preview_permits: Arc::new(Semaphore::new(
                MAX_CONCURRENT_ATTACHMENT_PREVIEWS,
            )),
            attachment_download_permits: Arc::new(Semaphore::new(
                MAX_CONCURRENT_ATTACHMENT_DOWNLOADS,
            )),
            autocomplete_requests: AutocompleteRequestScheduler::default(),
        }
    }

    pub(super) async fn dispatch(&self, command: AppCommand) {
        if matches!(
            &command,
            AppCommand::RequestApplicationCommandAutocomplete { .. }
        ) {
            let dispatcher = self.clone();
            self.autocomplete_requests.replace(async move {
                dispatcher.handle(command).await;
            });
            return;
        }
        if runs_inline(&command) {
            self.handle(command).await;
        } else {
            let dispatcher = self.clone();
            tokio::spawn(async move {
                dispatcher.handle(command).await;
            });
        }
    }

    async fn handle(&self, command: AppCommand) {
        match command {
            AppCommand::LoadMessageHistory { channel_id, before } => {
                history_commands::load_history(self.client.clone(), channel_id, before).await;
            }
            AppCommand::RefreshMessageHistory { channel_id } => {
                history_commands::refresh_history(self.client.clone(), channel_id).await;
            }
            AppCommand::LoadMessageHistoryAfter {
                channel_id,
                after,
                mode,
            } => {
                history_commands::load_history_after(self.client.clone(), channel_id, after, mode)
                    .await;
            }
            AppCommand::LoadMessageHistoryAround {
                channel_id,
                message_id,
            } => {
                history_commands::load_history_around(self.client.clone(), channel_id, message_id)
                    .await;
            }
            AppCommand::LoadThreadPreview {
                channel_id,
                message_id,
            } => {
                history_commands::load_thread_preview(self.client.clone(), channel_id, message_id)
                    .await;
            }
            AppCommand::LoadForumPostData {
                guild_id,
                channel_id,
                thread_ids,
            } => {
                history_commands::load_forum_post_data(
                    self.client.clone(),
                    guild_id,
                    channel_id,
                    thread_ids,
                )
                .await;
            }
            AppCommand::LoadArchivedThreads {
                guild_id,
                channel_id,
                before,
            } => {
                history_commands::load_archived_threads(
                    self.client.clone(),
                    guild_id,
                    channel_id,
                    before,
                )
                .await;
            }
            AppCommand::SearchMessages { query } => {
                history_commands::search_messages(self.client.clone(), query).await;
            }
            AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id,
            } => {
                history_commands::load_inbox_channel_history(
                    self.client.clone(),
                    channel_id,
                    request_id,
                )
                .await;
            }
            AppCommand::LoadInboxMentions { request_id, before } => {
                inbox_commands::load_mentions(self.client.clone(), request_id, before).await;
            }
            AppCommand::DeleteInboxMention { message_id } => {
                inbox_commands::delete_mention(self.client.clone(), message_id).await;
            }
            AppCommand::LoadGuildMembersByIds { guild_id, user_ids } => {
                gateway_commands::load_members_by_ids(self.client.clone(), guild_id, user_ids)
                    .await;
            }
            AppCommand::SearchGuildMembers {
                guild_id,
                query,
                limit,
            } => {
                gateway_commands::search_members(self.client.clone(), guild_id, query, limit).await;
            }
            AppCommand::SetSelectedGuild { guild_id } => {
                gateway_commands::set_selected_guild(self.client.clone(), guild_id).await;
            }
            AppCommand::SetSelectedMessageChannel { channel_id } => {
                gateway_commands::set_selected_message_channel(self.client.clone(), channel_id)
                    .await;
            }
            AppCommand::SubscribeDirectMessage { channel_id } => {
                gateway_commands::subscribe_direct_message(self.client.clone(), channel_id).await;
            }
            AppCommand::SubscribeGuildChannel {
                guild_id,
                channel_id,
            } => {
                gateway_commands::subscribe_guild_channel(
                    self.client.clone(),
                    guild_id,
                    channel_id,
                )
                .await;
            }
            AppCommand::UpdateMemberListSubscription {
                guild_id,
                channel_id,
                thread_id,
                ranges,
            } => {
                gateway_commands::update_member_list_subscription(
                    self.client.clone(),
                    guild_id,
                    channel_id,
                    thread_id,
                    ranges,
                )
                .await;
            }
            AppCommand::JoinVoiceChannel {
                scope,
                channel_id,
                self_mute,
                self_deaf,
                input_source,
                output_source,
                allow_microphone_transmit,
                noise_suppression,
                microphone_sensitivity,
                microphone_volume,
                voice_output_volume,
                participant_playback_settings,
            } => {
                voice_commands::join_channel(
                    self.client.clone(),
                    voice_commands::JoinRequest {
                        scope,
                        channel_id,
                        self_mute,
                        self_deaf,
                        input_source,
                        output_source,
                        allow_microphone_transmit,
                        noise_suppression,
                        microphone_sensitivity,
                        microphone_volume,
                        voice_output_volume,
                        participant_playback_settings,
                    },
                )
                .await;
            }
            AppCommand::UpdateVoiceState {
                scope,
                channel_id,
                self_mute,
                self_deaf,
            } => {
                voice_commands::update_state(
                    self.client.clone(),
                    scope,
                    channel_id,
                    self_mute,
                    self_deaf,
                )
                .await;
            }
            AppCommand::UpdateVoiceCapturePermission {
                scope,
                channel_id,
                allow_microphone_transmit,
                noise_suppression,
                microphone_sensitivity,
                microphone_volume,
                voice_output_volume,
            } => {
                voice_commands::update_capture_permission(
                    self.client.clone(),
                    scope,
                    channel_id,
                    VoiceAudioSettings {
                        allow_microphone_transmit,
                        noise_suppression,
                        microphone_sensitivity,
                        microphone_volume,
                        voice_output_volume,
                    },
                )
                .await;
            }
            AppCommand::UpdateVoiceAudioSources {
                input_source,
                output_source,
            } => {
                voice_commands::update_audio_sources(
                    &self.client,
                    crate::discord::VoiceAudioSources {
                        input: input_source,
                        output: output_source,
                    },
                );
            }
            AppCommand::LoadVoiceAudioSources { request_id } => {
                voice_commands::load_voice_audio_sources(self.client.clone(), request_id).await;
            }
            AppCommand::UpdateVoiceParticipantPlayback { user_id, settings } => {
                voice_commands::update_participant_playback(&self.client, user_id, settings);
            }
            AppCommand::WatchVoiceStream {
                scope,
                channel_id,
                user_id,
                display_name,
            } => {
                voice_commands::watch_stream(
                    self.client.clone(),
                    scope,
                    channel_id,
                    user_id,
                    display_name,
                )
                .await;
            }
            AppCommand::LoadStreamCaptureTargets {
                request_id,
                scope,
                channel_id,
            } => {
                voice_commands::load_stream_capture_targets(
                    self.client.clone(),
                    request_id,
                    scope,
                    channel_id,
                )
                .await;
            }
            AppCommand::StartVoiceStream {
                scope,
                channel_id,
                target,
            } => {
                voice_commands::start_stream(self.client.clone(), scope, channel_id, target).await;
            }
            AppCommand::StopVoiceStream { scope, channel_id } => {
                voice_commands::stop_stream(self.client.clone(), scope, channel_id).await;
            }
            AppCommand::LeaveVoiceChannel {
                scope,
                self_mute,
                self_deaf,
            } => {
                voice_commands::leave_channel(self.client.clone(), scope, self_mute, self_deaf)
                    .await;
            }
            AppCommand::LoadAttachmentPreview { url } => {
                media_commands::load_attachment_preview(
                    self.client.clone(),
                    url,
                    self.attachment_preview_permits.clone(),
                )
                .await;
            }
            AppCommand::LoadProfileAvatarPreview { key, upload } => {
                media_commands::load_profile_avatar_preview(self.client.clone(), key, upload).await;
            }
            AppCommand::OpenUrl { url } => {
                media_commands::open_url(self.client.clone(), url).await;
            }
            AppCommand::PlayMedia { target, request_id } => {
                media_commands::play_media(self.client.clone(), target, request_id).await;
            }
            AppCommand::DownloadAttachment {
                id,
                url,
                filename,
                source,
            } => {
                media_commands::download_attachment(
                    self.client.clone(),
                    id,
                    url,
                    filename,
                    source,
                    self.attachment_download_permits.clone(),
                )
                .await;
            }
            AppCommand::SendMessage {
                channel_id,
                nonce,
                content,
                reply_to,
                attachments,
            } => {
                message_commands::send_message(
                    self.client.clone(),
                    channel_id,
                    nonce,
                    content,
                    reply_to,
                    attachments,
                )
                .await;
            }
            AppCommand::TriggerTyping { channel_id } => {
                message_commands::trigger_typing(self.client.clone(), channel_id).await;
            }
            AppCommand::SendTtsMessage {
                channel_id,
                nonce,
                content,
            } => {
                message_commands::send_tts_message(self.client.clone(), channel_id, nonce, content)
                    .await;
            }
            AppCommand::CreateForumPost { post } => {
                message_commands::create_forum_post(self.client.clone(), post).await;
            }
            AppCommand::SetThreadArchived {
                channel_id,
                archived,
                label,
            } => {
                message_commands::set_thread_archived(
                    self.client.clone(),
                    channel_id,
                    archived,
                    label,
                )
                .await;
            }
            AppCommand::SetThreadLocked {
                channel_id,
                locked,
                label,
            } => {
                message_commands::set_thread_locked(self.client.clone(), channel_id, locked, label)
                    .await;
            }
            AppCommand::SetThreadPinned {
                channel_id,
                pinned,
                current_flags,
                label,
            } => {
                message_commands::set_thread_pinned(
                    self.client.clone(),
                    channel_id,
                    pinned,
                    current_flags,
                    label,
                )
                .await;
            }
            AppCommand::DeleteThread { channel_id, label } => {
                message_commands::delete_thread(self.client.clone(), channel_id, label).await;
            }
            AppCommand::EditThread {
                channel_id,
                name,
                applied_tags,
                rate_limit_per_user,
                auto_archive_duration,
                label,
            } => {
                message_commands::edit_thread(
                    self.client.clone(),
                    channel_id,
                    name,
                    applied_tags,
                    rate_limit_per_user,
                    auto_archive_duration,
                    label,
                )
                .await;
            }
            AppCommand::LoadApplicationCommands { guild_id } => {
                message_commands::load_application_commands(self.client.clone(), guild_id).await;
            }
            AppCommand::RunApplicationCommand { invocation } => {
                message_commands::run_application_command(self.client.clone(), invocation).await;
            }
            AppCommand::RequestApplicationCommandAutocomplete { invocation } => {
                message_commands::request_application_command_autocomplete(
                    self.client.clone(),
                    invocation,
                )
                .await;
            }
            AppCommand::EditMessage {
                channel_id,
                message_id,
                content,
            } => {
                message_commands::edit_message(
                    self.client.clone(),
                    channel_id,
                    message_id,
                    content,
                )
                .await;
            }
            AppCommand::DeleteMessage {
                channel_id,
                message_id,
            } => {
                message_commands::delete_message(self.client.clone(), channel_id, message_id).await;
            }
            AppCommand::RemoveMessageEmbeds {
                channel_id,
                message_id,
            } => {
                message_commands::remove_message_embeds(
                    self.client.clone(),
                    channel_id,
                    message_id,
                )
                .await;
            }
            AppCommand::LeaveGuild { guild_id, label } => {
                message_commands::leave_guild(self.client.clone(), guild_id, label).await;
            }
            AppCommand::AddReaction {
                channel_id,
                message_id,
                emoji,
            } => {
                message_commands::add_reaction(self.client.clone(), channel_id, message_id, emoji)
                    .await;
            }
            AppCommand::RemoveReaction {
                channel_id,
                message_id,
                emoji,
            } => {
                message_commands::remove_reaction(
                    self.client.clone(),
                    channel_id,
                    message_id,
                    emoji,
                )
                .await;
            }
            AppCommand::LoadReactionUsers {
                channel_id,
                message_id,
                emoji,
                after,
            } => {
                message_commands::load_reaction_users(
                    self.client.clone(),
                    channel_id,
                    message_id,
                    emoji,
                    after,
                )
                .await;
            }
            AppCommand::LoadPinnedMessages { channel_id } => {
                message_commands::load_pinned_messages(self.client.clone(), channel_id).await;
            }
            AppCommand::SetMessagePinned {
                channel_id,
                message_id,
                pinned,
            } => {
                message_commands::set_message_pinned(
                    self.client.clone(),
                    channel_id,
                    message_id,
                    pinned,
                )
                .await;
            }
            AppCommand::VotePoll {
                channel_id,
                message_id,
                answer_ids,
            } => {
                message_commands::vote_poll(
                    self.client.clone(),
                    channel_id,
                    message_id,
                    answer_ids,
                )
                .await;
            }
            AppCommand::LoadUserProfile { user_id, guild_id } => {
                user_commands::load_profile(self.client.clone(), user_id, guild_id).await;
            }
            AppCommand::UpdateUserProfile { update } => {
                user_commands::update_profile(self.client.clone(), update).await;
            }
            AppCommand::UpdateCurrentUserStatus { status } => {
                user_commands::update_status(self.client.clone(), status).await;
            }
            AppCommand::UpdateGuildFolderSettings {
                folder_id,
                name,
                color,
            } => {
                user_commands::update_guild_folder_settings(
                    self.client.clone(),
                    folder_id,
                    name,
                    color,
                )
                .await;
            }
            AppCommand::UpdateCurrentUserActivity {
                status,
                activities,
                track_client_id,
            } => {
                user_commands::update_activity(
                    self.client.clone(),
                    status,
                    activities,
                    track_client_id,
                )
                .await;
            }
            AppCommand::AckChannel {
                channel_id,
                message_id,
            } => {
                read_state_commands::ack_channel(self.client.clone(), channel_id, message_id).await;
            }
            AppCommand::ScheduleAckChannel {
                channel_id,
                message_id,
            } => {
                read_state_commands::schedule_ack_channel(
                    self.client.clone(),
                    channel_id,
                    message_id,
                )
                .await;
            }
            AppCommand::AckChannels { targets } => {
                read_state_commands::ack_channels(self.client.clone(), targets).await;
            }
            AppCommand::SetGuildMuted {
                guild_id,
                muted,
                duration,
                label: _,
            } => {
                notification_commands::set_guild_muted(
                    self.client.clone(),
                    guild_id,
                    muted,
                    duration,
                )
                .await;
            }
            AppCommand::SetChannelMuted {
                guild_id,
                channel_id,
                muted,
                duration,
                label: _,
            } => {
                notification_commands::set_channel_muted(
                    self.client.clone(),
                    guild_id,
                    channel_id,
                    muted,
                    duration,
                )
                .await;
            }
            AppCommand::SetThreadMuted {
                channel_id,
                muted,
                duration,
                label: _,
            } => {
                notification_commands::set_thread_muted(
                    self.client.clone(),
                    channel_id,
                    muted,
                    duration,
                )
                .await;
            }
            AppCommand::SetThreadNotificationLevel {
                channel_id,
                flags,
                label: _,
            } => {
                notification_commands::set_thread_notification_level(
                    self.client.clone(),
                    channel_id,
                    flags,
                )
                .await;
            }
            AppCommand::SetThreadFollowed {
                channel_id,
                followed,
                label: _,
            } => {
                notification_commands::set_thread_followed(
                    self.client.clone(),
                    channel_id,
                    followed,
                )
                .await;
            }
            AppCommand::SignOut => session_commands::sign_out(self.client.clone()).await,
        }
    }
}

fn runs_inline(command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::SetSelectedGuild { .. }
            | AppCommand::SetSelectedMessageChannel { .. }
            | AppCommand::JoinVoiceChannel { .. }
            | AppCommand::UpdateVoiceState { .. }
            | AppCommand::UpdateVoiceCapturePermission { .. }
            | AppCommand::UpdateVoiceAudioSources { .. }
            | AppCommand::UpdateVoiceParticipantPlayback { .. }
            | AppCommand::WatchVoiceStream { .. }
            | AppCommand::StartVoiceStream { .. }
            | AppCommand::StopVoiceStream { .. }
            | AppCommand::LeaveVoiceChannel { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::discord::{
        MicrophoneSensitivityDb, VoiceParticipantPlaybackSettings, VoiceScope, VoiceVolumePercent,
        ids::Id,
    };

    use super::*;

    #[test]
    fn only_order_sensitive_control_commands_run_inline() {
        assert!(runs_inline(&AppCommand::SetSelectedGuild {
            guild_id: Some(Id::new(1)),
        }));
        assert!(runs_inline(&AppCommand::SetSelectedMessageChannel {
            channel_id: Some(Id::new(2)),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceCapturePermission {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceParticipantPlayback {
            user_id: Id::new(3),
            settings: VoiceParticipantPlaybackSettings::default(),
        }));
        assert!(runs_inline(&AppCommand::JoinVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            self_mute: false,
            self_deaf: false,
            input_source: None,
            output_source: None,
            allow_microphone_transmit: true,
            noise_suppression: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
            participant_playback_settings: Vec::new(),
        }));
        assert!(runs_inline(&AppCommand::UpdateVoiceState {
            scope: VoiceScope::Guild(Id::new(1)),
            channel_id: Id::new(2),
            self_mute: true,
            self_deaf: false,
        }));
        assert!(runs_inline(&AppCommand::LeaveVoiceChannel {
            scope: VoiceScope::Guild(Id::new(1)),
            self_mute: false,
            self_deaf: false,
        }));

        assert!(!runs_inline(&AppCommand::LoadMessageHistory {
            channel_id: Id::new(2),
            before: None,
        }));
        assert!(!runs_inline(&AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/avatar.png".to_owned(),
        }));
        assert!(!runs_inline(&AppCommand::LoadVoiceAudioSources {
            request_id: 1,
        }));
    }

    #[tokio::test]
    async fn autocomplete_scheduler_runs_only_the_latest_request() {
        let scheduler = AutocompleteRequestScheduler::default();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let first_tx = result_tx.clone();
        scheduler.replace(async move {
            first_tx.send(1).expect("result receiver stays open");
        });
        scheduler.replace(async move {
            result_tx.send(2).expect("result receiver stays open");
        });

        let result = tokio::time::timeout(
            APPLICATION_COMMAND_AUTOCOMPLETE_DEBOUNCE + Duration::from_millis(100),
            result_rx.recv(),
        )
        .await
        .expect("latest autocomplete request runs after the debounce");
        assert_eq!(result, Some(2));
        let superseded = tokio::time::timeout(Duration::from_millis(25), result_rx.recv()).await;
        assert!(
            !matches!(superseded, Ok(Some(_))),
            "superseded autocomplete request must not run"
        );
    }
}
