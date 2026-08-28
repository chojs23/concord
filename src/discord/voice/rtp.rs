use aes_gcm::{
    Aes256Gcm, Nonce as AesGcmNonce,
    aead::{Aead, KeyInit, Payload},
};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use super::{
    AEAD_AES256_GCM_RTPSIZE, AEAD_XCHACHA20_POLY1305_RTPSIZE, DISCORD_OPUS_TIMESTAMP_INCREMENT,
    DISCORD_VOICE_PAYLOAD_TYPE, RTCP_MIN_PACKET_BYTES, RTCP_SENDER_SSRC_BYTES,
    RTCP_SENDER_SSRC_OFFSET, RTP_AEAD_NONCE_SUFFIX_BYTES, RTP_AEAD_TAG_BYTES,
    RTP_EXTENSION_WORD_BYTES, RTP_HEADER_EXTENSION_BYTES, RTP_HEADER_MIN_LEN, RTP_VERSION,
};

const RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RtpHeader {
    pub(super) has_padding: bool,
    pub(super) marker: bool,
    pub(super) payload_type: u8,
    pub(super) sequence: u16,
    pub(super) timestamp: u32,
    pub(super) ssrc: u32,
    pub(super) authenticated_header_len: usize,
    pub(super) encrypted_extension_body_len: usize,
    pub(super) payload_offset: usize,
}

pub(super) enum VoiceRtpDecryptor {
    Aes256Gcm(Box<Aes256Gcm>),
    XChaCha20Poly1305(XChaCha20Poly1305),
}

#[allow(dead_code)]
pub(super) enum VoiceRtpEncryptor {
    Aes256Gcm(Box<Aes256Gcm>),
    XChaCha20Poly1305(XChaCha20Poly1305),
}

pub(super) struct DecryptedRtpPayload {
    pub(super) media_payload: Vec<u8>,
    pub(super) encrypted_extension_body_len: usize,
    pub(super) extension_profile: Option<u16>,
    pub(super) extension_body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) struct VoiceOutboundRtpState {
    pub(super) sequence: u16,
    pub(super) timestamp: u32,
    pub(super) ssrc: u32,
}

#[cfg(test)]
#[allow(dead_code)]
impl VoiceOutboundRtpState {
    pub(super) fn test() -> Self {
        Self {
            sequence: 0,
            timestamp: 0,
            ssrc: 0,
        }
    }
}

impl VoiceRtpDecryptor {
    pub(super) fn new(mode: &str, secret_key: &[u8]) -> Result<Self, String> {
        match mode {
            AEAD_AES256_GCM_RTPSIZE => Aes256Gcm::new_from_slice(secret_key)
                .map(|cipher| Self::Aes256Gcm(Box::new(cipher)))
                .map_err(|_| "voice AES-GCM key is invalid".to_owned()),
            AEAD_XCHACHA20_POLY1305_RTPSIZE => XChaCha20Poly1305::new_from_slice(secret_key)
                .map(Self::XChaCha20Poly1305)
                .map_err(|_| "voice XChaCha20-Poly1305 key is invalid".to_owned()),
            other => Err(format!("unsupported voice RTP decrypt mode: {other}")),
        }
    }

    pub(super) fn decrypt_packet(
        &self,
        packet: &[u8],
        header: &RtpHeader,
    ) -> Result<DecryptedRtpPayload, String> {
        if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE {
            return Err(format!(
                "RTP packet has unsupported payload type: {}",
                header.payload_type
            ));
        }
        self.decrypt_packet_any(packet, header)
    }

