use crate::discord::AppCommand;

use super::{
    DashboardState, VoiceActionItem, VoiceActionKind,
    popups::VoiceLeaderActionState,
    scroll::clamp_selected_index,
};

impl DashboardState {
    pub fn open_voice_actions(&mut self) {
        self.close_all_action_contexts();
        self.voice_leader_action = Some(VoiceLeaderActionState { selected: 0 });
        self.open_leader_action_mode();
    }

    pub fn is_voice_leader_action_active(&self) -> bool {
        self.voice_leader_action.is_some()
    }

    pub fn selected_voice_action_items(&self) -> Vec<VoiceActionItem> {
        let joined_here = self
            .voice_connection
            .is_some_and(|voice| voice.channel_id.is_some());
        vec![
            VoiceActionItem {
                kind: VoiceActionKind::QuickDeafen,
                label: if self.voice_options.self_deaf {
                    "Undeafen voice".to_owned()
                } else {
                    "Deafen voice".to_owned()
                },
                enabled: true,
            },
            VoiceActionItem {
                kind: VoiceActionKind::QuickMute,
                label: if self.voice_options.self_mute {
                    "Unmute voice".to_owned()
                } else {
                    "Mute voice".to_owned()
                },
                enabled: true,
            },
            VoiceActionItem {
                kind: VoiceActionKind::QuickLeave,
                label: "Leave voice".to_owned(),
                enabled: joined_here,
            },
        ]
    }

    pub fn activate_voice_action_shortcut(&mut self, shortcut: char) -> Option<AppCommand> {
        let actions = self.selected_voice_action_items();
        let index = actions.iter().enumerate().find_map(|(index, action)| {
            let matches = action.enabled
                && super::voice_action_shortcut(&actions, index)
                    .is_some_and(|candidate| candidate == shortcut);
            matches.then_some(index)
        })?;
        self.activate_voice_action(index)
    }

    fn activate_voice_action(&mut self, index: usize) -> Option<AppCommand> {
        self.voice_leader_action.as_ref()?;
        let selected = clamp_selected_index(index, self.selected_voice_action_items().len());
        let item = self.selected_voice_action_items().get(selected)?.clone();
        if !item.enabled {
            return None;
        }

        match item.kind {
            VoiceActionKind::QuickDeafen => {
                self.voice_options.self_deaf = !self.voice_options.self_deaf;
                self.options_save_pending = true;
                self.queue_current_voice_state_update();
                self.voice_leader_action = None;
                None
            }
            VoiceActionKind::QuickMute => {
                self.voice_options.self_mute = !self.voice_options.self_mute;
                self.options_save_pending = true;
                self.queue_current_voice_state_update();
                self.voice_leader_action = None;
                None
            }
            VoiceActionKind::QuickLeave => {
                self.voice_leader_action = None;
                let voice = self.voice_connection?;
                voice.channel_id?;
                Some(AppCommand::LeaveVoiceChannel {
                    guild_id: voice.guild_id,
                    self_mute: self.voice_options.self_mute,
                    self_deaf: self.voice_options.self_deaf,
                })
            }
        }
    }

    pub(super) fn open_leader_action_mode(&mut self) {
        self.leader_mode = Some(super::LeaderMode::Actions);
    }
}
