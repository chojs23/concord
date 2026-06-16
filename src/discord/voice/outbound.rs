use super::rtp::{VoiceOutboundRtpState, VoiceRtpEncryptor};

#[allow(dead_code)]
pub(super) struct VoiceOutboundSendState {
    pub(super) rtp: VoiceOutboundRtpState,
    pub(super) encryptor: VoiceRtpEncryptor,
    pub(super) nonce_suffix: u32,
    pub(super) allow_microphone_transmit: bool,
    pub(super) self_mute: bool,
    pub(super) dave_active: bool,
    pub(super) speaking: bool,
    pub(super) logged_block_reason: Option<VoiceOutboundSendBlockReason>,
    pub(super) events: Vec<VoiceOutboundSendEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum VoiceOutboundSendEvent {
    Speaking { speaking: bool, ssrc: u32 },
    Packet { bytes: Vec<u8> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum VoiceOutboundSendOutcome {
    Noop,
    Sent,
    Blocked(VoiceOutboundSendBlockReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
pub(super) enum VoiceOutboundSendBlockReason {
    DaveOutboundUnsupported,
    DaveOutboundMissingSession,
    DaveOutboundNotReady,
    DaveOutboundEncryptFailed,
}