    pub(super) fn decrypt_packet_any(
        &self,
        packet: &[u8],
        header: &RtpHeader,
    ) -> Result<DecryptedRtpPayload, String> {
        let sealed_end = packet
            .len()
            .checked_sub(RTP_AEAD_NONCE_SUFFIX_BYTES)
            .ok_or_else(|| "RTP packet is missing nonce suffix".to_owned())?;
        if sealed_end < header.authenticated_header_len + RTP_AEAD_TAG_BYTES {
            return Err("RTP packet is too short for encrypted payload".to_owned());
        }
        let nonce_suffix = &packet[sealed_end..];
        let sealed_payload = &packet[header.authenticated_header_len..sealed_end];
        let aad = &packet[..header.authenticated_header_len];
        let decrypted = self.decrypt_authenticated_payload(aad, sealed_payload, nonce_suffix)?;
        if decrypted.len() < header.encrypted_extension_body_len {
            return Err("decrypted RTP payload is shorter than extension body".to_owned());
        }
        let extension_profile = if header.encrypted_extension_body_len == 0 {
            None
        } else {
            let profile_offset = header
                .authenticated_header_len
                .checked_sub(RTP_HEADER_EXTENSION_BYTES)
                .ok_or_else(|| "RTP extension header offset underflowed".to_owned())?;
            Some(u16::from_be_bytes([
                packet[profile_offset],
                packet[profile_offset + 1],
            ]))
        };
        let extension_body = decrypted[..header.encrypted_extension_body_len].to_vec();
        let media_payload = &decrypted[header.encrypted_extension_body_len..];
        let media_payload = strip_rtp_padding(media_payload, header.has_padding)?;
        Ok(DecryptedRtpPayload {
            media_payload: media_payload.to_vec(),
            encrypted_extension_body_len: header.encrypted_extension_body_len,
            extension_profile,
            extension_body,
        })
    }

    pub(super) fn decrypt_rtcp_feedback(&self, packet: &[u8]) -> Result<Vec<u8>, String> {
        if !looks_like_rtcp_packet(packet) {
            return Err("RTCP feedback packet has an invalid header".to_owned());
        }
        let sealed_end = packet
            .len()
            .checked_sub(RTP_AEAD_NONCE_SUFFIX_BYTES)
            .ok_or_else(|| "RTCP feedback packet is missing nonce suffix".to_owned())?;
        if sealed_end < RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES + RTP_AEAD_TAG_BYTES {
            return Err("RTCP feedback packet is too short for encrypted body".to_owned());
        }

        let aad = &packet[..RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES];
        let sealed_payload = &packet[RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES..sealed_end];
        let nonce_suffix = &packet[sealed_end..];
        let plaintext = self.decrypt_authenticated_payload(aad, sealed_payload, nonce_suffix)?;

        let mut decrypted = Vec::with_capacity(aad.len() + plaintext.len());
        decrypted.extend_from_slice(aad);
        decrypted.extend_from_slice(&plaintext);
        validate_rtcp_compound_packet(&decrypted)?;
        Ok(decrypted)
    }

