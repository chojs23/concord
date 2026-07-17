use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::{DiscordClient, discord::AppCommand};

use super::{
    gateway_commands, history_commands, media_commands, message_commands, notification_commands,
    read_state_commands, session_commands, user_commands, voice_commands,
};

const MAX_CONCURRENT_ATTACHMENT_PREVIEWS: usize = 4;
const MAX_CONCURRENT_ATTACHMENT_DOWNLOADS: usize = 2;

#[derive(Clone)]
pub(super) struct CommandDispatcher {
    client: DiscordClient,
    attachment_preview_permits: Arc<Semaphore>,
    attachment_download_permits: Arc<Semaphore>,
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
        }
    }

    pub(super) async fn dispatch(&self, command: AppCommand) {
        if runs_inline(&command) {
            self.handle(command).await;
        } else {
            let dispatcher = Self {
                client: self.client.clone(),
                attachment_preview_permits: self.attachment_preview_permits.clone(),
                attachment_download_permits: self.attachment_download_permits.clone(),
            };
            tokio::spawn(async move {
                dispatcher.handle(command).await;
            });
        }
    }

    async fn handle(&self, command: AppCommand) {
        match command {
            command @ (AppCommand::LoadMessageHistory { .. }
            | AppCommand::RefreshMessageHistory { .. }
            | AppCommand::LoadMessageHistoryAfter { .. }
            | AppCommand::LoadMessageHistoryAround { .. }
            | AppCommand::LoadThreadPreview { .. }
            | AppCommand::LoadForumPosts { .. }
            | AppCommand::LoadInboxMentions { .. }
            | AppCommand::LoadInboxChannelHistory { .. }
            | AppCommand::SearchMessages { .. }) => {
                history_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadGuildMembersByIds { .. }
            | AppCommand::SearchGuildMembers { .. }
            | AppCommand::SetSelectedGuild { .. }
            | AppCommand::SetSelectedMessageChannel { .. }
            | AppCommand::SubscribeDirectMessage { .. }
            | AppCommand::SubscribeGuildChannel { .. }
            | AppCommand::UpdateMemberListSubscription { .. }) => {
                gateway_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::JoinVoiceChannel { .. }
            | AppCommand::UpdateVoiceState { .. }
            | AppCommand::UpdateVoiceCapturePermission { .. }
            | AppCommand::LeaveVoiceChannel { .. }) => {
                voice_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadAttachmentPreview { .. }
            | AppCommand::LoadProfileAvatarPreview { .. }
            | AppCommand::OpenUrl { .. }
            | AppCommand::PlayMedia { .. }
            | AppCommand::DownloadAttachment { .. }) => {
                media_commands::handle(
                    self.client.clone(),
                    command,
                    self.attachment_preview_permits.clone(),
                    self.attachment_download_permits.clone(),
                )
                .await;
            }
            command @ (AppCommand::SendMessage { .. }
            | AppCommand::TriggerTyping { .. }
            | AppCommand::CreateForumPost { .. }
            | AppCommand::SetThreadArchived { .. }
            | AppCommand::SetThreadLocked { .. }
            | AppCommand::SetThreadPinned { .. }
            | AppCommand::DeleteThread { .. }
            | AppCommand::EditThread { .. }
            | AppCommand::SendTtsMessage { .. }
            | AppCommand::LoadApplicationCommands { .. }
            | AppCommand::RunApplicationCommand { .. }
            | AppCommand::EditMessage { .. }
            | AppCommand::DeleteMessage { .. }
            | AppCommand::RemoveMessageEmbeds { .. }
            | AppCommand::LeaveGuild { .. }
            | AppCommand::AddReaction { .. }
            | AppCommand::RemoveReaction { .. }
            | AppCommand::LoadReactionUsers { .. }
            | AppCommand::LoadPinnedMessages { .. }
            | AppCommand::SetMessagePinned { .. }
            | AppCommand::VotePoll { .. }) => {
                message_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::LoadUserProfile { .. }
            | AppCommand::LoadUserNote { .. }
            | AppCommand::UpdateUserProfile { .. }
            | AppCommand::UpdateCurrentUserStatus { .. }
            | AppCommand::UpdateGuildFolderSettings { .. }
            | AppCommand::UpdateCurrentUserActivity { .. }) => {
                user_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::AckChannel { .. }
            | AppCommand::ScheduleAckChannel { .. }
            | AppCommand::AckChannels { .. }) => {
                read_state_commands::handle(self.client.clone(), command).await;
            }
            command @ (AppCommand::SetGuildMuted { .. }
            | AppCommand::SetChannelMuted { .. }
            | AppCommand::SetThreadMuted { .. }
            | AppCommand::SetThreadFollowed { .. }
            | AppCommand::SetThreadNotificationLevel { .. }) => {
                notification_commands::handle(self.client.clone(), command).await;
            }
            command @ AppCommand::SignOut => {
                session_commands::handle(self.client.clone(), command).await;
            }
        }
    }
}

fn runs_inline(command: &AppCommand) -> bool {
    matches!(
        command,
        AppCommand::SetSelectedGuild { .. }
            | AppCommand::SetSelectedMessageChannel { .. }
            | AppCommand::UpdateVoiceCapturePermission { .. }
    )
}

#[cfg(test)]
mod tests {
    use crate::discord::{MicrophoneSensitivityDb, VoiceScope, VoiceVolumePercent, ids::Id};

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
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
        }));

        assert!(!runs_inline(&AppCommand::LoadMessageHistory {
            channel_id: Id::new(2),
            before: None,
        }));
        assert!(!runs_inline(&AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/avatar.png".to_owned(),
        }));
    }
}
