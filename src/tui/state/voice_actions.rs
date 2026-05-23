use crate::discord::AppCommand;

use super::DashboardState;

impl DashboardState {
    pub fn toggle_voice_deafen(&mut self) {
        self.options.voice_options.self_deaf = !self.options.voice_options.self_deaf;
        self.options.options_save_pending = true;
        self.queue_current_voice_state_update();
    }

    pub fn toggle_voice_mute(&mut self) {
        self.options.voice_options.self_mute = !self.options.voice_options.self_mute;
        self.options.options_save_pending = true;
        self.queue_current_voice_state_update();
    }

    pub fn leave_current_voice_channel_command(&self) -> Option<AppCommand> {
        let voice = self.runtime.voice_connection?;
        voice.channel_id?;
        Some(AppCommand::LeaveVoiceChannel {
            guild_id: voice.guild_id,
            self_mute: self.options.voice_options.self_mute,
            self_deaf: self.options.voice_options.self_deaf,
        })
    }
}