    fn decrypt_authenticated_payload(
        &self,
        aad: &[u8],
        sealed_payload: &[u8],
        nonce_suffix: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(match self {
            Self::Aes256Gcm(cipher) => {
                let mut nonce = [0u8; 12];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(nonce_suffix);
                cipher
                    .decrypt(
                        AesGcmNonce::from_slice(&nonce),
                        Payload {
                            msg: sealed_payload,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP AES-GCM decrypt failed".to_owned())?
            }
            Self::XChaCha20Poly1305(cipher) => {
                let mut nonce = [0u8; 24];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(nonce_suffix);
                cipher
                    .decrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: sealed_payload,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP XChaCha20-Poly1305 decrypt failed".to_owned())?
            }
        })
    }
}

fn strip_rtp_padding(payload: &[u8], has_padding: bool) -> Result<&[u8], String> {
    if !has_padding {
        return Ok(payload);
    }
    let padding_len = payload
        .last()
        .copied()
        .map(usize::from)
        .ok_or_else(|| "RTP padding flag is set on an empty payload".to_owned())?;
    if padding_len == 0 || padding_len > payload.len() {
        return Err("RTP packet has invalid padding length".to_owned());
    }
    Ok(&payload[..payload.len() - padding_len])
}

#[allow(dead_code)]
impl VoiceRtpEncryptor {
    pub(super) fn new(mode: &str, secret_key: &[u8]) -> Result<Self, String> {
        match mode {
            AEAD_AES256_GCM_RTPSIZE => Aes256Gcm::new_from_slice(secret_key)
                .map(|cipher| Self::Aes256Gcm(Box::new(cipher)))
                .map_err(|_| "voice AES-GCM key is invalid".to_owned()),
            AEAD_XCHACHA20_POLY1305_RTPSIZE => XChaCha20Poly1305::new_from_slice(secret_key)
                .map(Self::XChaCha20Poly1305)
                .map_err(|_| "voice XChaCha20-Poly1305 key is invalid".to_owned()),
            other => Err(format!("unsupported voice RTP encrypt mode: {other}")),
        }
    }

    pub(super) fn encrypt_packet(
        &self,
        packet: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        let header = parse_rtp_header(packet)?;
        if header.payload_type != DISCORD_VOICE_PAYLOAD_TYPE {
            return Err(format!(
                "RTP packet has unsupported payload type: {}",
                header.payload_type
            ));
        }
        self.encrypt_media_packet_with_header(packet, header, nonce_suffix)
    }

    pub(super) fn encrypt_media_packet(
        &self,
        packet: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        let header = parse_rtp_header(packet)?;
        self.encrypt_media_packet_with_header(packet, header, nonce_suffix)
    }

    fn encrypt_media_packet_with_header(
        &self,
        packet: &[u8],
        header: RtpHeader,
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        if packet.len() <= header.authenticated_header_len {
            return Err("RTP packet is missing media payload".to_owned());
        }

        let aad = &packet[..header.authenticated_header_len];
        let plaintext = &packet[header.authenticated_header_len..];
        self.encrypt_authenticated_payload(aad, plaintext, nonce_suffix)
    }

    pub(super) fn encrypt_rtcp_feedback(
        &self,
        packet: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        if !looks_like_rtcp_packet(packet) {
            return Err("RTCP feedback packet has an invalid header".to_owned());
        }
        if packet.len() <= RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES {
            return Err("RTCP feedback packet is missing feedback body".to_owned());
        }
        validate_rtcp_compound_packet(packet)?;

        let aad = &packet[..RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES];
        let plaintext = &packet[RTCP_FEEDBACK_AUTHENTICATED_HEADER_BYTES..];
        self.encrypt_authenticated_payload(aad, plaintext, nonce_suffix)
    }

    fn encrypt_authenticated_payload(
        &self,
        aad: &[u8],
        plaintext: &[u8],
        nonce_suffix: [u8; RTP_AEAD_NONCE_SUFFIX_BYTES],
    ) -> Result<Vec<u8>, String> {
        let sealed_payload = match self {
            Self::Aes256Gcm(cipher) => {
                let mut nonce = [0u8; 12];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        AesGcmNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP AES-GCM encrypt failed".to_owned())?
            }
            Self::XChaCha20Poly1305(cipher) => {
                let mut nonce = [0u8; 24];
                nonce[..RTP_AEAD_NONCE_SUFFIX_BYTES].copy_from_slice(&nonce_suffix);
                cipher
                    .encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: plaintext,
                            aad,
                        },
                    )
                    .map_err(|_| "RTP XChaCha20-Poly1305 encrypt failed".to_owned())?
            }
        };

        let mut encrypted =
            Vec::with_capacity(aad.len() + sealed_payload.len() + RTP_AEAD_NONCE_SUFFIX_BYTES);
        encrypted.extend_from_slice(aad);
        encrypted.extend_from_slice(&sealed_payload);
        encrypted.extend_from_slice(&nonce_suffix);
        Ok(encrypted)
    }
}

#[allow(dead_code)]
impl VoiceOutboundRtpState {
    pub(super) fn packetize(&mut self, opus_payload: &[u8]) -> Result<Vec<u8>, String> {
        let packet =
            build_voice_rtp_packet(self.sequence, self.timestamp, self.ssrc, opus_payload)?;
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self
            .timestamp
            .wrapping_add(DISCORD_OPUS_TIMESTAMP_INCREMENT);
        Ok(packet)
    }
}

#[allow(dead_code)]
pub(super) fn build_voice_rtp_packet(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    opus_payload: &[u8],
) -> Result<Vec<u8>, String> {
    build_voice_rtp_packet_with_marker(sequence, timestamp, ssrc, false, opus_payload)
}

#[allow(dead_code)]
pub(super) fn build_voice_rtp_packet_with_marker(
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
    marker: bool,
    opus_payload: &[u8],
) -> Result<Vec<u8>, String> {
    if opus_payload.is_empty() {
        return Err("voice RTP packet requires a non-empty Opus payload".to_owned());
    }

    let mut packet = Vec::with_capacity(RTP_HEADER_MIN_LEN + opus_payload.len());
    packet.push(RTP_VERSION << 6);
    packet.push(u8::from(marker) << 7 | DISCORD_VOICE_PAYLOAD_TYPE);
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&timestamp.to_be_bytes());
    packet.extend_from_slice(&ssrc.to_be_bytes());
    packet.extend_from_slice(opus_payload);
    Ok(packet)
}

