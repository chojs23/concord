use std::collections::BTreeMap;

use crate::discord::VoiceStateInfo;
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVoiceConnectionState {
    pub guild_id: Id<GuildMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
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
        }
    }

    pub(super) fn update_voice_state(&mut self, state: &VoiceStateInfo) {
        let key = (state.guild_id, state.user_id);
        if let Some(channel_id) = state.channel_id {
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
                },
            );
        } else {
            self.voice.states.remove(&key);
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
