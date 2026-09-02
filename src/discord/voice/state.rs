use std::collections::{BTreeMap, BTreeSet};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::discord::{MicrophoneSensitivityDb, VoiceVolumePercent};
use crate::discord::{
    StreamCreateInfo, StreamUpdateInfo, VoiceScope, VoiceSoundKind, VoiceStateInfo,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamParticipantList {
    pub(crate) paused: bool,
    pub(crate) broadcaster: String,
    pub(crate) viewers: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurrentVoiceConnectionState {
    pub scope: VoiceScope,
    pub channel_id: Id<ChannelMarker>,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub allow_microphone_transmit: bool,
    pub noise_suppression: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
}

/// Audio settings applied together to an active voice connection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct VoiceAudioSettings {
    pub allow_microphone_transmit: bool,
    pub noise_suppression: bool,
    pub microphone_sensitivity: MicrophoneSensitivityDb,
    pub microphone_volume: VoiceVolumePercent,
    pub voice_output_volume: VoiceVolumePercent,
}

impl CurrentVoiceConnectionState {
    /// The guild this connection belongs to, or `None` for a DM/group-DM call.
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.scope.guild_id()
    }

    pub(crate) fn audio_settings(self) -> VoiceAudioSettings {
        VoiceAudioSettings {
            allow_microphone_transmit: self.allow_microphone_transmit,
            noise_suppression: self.noise_suppression,
            microphone_sensitivity: self.microphone_sensitivity,
            microphone_volume: self.microphone_volume,
            voice_output_volume: self.voice_output_volume,
        }
    }

    pub(crate) fn set_audio_settings(&mut self, settings: VoiceAudioSettings) {
        self.allow_microphone_transmit = settings.allow_microphone_transmit;
        self.noise_suppression = settings.noise_suppression;
        self.microphone_sensitivity = settings.microphone_sensitivity;
        self.microphone_volume = settings.microphone_volume;
        self.voice_output_volume = settings.voice_output_volume;
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
            noise_suppression: false,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::discord) struct StreamState {
    pub(in crate::discord) scope: VoiceScope,
    pub(in crate::discord) channel_id: Id<ChannelMarker>,
    pub(in crate::discord) owner_id: Id<UserMarker>,
    pub(in crate::discord) viewer_ids: BTreeSet<Id<UserMarker>>,
    paused: bool,
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
                    noise_suppression: false,
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
            .map(|((scope, _), current)| (*scope, current));
        let before_channel = before.map(|(_, current)| current.channel_id);
        let after = state.channel_id;
        if self.session.current_user_id != Some(state.user_id)
            && let Some(active_voice) = self.current_user_voice_connection()
            && let Some((before_scope, current)) = before
            && before_scope == active_voice.scope
            && current.channel_id == active_voice.channel_id
            && after == Some(active_voice.channel_id)
            && !current.self_stream
            && state.self_stream
        {
            return Some(VoiceSoundKind::StreamStart);
        }

        if before_channel == after {
            return None;
        }

        if self.session.current_user_id == Some(state.user_id) {
            return match (before_channel, after) {
                (None, Some(_)) | (Some(_), Some(_)) => Some(VoiceSoundKind::Join),
                (Some(_), None) => Some(VoiceSoundKind::Leave),
                (None, None) => None,
            };
        }

        // For other users, only chime for the channel the current user is in.
        let active_voice_channel = self.current_user_voice_connection()?.channel_id;
        match (
            before_channel == Some(active_voice_channel),
            after == Some(active_voice_channel),
        ) {
            (false, true) => Some(VoiceSoundKind::Join),
            (true, false) => Some(VoiceSoundKind::Leave),
            _ => None,
        }
    }

    pub(crate) fn stream_viewer_sounds_for_update(
        &self,
        update: &StreamUpdateInfo,
    ) -> Vec<VoiceSoundKind> {
        let Some(current_user_id) = self.session.current_user_id else {
            return Vec::new();
        };
        let Some(stream) = self.voice.streams.get(&update.stream_key) else {
            return Vec::new();
        };
        if stream.owner_id != current_user_id && !stream.viewer_ids.contains(&current_user_id) {
            return Vec::new();
        }

        let next_viewers = update.viewer_ids.iter().copied().collect::<BTreeSet<_>>();
        let viewer_joined = next_viewers
            .difference(&stream.viewer_ids)
            .any(|user_id| *user_id != current_user_id);
        let viewer_left = stream
            .viewer_ids
            .difference(&next_viewers)
            .any(|user_id| *user_id != current_user_id);

        let mut sounds = Vec::with_capacity(2);
        if viewer_joined {
            sounds.push(VoiceSoundKind::StreamViewerJoin);
        }
        if viewer_left {
            sounds.push(VoiceSoundKind::StreamViewerLeave);
        }
        sounds
    }

    pub(crate) fn stream_participants(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        owner_id: Id<UserMarker>,
    ) -> StreamParticipantList {
        let stream = self.voice.streams.values().find(|stream| {
            stream.scope == scope && stream.channel_id == channel_id && stream.owner_id == owner_id
        });
        let paused = stream.is_some_and(|stream| stream.paused);
        let viewers = stream
            .into_iter()
            .flat_map(|stream| stream.viewer_ids.iter().copied())
            .filter(|user_id| *user_id != owner_id)
            .map(|user_id| self.stream_participant_display_name(scope, channel_id, user_id))
            .collect();
        StreamParticipantList {
            paused,
            broadcaster: self.stream_participant_display_name(scope, channel_id, owner_id),
            viewers,
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

    pub(crate) fn voice_participant_counts_by_channel_for_guild(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> BTreeMap<Id<ChannelMarker>, usize> {
        let scope = VoiceScope::Guild(guild_id);
        let mut counts = BTreeMap::new();
        for ((state_scope, _), state) in &self.voice.states {
            if *state_scope != scope {
                continue;
            }
            let count = counts.entry(state.channel_id).or_insert(0usize);
            *count = count.saturating_add(1);
        }
        counts
    }

    fn voice_participant_state(
        &self,
        scope: VoiceScope,
        state: &VoiceState,
    ) -> VoiceParticipantState {
        VoiceParticipantState {
            user_id: state.user_id,
            display_name: self
                .voice_participant_display_name(scope, state.channel_id, state.user_id)
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
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<String> {
        self.user_display_name_for_channel(channel_id, user_id)
            .or_else(|| match scope {
                VoiceScope::Guild(guild_id) => self
                    .member_display_name(guild_id, user_id)
                    .filter(|name| *name != "unknown")
                    .map(str::to_owned),
                VoiceScope::Private(_) => None,
            })
    }

    fn stream_participant_display_name(
        &self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> String {
        self.voice_participant_display_name(scope, channel_id, user_id)
            .unwrap_or_else(|| format!("user-{}", user_id.get()))
    }

    pub(in crate::discord) fn record_stream_create(&mut self, stream: &StreamCreateInfo) {
        self.record_stream_presence(&stream.stream_key, &stream.viewer_ids, stream.paused);
    }

    pub(in crate::discord) fn record_stream_update(&mut self, stream: &StreamUpdateInfo) {
        self.record_stream_presence(&stream.stream_key, &stream.viewer_ids, stream.paused);
    }

    fn record_stream_presence(
        &mut self,
        stream_key: &str,
        viewer_ids: &[Id<UserMarker>],
        paused: bool,
    ) {
        let Some((scope, channel_id, owner_id)) = parse_stream_location(stream_key) else {
            return;
        };
        self.voice_mut().streams.insert(
            stream_key.to_owned(),
            StreamState {
                scope,
                channel_id,
                owner_id,
                viewer_ids: viewer_ids.iter().copied().collect(),
                paused,
            },
        );
    }

    pub(in crate::discord) fn remove_stream(&mut self, stream_key: &str) {
        self.voice_mut().streams.remove(stream_key);
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
            self.voice_mut()
                .states
                .retain(|(state_scope, state_user_id), _| {
                    *state_user_id != user_id || *state_scope == scope
                });
            self.voice_mut().states.insert(
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
                    self.voice_mut()
                        .states
                        .remove(&(VoiceScope::Guild(guild_id), user_id));
                }
                None => {
                    self.voice_mut().states.retain(|(scope, state_user_id), _| {
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
        let Some(state) = self.voice_mut().states.get_mut(&(scope, user_id)) else {
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
        self.voice_mut()
            .states
            .remove(&(VoiceScope::Guild(guild_id), user_id));
    }

    pub(in crate::discord) fn remove_voice_states_for_guild(&mut self, guild_id: Id<GuildMarker>) {
        self.voice_mut()
            .states
            .retain(|(scope, _), _| *scope != VoiceScope::Guild(guild_id));
        self.voice_mut()
            .streams
            .retain(|_, stream| stream.scope != VoiceScope::Guild(guild_id));
    }

    pub(in crate::discord) fn remove_voice_states_for_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        self.voice_mut()
            .states
            .retain(|_, state| state.channel_id != channel_id);
        self.voice_mut()
            .streams
            .retain(|_, stream| stream.channel_id != channel_id);
    }

    fn clear_voice_speaking_for_channel(
        &mut self,
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
    ) {
        for ((state_scope, _), state) in &mut self.voice_mut().states {
            if *state_scope == scope && state.channel_id == channel_id {
                state.speaking = false;
            }
        }
    }
}

fn parse_stream_location(
    stream_key: &str,
) -> Option<(VoiceScope, Id<ChannelMarker>, Id<UserMarker>)> {
    let parts = stream_key.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["guild", guild_id, channel_id, owner_id] => {
            let guild_id = parse_stream_id::<GuildMarker>(guild_id)?;
            let channel_id = parse_stream_id::<ChannelMarker>(channel_id)?;
            let owner_id = parse_stream_id::<UserMarker>(owner_id)?;
            Some((VoiceScope::Guild(guild_id), channel_id, owner_id))
        }
        ["call", channel_id, owner_id] => {
            let channel_id = parse_stream_id::<ChannelMarker>(channel_id)?;
            let owner_id = parse_stream_id::<UserMarker>(owner_id)?;
            Some((VoiceScope::Private(channel_id), channel_id, owner_id))
        }
        _ => None,
    }
}

fn parse_stream_id<T>(raw: &str) -> Option<Id<T>> {
    raw.parse::<u64>().ok().and_then(Id::new_checked)
}

fn sort_voice_participants(participants: &mut [VoiceParticipantState]) {
    participants.sort_by_cached_key(|participant| {
        (participant.display_name.to_lowercase(), participant.user_id)
    });
}
