use std::collections::BTreeMap;

use crate::config::{MicrophoneSensitivityDb, VoiceVolumePercent};
use crate::discord::{VoiceSoundKind, VoiceStateInfo};
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::DiscordState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceParticipantState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub deaf: bool,
    pub mute: bool,
    pub self_deaf: bool,
    pub self_mute: bool,
    pub self_stream: bool,
    pub speaking: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVoiceConnectionState {
    pub guild_id: Id<GuildMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VoiceState {
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    deaf: bool,
    mute: bool,
    self_deaf: bool,
    self_mute: bool,
    self_stream: bool,
    speaking: bool,
}

impl DiscordState {
    pub fn current_user_voice_connection(&self) -> Option<CurrentVoiceConnectionState> {
        let current_user_id = self.session.current_user_id?;
        self.voice
            .states
            .iter()
            .find_map(|((guild_id, user_id), state)| {
                (*user_id == current_user_id).then_some(CurrentVoiceConnectionState {
                    guild_id: *guild_id,
                    channel_id: state.channel_id,
                    self_mute: state.self_mute,
                    self_deaf: state.self_deaf,
                    allow_microphone_transmit: false,
                    microphone_sensitivity: MicrophoneSensitivityDb::default(),
                    microphone_volume: VoiceVolumePercent::default(),
                    voice_output_volume: VoiceVolumePercent::default(),
                })
            })
    }

    pub fn voice_participants_for_channel(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<VoiceParticipantState> {
        let mut participants = Vec::new();
        for ((state_guild_id, _), state) in &self.voice.states {
            if *state_guild_id == guild_id && state.channel_id == channel_id {
                participants.push(self.voice_participant_state(guild_id, state));
            }
        }
        sort_voice_participants(&mut participants);
        participants
    }

    pub fn current_user_voice_speaking(&self) -> bool {
        let Some(current_user_id) = self.session.current_user_id else {
            return false;
        };
        self.user_voice_speaking(current_user_id)
    }

    pub fn user_voice_speaking_in_guild(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> bool {
        self.voice
            .states
            .get(&(guild_id, user_id))
            .map(|state| state.speaking)
            .unwrap_or(false)
    }

    pub fn user_voice_channel_in_guild(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<Id<ChannelMarker>> {
        self.voice
            .states
            .get(&(guild_id, user_id))
            .map(|state| state.channel_id)
    }

    pub(crate) fn voice_sound_for_state_update(
        &self,
        state: &VoiceStateInfo,
    ) -> Option<VoiceSoundKind> {
        let before = self.user_voice_channel_in_guild(state.guild_id, state.user_id);
        let after = state.channel_id;
        if before == after {
            return None;
        }

        if self.session.current_user_id == Some(state.user_id) {
            return match (before, after) {
                (None, Some(_)) | (Some(_), Some(_)) => Some(VoiceSoundKind::Join),
                (Some(_), None) => Some(VoiceSoundKind::Leave),
                (None, None) => None,
            };
        }

        let active_voice_channel = self.current_user_voice_connection()?.channel_id;
        match (before == Some(active_voice_channel), after == Some(active_voice_channel)) {
            (false, true) => Some(VoiceSoundKind::Join),
            (true, false) => Some(VoiceSoundKind::Leave),
            _ => None,
        }
    }

    fn user_voice_speaking(&self, user_id: Id<UserMarker>) -> bool {
        self.voice
            .states
            .iter()
            .find_map(|((_, state_user_id), state)| {
                (*state_user_id == user_id).then_some(state.speaking)
            })
            .unwrap_or(false)
    }

    pub fn voice_participants_by_channel_for_guild(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> BTreeMap<Id<ChannelMarker>, Vec<VoiceParticipantState>> {
        let mut participants_by_channel: BTreeMap<Id<ChannelMarker>, Vec<VoiceParticipantState>> =
            BTreeMap::new();
        for ((state_guild_id, _), state) in &self.voice.states {
            if *state_guild_id != guild_id {
                continue;
            }
            participants_by_channel
                .entry(state.channel_id)
                .or_default()
                .push(self.voice_participant_state(guild_id, state));
        }
        for participants in participants_by_channel.values_mut() {
            sort_voice_participants(participants);
        }
        participants_by_channel
    }

    fn voice_participant_state(
        &self,
        guild_id: Id<GuildMarker>,
        state: &VoiceState,
    ) -> VoiceParticipantState {
        VoiceParticipantState {
            user_id: state.user_id,
            display_name: self
                .member_display_name(guild_id, state.user_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("user-{}", state.user_id.get())),
            deaf: state.deaf,
            mute: state.mute,
            self_deaf: state.self_deaf,
            self_mute: state.self_mute,
            self_stream: state.self_stream,
            speaking: state.speaking,
        }
    }

    pub(super) fn update_voice_state(&mut self, state: &VoiceStateInfo) {
        let key = (state.guild_id, state.user_id);
        if let Some(channel_id) = state.channel_id {
            let speaking = self
                .voice
                .states
                .get(&key)
                .is_some_and(|current| current.channel_id == channel_id && current.speaking);
            self.voice.states.insert(
                key,
                VoiceState {
                    channel_id,
                    user_id: state.user_id,
                    deaf: state.deaf,
                    mute: state.mute,
                    self_deaf: state.self_deaf,
                    self_mute: state.self_mute,
                    self_stream: state.self_stream,
                    speaking,
                },
            );
        } else {
            self.voice.states.remove(&key);
        }
    }

    pub(super) fn update_voice_speaking(
        &mut self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) {
        let Some(state) = self.voice.states.get_mut(&(guild_id, user_id)) else {
            return;
        };
        if state.channel_id == channel_id {
            state.speaking = speaking;
        }
    }

    pub(super) fn remove_voice_state(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) {
        self.voice.states.remove(&(guild_id, user_id));
    }

    pub(super) fn remove_voice_states_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.voice
            .states
            .retain(|(state_guild_id, _), _| *state_guild_id != guild_id);
    }

    pub(super) fn remove_voice_states_for_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.voice
            .states
            .retain(|_, state| state.channel_id != channel_id);
    }
}

fn sort_voice_participants(participants: &mut [VoiceParticipantState]) {
    participants.sort_by_cached_key(|participant| {
        (participant.display_name.to_lowercase(), participant.user_id)
    });
}
