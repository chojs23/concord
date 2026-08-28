//! User-tunable voice levels. These are voice domain values that the config
//! layer parses and the audio pipeline consumes, clamped on construction so
//! invalid settings can never reach the capture or playback paths.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MicrophoneSensitivityDb(i8);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VoiceVolumePercent(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VoiceParticipantVolumePercent(u16);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceParticipantPlaybackSettings {
    pub volume: VoiceParticipantVolumePercent,
    pub muted: bool,
}

const MIN_MICROPHONE_SENSITIVITY_DB: i8 = -100;
const MAX_MICROPHONE_SENSITIVITY_DB: i8 = 0;
const DEFAULT_MICROPHONE_SENSITIVITY_DB: i8 = -30;
const MIN_VOICE_VOLUME_PERCENT: u8 = 0;
const MAX_VOICE_VOLUME_PERCENT: u8 = 200;
const DEFAULT_VOICE_VOLUME_PERCENT: u8 = 100;
const MIN_VOICE_PARTICIPANT_VOLUME_PERCENT: u16 = 0;
const MAX_VOICE_PARTICIPANT_VOLUME_PERCENT: u16 = 200;
const DEFAULT_VOICE_PARTICIPANT_VOLUME_PERCENT: u16 = 100;

impl<'de> Deserialize<'de> for MicrophoneSensitivityDb {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw_db(i64::deserialize(deserializer)?))
    }
}

impl Default for MicrophoneSensitivityDb {
    fn default() -> Self {
        Self(DEFAULT_MICROPHONE_SENSITIVITY_DB)
    }
}

impl<'de> Deserialize<'de> for VoiceVolumePercent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw_percent(i64::deserialize(deserializer)?))
    }
}

impl Default for VoiceVolumePercent {
    fn default() -> Self {
        Self(DEFAULT_VOICE_VOLUME_PERCENT)
    }
}

impl<'de> Deserialize<'de> for VoiceParticipantVolumePercent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from_raw_percent(i64::deserialize(deserializer)?))
    }
}

impl Default for VoiceParticipantVolumePercent {
    fn default() -> Self {
        Self(DEFAULT_VOICE_PARTICIPANT_VOLUME_PERCENT)
    }
}

impl MicrophoneSensitivityDb {
    pub fn new(value: i8) -> Self {
        Self::from_raw_db(i64::from(value))
    }

    fn from_raw_db(value: i64) -> Self {
        Self(value.clamp(
            i64::from(MIN_MICROPHONE_SENSITIVITY_DB),
            i64::from(MAX_MICROPHONE_SENSITIVITY_DB),
        ) as i8)
    }

    pub fn value(self) -> i8 {
        self.0
    }

    pub fn label(self) -> String {
        format!("{} dB", self.0)
    }

    pub fn adjust(self, delta: i8) -> Self {
        Self::new(self.0.saturating_add(delta))
    }

    pub fn peak_threshold(self) -> i32 {
        let ratio = 10_f64.powf(f64::from(self.0) / 20.0);
        (f64::from(i16::MAX) * ratio).round() as i32
    }
}

impl VoiceVolumePercent {
    pub const fn maximum() -> u8 {
        MAX_VOICE_VOLUME_PERCENT
    }

    pub fn new(value: u8) -> Self {
        Self(value.clamp(MIN_VOICE_VOLUME_PERCENT, MAX_VOICE_VOLUME_PERCENT))
    }

    fn from_raw_percent(value: i64) -> Self {
        Self(value.clamp(
            i64::from(MIN_VOICE_VOLUME_PERCENT),
            i64::from(MAX_VOICE_VOLUME_PERCENT),
        ) as u8)
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn label(self) -> String {
        format!("{}%", self.0)
    }

    pub fn adjust(self, delta: i8) -> Self {
        if delta.is_negative() {
            Self::new(self.0.saturating_sub(delta.unsigned_abs()))
        } else {
            Self::new(self.0.saturating_add(delta as u8))
        }
    }

    pub fn gain(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

impl VoiceParticipantVolumePercent {
    pub const fn maximum() -> u16 {
        MAX_VOICE_PARTICIPANT_VOLUME_PERCENT
    }

    pub fn new(value: u16) -> Self {
        Self(value.clamp(
            MIN_VOICE_PARTICIPANT_VOLUME_PERCENT,
            MAX_VOICE_PARTICIPANT_VOLUME_PERCENT,
        ))
    }

    fn from_raw_percent(value: i64) -> Self {
        Self(value.clamp(
            i64::from(MIN_VOICE_PARTICIPANT_VOLUME_PERCENT),
            i64::from(MAX_VOICE_PARTICIPANT_VOLUME_PERCENT),
        ) as u16)
    }

    pub fn value(self) -> u16 {
        self.0
    }

    pub fn label(self) -> String {
        format!("{}%", self.0)
    }

    pub fn adjust(self, delta: i16) -> Self {
        if delta.is_negative() {
            Self::new(self.0.saturating_sub(delta.unsigned_abs()))
        } else {
            Self::new(self.0.saturating_add(delta as u16))
        }
    }

    pub fn gain(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}
