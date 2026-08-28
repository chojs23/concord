use crate::discord::{
    AppCommand, DiscordAction, VoiceScope,
    ids::{Id, marker::ChannelMarker},
};

use super::DashboardState;

impl DashboardState {
    pub fn toggle_voice_deafen(&mut self) {
        self.options.voice_options.self_deaf = !self.options.voice_options.self_deaf;
        self.options.config_save_pending = true;
        self.queue_current_voice_state_update();
    }

    pub fn toggle_voice_mute(&mut self) {
        self.options.voice_options.self_mute = !self.options.voice_options.self_mute;
        self.options.config_save_pending = true;
        self.queue_current_voice_state_update();
    }

    pub fn leave_current_voice_channel_command(&self) -> Option<AppCommand> {
        let voice = self.runtime.voice_connection?;
        voice.channel_id?;
        Some(AppCommand::LeaveVoiceChannel {
            scope: voice.scope,
            self_mute: self.options.voice_options.self_mute,
            self_deaf: self.options.voice_options.self_deaf,
        })
    }

    pub(in crate::tui) fn stream_broadcast_active_for(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        let tracked_here = self
            .runtime
            .active_stream_broadcast
            .as_ref()
            .or(self.runtime.stream_broadcast_preparing.as_ref())
            .is_some_and(|target| target.matches(scope, channel_id));
        if tracked_here {
            return true;
        }

        let Some(current_user_id) = self.discord.cache.current_user_id() else {
            return false;
        };
        let participants = match scope {
            VoiceScope::Guild(guild_id) => self
                .discord
                .cache
                .voice_participants_for_channel(guild_id, channel_id),
            VoiceScope::Private(_) => self
                .discord
                .cache
                .voice_participants_for_private_channel(channel_id),
        };
        participants
            .iter()
            .any(|participant| participant.user_id == current_user_id && participant.self_stream)
    }

    pub(in crate::tui) fn stream_broadcast_action_label_for(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> &'static str {
        if self.stream_broadcast_active_for(scope, channel_id) {
            "Stop sharing"
        } else {
            "Share screen"
        }
    }

    pub(in crate::tui) fn current_voice_stream_action_label(&self) -> &'static str {
        let Some(voice) = self.runtime.voice_connection else {
            return "Share screen";
        };
        let Some(channel_id) = voice.channel_id else {
            return "Share screen";
        };
        self.stream_broadcast_action_label_for(voice.scope, channel_id)
    }

    pub(in crate::tui) fn toggle_current_voice_stream_command(&mut self) -> Option<AppCommand> {
        let voice = self.runtime.voice_connection?;
        let channel_id = voice.channel_id?;
        if self.stream_broadcast_active_for(voice.scope, channel_id) {
            self.record_stream_broadcast_ended(voice.scope, channel_id);
            return Some(AppCommand::StopVoiceStream {
                scope: voice.scope,
                channel_id,
            });
        }

        let channel = self.discord.cache.channel(channel_id)?;
        if channel.guild_id.is_some()
            && !self.discord_action_allowed_in_channel(channel_id, DiscordAction::StreamVoice)
        {
            return None;
        }
        let request_id = self.begin_stream_capture_targets_request(voice.scope, channel_id);
        Some(AppCommand::LoadStreamCaptureTargets {
            request_id,
            scope: voice.scope,
            channel_id,
        })
    }
}
