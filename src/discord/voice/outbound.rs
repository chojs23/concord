use super::{
    DISCORD_OPUS_SILENCE_FRAME, DISCORD_OPUS_TIMESTAMP_INCREMENT, DISCORD_TRAILING_SILENCE_FRAMES,
    RTP_AEAD_NONCE_SUFFIX_BYTES,
    dave::{VoiceDaveOutboundPayload, VoiceDaveState},
    rtp::{VoiceOutboundRtpState, VoiceRtpEncryptor, build_voice_rtp_packet},
};

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

#[allow(dead_code)]
impl VoiceOutboundSendState {
    pub(super) fn new(
        mode: &str,
        secret_key: &[u8],
        rtp: VoiceOutboundRtpState,
        nonce_suffix: u32,
    ) -> Result<Self, String> {
        Ok(Self {
            rtp,
            encryptor: VoiceRtpEncryptor::new(mode, secret_key)?,
            nonce_suffix,
            allow_microphone_transmit: false,
            self_mute: true,
            dave_active: false,
            speaking: false,
            logged_block_reason: None,
            events: Vec::new(),
        })
    }

    pub(super) fn set_capture_gate(&mut self, allow_microphone_transmit: bool, self_mute: bool) {
        self.allow_microphone_transmit = allow_microphone_transmit;
        self.self_mute = self_mute;
    }

    #[allow(dead_code)]
    pub(super) fn set_dave_active(&mut self, active: bool) {
        self.dave_active = active;
    }

    #[allow(dead_code)]
    pub(super) fn events(&self) -> &[VoiceOutboundSendEvent] {
        &self.events
    }

    #[allow(dead_code)]
    pub(super) fn take_events(&mut self) -> Vec<VoiceOutboundSendEvent> {
        std::mem::take(&mut self.events)
    }

    #[allow(dead_code)]
    pub(super) fn record_blocked_transmit(&mut self, reason: VoiceOutboundSendBlockReason) -> bool {
        if self.logged_block_reason == Some(reason) {
            return false;
        }
        self.logged_block_reason = Some(reason);
        true
    }

    #[allow(dead_code)]
    pub(super) fn take_logged_block_reason(&mut self) -> Option<VoiceOutboundSendBlockReason> {
        self.logged_block_reason.take()
    }

    #[allow(dead_code)]
    pub(super) fn send_opus_frame(
        &mut self,
        opus_payload: &[u8],
    ) -> Result<VoiceOutboundSendOutcome, String> {
        self.send_opus_frame_with_dave_payload(VoiceDaveOutboundPayload::Plain(
            opus_payload.to_vec(),
        ))
    }