pub(super) fn parse_rtp_header(packet: &[u8]) -> Result<RtpHeader, String> {
    if packet.len() < RTP_HEADER_MIN_LEN {
        return Err("RTP packet is too short".to_owned());
    }
    let version = packet[0] >> 6;
    if version != RTP_VERSION {
        return Err("RTP packet has unsupported version".to_owned());
    }
    if looks_like_rtcp_packet(packet) {
        return Err("RTP parser received RTCP packet".to_owned());
    }
    let has_extension = packet[0] & 0x10 != 0;
    let csrc_count = usize::from(packet[0] & 0x0f);
    let mut authenticated_header_len = RTP_HEADER_MIN_LEN + csrc_count * 4;
    if packet.len() < authenticated_header_len {
        return Err("RTP packet is shorter than CSRC list".to_owned());
    }
    let mut encrypted_extension_body_len = 0;
    if has_extension {
        if packet.len() < authenticated_header_len + RTP_HEADER_EXTENSION_BYTES {
            return Err("RTP packet is shorter than extension header".to_owned());
        }
        let extension_words = u16::from_be_bytes([
            packet[authenticated_header_len + 2],
            packet[authenticated_header_len + 3],
        ]);
        authenticated_header_len += RTP_HEADER_EXTENSION_BYTES;
        encrypted_extension_body_len = usize::from(extension_words) * RTP_EXTENSION_WORD_BYTES;
    }
    let payload_offset = authenticated_header_len + encrypted_extension_body_len;
    if packet.len() < payload_offset {
        return Err("RTP packet is shorter than extension body".to_owned());
    }

    Ok(RtpHeader {
        has_padding: packet[0] & 0x20 != 0,
        marker: packet[1] & 0x80 != 0,
        payload_type: packet[1] & 0x7f,
        sequence: u16::from_be_bytes([packet[2], packet[3]]),
        timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
        ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
        authenticated_header_len,
        encrypted_extension_body_len,
        payload_offset,
    })
}

pub(super) fn looks_like_rtcp_packet(packet: &[u8]) -> bool {
    packet.len() >= RTCP_MIN_PACKET_BYTES
        && packet[0] >> 6 == RTP_VERSION
        && (192..=223).contains(&packet[1])
}

fn validate_rtcp_compound_packet(packet: &[u8]) -> Result<(), String> {
    if packet.is_empty() {
        return Err("RTCP packet is empty".to_owned());
    }

    // Each packet in a compound RTCP datagram declares only its own length.
    // The datagram is valid when every subpacket ends at the next boundary.
    let mut offset = 0usize;
    while offset < packet.len() {
        let remaining = &packet[offset..];
        if remaining.len() < RTCP_MIN_PACKET_BYTES {
            return Err("RTCP packet is shorter than its header".to_owned());
        }
        if remaining[0] >> 6 != RTP_VERSION {
            return Err("RTCP packet has unsupported version".to_owned());
        }
        if !(192..=223).contains(&remaining[1]) {
            return Err("RTCP packet has invalid packet type".to_owned());
        }

        let packet_len = (usize::from(u16::from_be_bytes([remaining[2], remaining[3]])) + 1) * 4;
        if packet_len > remaining.len() {
            return Err("RTCP packet length exceeds received data".to_owned());
        }
        offset += packet_len;
    }

    Ok(())
}

pub(super) fn rtcp_sender_ssrc(packet: &[u8]) -> Option<u32> {
    let end = RTCP_SENDER_SSRC_OFFSET + RTCP_SENDER_SSRC_BYTES;
    (packet.len() >= end).then(|| {
        u32::from_be_bytes([
            packet[RTCP_SENDER_SSRC_OFFSET],
            packet[RTCP_SENDER_SSRC_OFFSET + 1],
            packet[RTCP_SENDER_SSRC_OFFSET + 2],
            packet[RTCP_SENDER_SSRC_OFFSET + 3],
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_rtcp_validation_rejects_malformed_subpackets() {
        let valid_pli = [0x81, 206, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2];
        let cases = [
            ("truncated header", vec![0x80, 206, 0]),
            ("unsupported version", vec![0x40, 206, 0, 0]),
            ("invalid packet type", vec![0x80, 100, 0, 0]),
            ("truncated body", vec![0x80, 206, 0, 2]),
        ];

        for (case, suffix) in cases {
            let mut packet = valid_pli.to_vec();
            packet.extend_from_slice(&suffix);

            assert!(
                validate_rtcp_compound_packet(&packet).is_err(),
                "{case} should be rejected"
            );
        }
    }
}
