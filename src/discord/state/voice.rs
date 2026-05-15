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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VoiceState {
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    deaf: bool,
    mute: bool,
    self_deaf: bool,
    self_mute: bool,
}

impl DiscordState {
    pub fn voice_participants_for_channel(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<VoiceParticipantState> {
        let mut participants: Vec<VoiceParticipantState> = self
            .voice
            .states
            .iter()
            .filter(|((state_guild_id, _), state)| {
                *state_guild_id == guild_id && state.channel_id == channel_id
            })
            .map(|(_, state)| VoiceParticipantState {
                user_id: state.user_id,
                display_name: self
                    .member_display_name(guild_id, state.user_id)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("user-{}", state.user_id.get())),
                deaf: state.deaf,
                mute: state.mute,
                self_deaf: state.self_deaf,
                self_mute: state.self_mute,
            })
            .collect();
        participants.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then(left.user_id.cmp(&right.user_id))
        });
        participants
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