    pub(super) fn send_opus_frame_with_dave(
        &mut self,
        opus_payload: &[u8],
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceOutboundSendOutcome, String> {
        let dave_payload = dave.prepare_outbound_opus(opus_payload);
        self.send_opus_frame_with_dave_payload(dave_payload)
    }

    pub(super) fn send_opus_frame_with_dave_payload(
        &mut self,
        dave_payload: VoiceDaveOutboundPayload,
    ) -> Result<VoiceOutboundSendOutcome, String> {
        if !self.capture_gate_enabled() {
            return Ok(VoiceOutboundSendOutcome::Noop);
        }
        if self.dave_active {
            return Ok(VoiceOutboundSendOutcome::Blocked(
                VoiceOutboundSendBlockReason::DaveOutboundUnsupported,
            ));
        }
        let opus_payload = match dave_payload {
            VoiceDaveOutboundPayload::Plain(opus) | VoiceDaveOutboundPayload::Encrypted(opus) => {
                opus
            }
            VoiceDaveOutboundPayload::Blocked(reason) => {
                return Ok(VoiceOutboundSendOutcome::Blocked(reason));
            }
        };

        let encrypted = self.encrypt_current_packet(&opus_payload)?;
        if !self.speaking {
            self.events.push(VoiceOutboundSendEvent::Speaking {
                speaking: true,
                ssrc: self.rtp.ssrc,
            });
            self.speaking = true;
        }
        self.events
            .push(VoiceOutboundSendEvent::Packet { bytes: encrypted });
        self.advance_packet_state();
        Ok(VoiceOutboundSendOutcome::Sent)
    }

    #[allow(dead_code)]
    pub(super) fn stop_speaking(&mut self) -> Result<VoiceOutboundSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            VoiceDaveOutboundPayload::Plain(DISCORD_OPUS_SILENCE_FRAME.to_vec())
        })
    }

    pub(super) fn stop_speaking_with_dave(
        &mut self,
        dave: &mut VoiceDaveState,
    ) -> Result<VoiceOutboundSendOutcome, String> {
        self.stop_speaking_with_dave_payload(|| {
            dave.prepare_outbound_opus(&DISCORD_OPUS_SILENCE_FRAME)
        })
    }

    pub(super) fn stop_speaking_with_dave_payload(
        &mut self,
        mut next_silence: impl FnMut() -> VoiceDaveOutboundPayload,
    ) -> Result<VoiceOutboundSendOutcome, String> {
        if !self.speaking {
            return Ok(VoiceOutboundSendOutcome::Noop);
        }
        if !self.capture_gate_enabled() {
            return Ok(self.queue_speaking_off());
        }
        if self.dave_active {
            return Ok(self.queue_speaking_off());
        }
        if self
            .ensure_nonce_capacity(DISCORD_TRAILING_SILENCE_FRAMES)
            .is_err()
        {
            return Ok(self.queue_speaking_off());
        }

        for _ in 0..DISCORD_TRAILING_SILENCE_FRAMES {
            let opus_payload = match next_silence() {
                VoiceDaveOutboundPayload::Plain(opus)
                | VoiceDaveOutboundPayload::Encrypted(opus) => opus,
                VoiceDaveOutboundPayload::Blocked(_) => {
                    return Ok(self.queue_speaking_off());
                }
            };
            let encrypted = self.encrypt_current_packet(&opus_payload)?;
            self.events
                .push(VoiceOutboundSendEvent::Packet { bytes: encrypted });
            self.advance_packet_state();
        }
        Ok(self.queue_speaking_off())
    }

    pub(super) fn queue_speaking_off(&mut self) -> VoiceOutboundSendOutcome {
        self.events.push(VoiceOutboundSendEvent::Speaking {
            speaking: false,
            ssrc: self.rtp.ssrc,
        });
        self.speaking = false;
        VoiceOutboundSendOutcome::Sent
    }

    pub(super) fn capture_gate_enabled(&self) -> bool {
        self.allow_microphone_transmit && !self.self_mute
    }

    pub(super) fn encrypt_current_packet(&self, opus_payload: &[u8]) -> Result<Vec<u8>, String> {
        let nonce_suffix = self.current_nonce_suffix()?;
        let packet = build_voice_rtp_packet(
            self.rtp.sequence,
            self.rtp.timestamp,
            self.rtp.ssrc,
            opus_payload,
        )?;
        self.encryptor.encrypt_packet(&packet, nonce_suffix)
    }

    pub(super) fn current_nonce_suffix(&self) -> Result<[u8; RTP_AEAD_NONCE_SUFFIX_BYTES], String> {
        if self.nonce_suffix == u32::MAX {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(self.nonce_suffix.to_be_bytes())
    }

    pub(super) fn ensure_nonce_capacity(&self, packets: usize) -> Result<(), String> {
        let remaining = u32::MAX - self.nonce_suffix;
        if remaining < packets as u32 {
            return Err("voice RTP nonce suffix exhausted".to_owned());
        }
        Ok(())
    }

    pub(super) fn advance_packet_state(&mut self) {
        self.rtp.sequence = self.rtp.sequence.wrapping_add(1);
        self.rtp.timestamp = self
            .rtp
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
        self.nonce_suffix = self.nonce_suffix.saturating_add(1);
    }
}
