use std::collections::BTreeMap;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{MicrophoneSensitivityDb, VoiceVolumePercent};
use crate::discord::{VoiceScope, VoiceSoundKind, VoiceStateInfo};

use crate::discord::state::DiscordState;

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
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
}

impl CurrentVoiceConnectionState {
    /// The guild this connection belongs to, or `None` for a DM/group-DM call.
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.scope.guild_id()
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl CurrentVoiceConnectionState {
    pub(crate) fn test(guild_id: Id<GuildMarker>, channel_id: Id<ChannelMarker>) -> Self {
        Self {
            scope: VoiceScope::Guild(guild_id),
            channel_id,
            self_mute: false,
            self_deaf: false,
            allow_microphone_transmit: false,
            microphone_sensitivity: MicrophoneSensitivityDb::default(),
            microphone_volume: VoiceVolumePercent::default(),
            voice_output_volume: VoiceVolumePercent::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::discord) struct VoiceState {
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
            .find_map(|((scope, user_id), state)| {
                (*user_id == current_user_id).then_some(CurrentVoiceConnectionState {
                    scope: *scope,
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
        self.voice_participants_for_scope(VoiceScope::Guild(guild_id), channel_id)
    }

    /// Voice participants currently in a DM or group-DM call.
    pub fn voice_participants_for_private_channel(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<VoiceParticipantState> {
        self.voice_participants_for_scope(VoiceScope::Private(channel_id), channel_id)
    }

    fn voice_participants_for_scope(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<VoiceParticipantState> {
        let mut participants = Vec::new();
        for ((state_scope, _), state) in &self.voice.states {
            if *state_scope == scope && state.channel_id == channel_id {
                participants.push(self.voice_participant_state(scope, state));
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
            .get(&(VoiceScope::Guild(guild_id), user_id))
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
            .get(&(VoiceScope::Guild(guild_id), user_id))
            .map(|state| state.channel_id)
    }

    pub(crate) fn voice_sound_for_state_update(
        &self,
        state: &VoiceStateInfo,
    ) -> Option<VoiceSoundKind> {
        // Look the user up by id, not scope: a DM leave carries no location, so
        // their cached entry is the only way to know which call they left.
        let before = self
            .voice
            .states
            .iter()
            .find(|((_, user_id), _)| *user_id == state.user_id)
            .map(|(_, current)| current.channel_id);
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

        // For other users, only chime for the channel the current user is in.
        let active_voice_channel = self.current_user_voice_connection()?.channel_id;
        match (
            before == Some(active_voice_channel),
            after == Some(active_voice_channel),
        ) {
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
        let scope = VoiceScope::Guild(guild_id);
        let mut participants_by_channel: BTreeMap<Id<ChannelMarker>, Vec<VoiceParticipantState>> =
            BTreeMap::new();
        for ((state_scope, _), state) in &self.voice.states {
            if *state_scope != scope {
                continue;
            }
            participants_by_channel
                .entry(state.channel_id)
                .or_default()
                .push(self.voice_participant_state(scope, state));
        }
        for participants in participants_by_channel.values_mut() {
            sort_voice_participants(participants);
        }
        participants_by_channel
    }

    fn voice_participant_state(
        &self,
        scope: VoiceScope,
        state: &VoiceState,
    ) -> VoiceParticipantState {
        VoiceParticipantState {
            user_id: state.user_id,
            display_name: self
                .voice_participant_display_name(scope, state.user_id)
                .unwrap_or_else(|| format!("user-{}", state.user_id.get())),
            deaf: state.deaf,
            mute: state.mute,
            self_deaf: state.self_deaf,
            self_mute: state.self_mute,
            self_stream: state.self_stream,
            speaking: state.speaking,
        }
    }

    /// Resolve a participant's display name: guild voice via the member list, a
    /// DM via the channel recipients. The current user is special-cased because
    /// Discord omits self from a group DM's recipient list.
    fn voice_participant_display_name(
        &self,
        scope: VoiceScope,
        user_id: Id<UserMarker>,
    ) -> Option<String> {
        match scope {
            VoiceScope::Guild(guild_id) => self
                .member_display_name(guild_id, user_id)
                .map(str::to_owned),
            VoiceScope::Private(channel_id) => {
                if self.session.current_user_id == Some(user_id) {
                    if let Some(name) = self.session.current_user.clone() {
                        return Some(name);
                    }
                }
                self.channel(channel_id)?
                    .recipients
                    .iter()
                    .find(|recipient| recipient.user_id == user_id)
                    .map(|recipient| recipient.display_name.clone())
            }
        }
    }

    pub(in crate::discord) fn update_voice_state(&mut self, state: &VoiceStateInfo) {
        let user_id = state.user_id;
        let is_current_user = self.session.current_user_id == Some(user_id);

        // `None` only for a DM leave (null guild and channel), handled by the
        // user-id removal in the leave branch below.
        let scope = state.scope();

        // When the current user moves or leaves, clear stale speaking flags in
        // the channel they were in. Found by user id, not scope, so it also
        // covers cross-scope moves (DM A -> DM B) and DM leaves (no scope).
        if is_current_user
            && let Some((previous_scope, previous_channel_id)) = self
                .voice
                .states
                .iter()
                .find(|((_, state_user_id), _)| *state_user_id == user_id)
                .map(|((scope, _), current)| (*scope, current.channel_id))
            && state.channel_id != Some(previous_channel_id)
        {
            self.clear_voice_speaking_for_channel(previous_scope, previous_channel_id);
        }

        if let Some(channel_id) = state.channel_id {
            let scope = scope.expect("a voice state with a channel always has a scope");
            let key = (scope, user_id);
            let speaking = self
                .voice
                .states
                .get(&key)
                .is_some_and(|current| current.channel_id == channel_id && current.speaking);
            // Moving across scopes (DM A -> DM B, guild -> DM) changes the key,
            // and Discord sends only the new location, so drop any stale entry
            // this user still holds under a different scope.
            self.voice.states.retain(|(state_scope, state_user_id), _| {
                *state_user_id != user_id || *state_scope == scope
            });
            self.voice.states.insert(
                key,
                VoiceState {
                    channel_id,
                    user_id,
                    deaf: state.deaf,
                    mute: state.mute,
                    self_deaf: state.self_deaf,
                    self_mute: state.self_mute,
                    self_stream: state.self_stream,
                    speaking,
                },
            );
        } else {
            // A leave: a guild leave names its guild, a DM leave names nothing
            // so we drop every private entry this user holds.
            match state.guild_id {
                Some(guild_id) => {
                    self.voice
                        .states
                        .remove(&(VoiceScope::Guild(guild_id), user_id));
                }
                None => {
                    self.voice.states.retain(|(scope, state_user_id), _| {
                        !(matches!(scope, VoiceScope::Private(_)) && *state_user_id == user_id)
                    });
                }
            }
        }
    }

    pub(in crate::discord) fn update_voice_speaking(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        speaking: bool,
    ) {
        let Some(state) = self.voice.states.get_mut(&(scope, user_id)) else {
            return;
        };
        if state.channel_id == channel_id {
            state.speaking = speaking;
        }
    }

    pub(in crate::discord) fn remove_voice_state(
        &mut self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) {
        self.voice
            .states
            .remove(&(VoiceScope::Guild(guild_id), user_id));
    }

    pub(in crate::discord) fn remove_voice_states_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.voice
            .states
            .retain(|(scope, _), _| *scope != VoiceScope::Guild(guild_id));
    }

    pub(in crate::discord) fn remove_voice_states_for_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        self.voice
            .states
            .retain(|_, state| state.channel_id != channel_id);
    }

    fn clear_voice_speaking_for_channel(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        for ((state_scope, _), state) in &mut self.voice.states {
            if *state_scope == scope && state.channel_id == channel_id {
                state.speaking = false;
            }
        }
    }
}

fn sort_voice_participants(participants: &mut [VoiceParticipantState]) {
    participants.sort_by_cached_key(|participant| {
        (participant.display_name.to_lowercase(), participant.user_id)
    });
}
