use std::{
    collections::{BTreeSet, HashMap},
    num::NonZeroU16,
};

use davey::{DaveSession, MediaType, ProposalsOperationType};
use serde_json::{Value, json};

use crate::discord::ids::{Id, marker::UserMarker};
use crate::logging;

use super::{
    DAVE_MAGIC_MARKER, DAVE_MIN_SUPPLEMENTAL_BYTES, VOICE_OP_CLIENT_DISCONNECT,
    VOICE_OP_CLIENT_FLAGS, VOICE_OP_CLIENT_PLATFORM, VOICE_OP_CLIENTS_CONNECT,
    VOICE_OP_DAVE_EXECUTE_TRANSITION, VOICE_OP_DAVE_MLS_ANNOUNCE_COMMIT_TRANSITION,
    VOICE_OP_DAVE_MLS_COMMIT_WELCOME, VOICE_OP_DAVE_MLS_EXTERNAL_SENDER,
    VOICE_OP_DAVE_MLS_INVALID_COMMIT_WELCOME, VOICE_OP_DAVE_MLS_KEY_PACKAGE,
    VOICE_OP_DAVE_MLS_PROPOSALS, VOICE_OP_DAVE_MLS_WELCOME, VOICE_OP_DAVE_PREPARE_EPOCH,
    VOICE_OP_DAVE_PREPARE_TRANSITION, VOICE_OP_DAVE_TRANSITION_READY, VOICE_OP_MEDIA_SINK_WANTS,
    VOICE_OP_SPEAKING, VoiceBinaryFrame, VoiceGatewaySession, VoiceOutboundSendBlockReason,
    VoiceWriter, send_voice_binary, send_voice_text,
};

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum VoiceDaveOutboundPayload {
    Plain(Vec<u8>),
    Encrypted(Vec<u8>),
    Blocked(VoiceOutboundSendBlockReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum VoiceMediaPayload {
    Plain(Vec<u8>),
    DaveUnexpectedPlain { payload_len: usize },
    DaveMissingUser { payload_len: usize },
    DaveNotReady { user_id: u64, payload_len: usize },
    DaveDecryptFailed { user_id: u64, message: String },
    DaveDecrypted { user_id: u64, opus: Vec<u8> },
}

impl VoiceMediaPayload {
    pub(super) fn pending_reason(&self) -> &'static str {
        match self {
            Self::DaveUnexpectedPlain { .. } => "DAVE active non-DAVE payload",
            Self::DaveMissingUser { .. } => "missing SSRC user mapping",
            Self::DaveNotReady { .. } => "DAVE session is not ready",
            _ => "not pending",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceSpeakingState {
    pub(super) user_id: Option<u64>,
    pub(super) ssrc: Option<u32>,
    pub(super) speaking: Option<u64>,
}

pub(super) struct VoiceDaveState {
    pub(super) user_id: u64,
    pub(super) channel_id: u64,
    pub(super) protocol_version: Option<NonZeroU16>,
    pub(super) session: Option<DaveSession>,
    pub(super) pending_transitions: HashMap<u16, u16>,
    pub(super) known_user_ids: BTreeSet<u64>,
    pub(super) ssrc_user_ids: HashMap<u32, u64>,
}

impl VoiceDaveState {
    pub(super) fn new(session: &VoiceGatewaySession) -> Self {
        let user_id = session.user_id.get();
        let mut known_user_ids = BTreeSet::new();
        known_user_ids.insert(user_id);
        Self {
            user_id,
            channel_id: session.channel_id.get(),
            protocol_version: None,
            session: None,
            pending_transitions: HashMap::new(),
            known_user_ids,
            ssrc_user_ids: HashMap::new(),
        }
    }

    pub(super) async fn handle_json_op(
        &mut self,
        writer: &VoiceWriter,
        opcode: u8,
        value: &Value,
    ) -> Result<(), String> {
        match opcode {
            VOICE_OP_SPEAKING => {
                self.handle_speaking_op(value);
            }
            VOICE_OP_CLIENTS_CONNECT => {
                for user_id in voice_user_ids(value) {
                    self.known_user_ids.insert(user_id);
                }
                logging::debug(
                    "voice",
                    format!(
                        "voice clients connected: known_users={}",
                        self.known_user_ids.len()
                    ),
                );
            }
            VOICE_OP_CLIENT_DISCONNECT => {
                if let Some(user_id) = voice_user_id(value) {
                    self.known_user_ids.remove(&user_id);
                    self.ssrc_user_ids
                        .retain(|_, mapped_user_id| *mapped_user_id != user_id);
                    logging::debug(
                        "voice",
                        format!(
                            "voice client disconnected: user_id={} known_users={} known_ssrcs={}",
                            user_id,
                            self.known_user_ids.len(),
                            self.ssrc_user_ids.len()
                        ),
                    );
                }
            }
            VOICE_OP_MEDIA_SINK_WANTS => {
                logging::debug(
                    "voice",
                    format!(
                        "voice media sink wants received: field_count={}",
                        voice_data_field_count(value)
                    ),
                );
            }
            VOICE_OP_CLIENT_FLAGS => {
                logging::debug(
                    "voice",
                    format!(
                        "voice client flags received: user_id={:?} flags={:?}",
                        voice_user_id(value),
                        voice_data_u64(value, "flags")
                    ),
                );
            }
            VOICE_OP_CLIENT_PLATFORM => {
                logging::debug(
                    "voice",
                    format!(
                        "voice client platform received: user_id={:?} platform={:?}",
                        voice_user_id(value),
                        voice_data_string(value, "platform")
                    ),
                );
            }
            VOICE_OP_DAVE_PREPARE_TRANSITION => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE transition missing data".to_owned())?;
                let transition_id = json_u16(data, "transition_id")?;
                let protocol_version = json_u16(data, "protocol_version")
                    .or_else(|_| json_u16(data, "dave_protocol_version"))?;
                self.pending_transitions
                    .insert(transition_id, protocol_version);
                logging::debug(
                    "voice",
                    format!(
                        "DAVE prepare transition received: transition_id={} protocol_version={}",
                        transition_id, protocol_version
                    ),
                );
                if protocol_version == 0 {
                    if let Some(session) = self.session.as_mut() {
                        session.set_passthrough_mode(true, Some(120));
                    }
                }
                if transition_id == 0 {
                    self.execute_transition(transition_id)?;
                } else {
                    send_dave_transition_ready(writer, transition_id).await?;
                }
            }
            VOICE_OP_DAVE_EXECUTE_TRANSITION => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE execute transition missing data".to_owned())?;
                let transition_id = json_u16(data, "transition_id")?;
                self.execute_transition(transition_id)?;
            }
            VOICE_OP_DAVE_PREPARE_EPOCH => {
                let data = value
                    .get("d")
                    .ok_or_else(|| "DAVE prepare epoch missing data".to_owned())?;
                let epoch = json_u64(data, "epoch")?;
                logging::debug(
                    "voice",
                    format!("DAVE prepare epoch received: epoch={epoch}"),
                );
                if epoch == 1 {
                    let protocol_version = json_u16(data, "protocol_version")
                        .or_else(|_| json_u16(data, "dave_protocol_version"))?;
                    self.reinit(protocol_version)?;
                    self.send_key_package(writer).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn handle_speaking_op(&mut self, value: &Value) -> VoiceSpeakingState {
        let speaking = parse_voice_speaking(value);
        self.record_speaking_state(speaking);
        logging::debug(
            "voice",
            format!(
                "voice speaking received: user_id={:?} ssrc={:?} speaking={:?} known_ssrcs={}",
                speaking.user_id,
                speaking.ssrc,
                speaking.speaking,
                self.ssrc_user_ids.len()
            ),
        );
        speaking
    }

    pub(super) async fn handle_binary_frame(
        &mut self,
        writer: &VoiceWriter,
        frame: VoiceBinaryFrame<'_>,
    ) -> Result<(), String> {
        match frame.opcode {
            VOICE_OP_DAVE_MLS_EXTERNAL_SENDER => {
                let session = self.session_mut()?;
                session
                    .set_external_sender(frame.payload)
                    .map_err(|error| format!("DAVE external sender failed: {error}"))?;
                logging::debug("voice", "DAVE external sender processed");
                self.send_key_package(writer).await?;
            }
            VOICE_OP_DAVE_MLS_PROPOSALS => {
                let Some((&operation, proposals)) = frame.payload.split_first() else {
                    return Err("DAVE proposals payload is empty".to_owned());
                };
                let operation_type = match operation {
                    0 => ProposalsOperationType::APPEND,
                    1 => ProposalsOperationType::REVOKE,
                    other => {
                        return Err(format!("DAVE proposals operation is unsupported: {other}"));
                    }
                };
                let known_user_ids = self.known_user_ids.iter().copied().collect::<Vec<_>>();
                let result = self
                    .session_mut()?
                    .process_proposals(operation_type, proposals, Some(&known_user_ids))
                    .map_err(|error| format!("DAVE proposals processing failed: {error}"))?;
                if let Some(commit_welcome) = result {
                    send_dave_commit_welcome(writer, commit_welcome).await?;
                }
                logging::debug("voice", "DAVE proposals processed");
            }
            VOICE_OP_DAVE_MLS_ANNOUNCE_COMMIT_TRANSITION => {
                let Some((transition_id, commit)) = split_transition_payload(frame.payload) else {
                    return Err("DAVE commit transition payload is too short".to_owned());
                };
                match self.session_mut()?.process_commit(commit) {
                    Ok(()) => {
                        logging::debug(
                            "voice",
                            format!("DAVE commit processed: transition_id={transition_id}"),
                        );
                        if transition_id != 0 {
                            self.pending_transitions.insert(
                                transition_id,
                                self.protocol_version
                                    .map(NonZeroU16::get)
                                    .unwrap_or_default(),
                            );
                            send_dave_transition_ready(writer, transition_id).await?;
                        }
                    }
                    Err(error) => {
                        logging::error("voice", format!("DAVE commit failed: {error}"));
                        send_dave_invalid_commit_welcome(writer, transition_id).await?;
                        self.reinit_current()?;
                        self.send_key_package(writer).await?;
                    }
                }
            }
            VOICE_OP_DAVE_MLS_WELCOME => {
                let Some((transition_id, welcome)) = split_transition_payload(frame.payload) else {
                    return Err("DAVE welcome payload is too short".to_owned());
                };
                match self.session_mut()?.process_welcome(welcome) {
                    Ok(()) => {
                        logging::debug(
                            "voice",
                            format!("DAVE welcome processed: transition_id={transition_id}"),
                        );
                        if transition_id != 0 {
                            self.pending_transitions.insert(
                                transition_id,
                                self.protocol_version
                                    .map(NonZeroU16::get)
                                    .unwrap_or_default(),
                            );
                            send_dave_transition_ready(writer, transition_id).await?;
                        }
                    }
                    Err(error) => {
                        logging::error("voice", format!("DAVE welcome failed: {error}"));
                        send_dave_invalid_commit_welcome(writer, transition_id).await?;
                        self.reinit_current()?;
                        self.send_key_package(writer).await?;
                    }
                }
            }
            other => logging::debug("voice", format!("unhandled voice binary op={other}")),
        }
        Ok(())
    }

    pub(super) fn reinit(&mut self, protocol_version: u16) -> Result<(), String> {
        let Some(protocol_version) = NonZeroU16::new(protocol_version) else {
            self.protocol_version = None;
            if let Some(session) = self.session.as_mut() {
                session
                    .reset()
                    .map_err(|error| format!("DAVE reset failed: {error}"))?;
                session.set_passthrough_mode(true, Some(10));
            }
            logging::debug("voice", "DAVE disabled by protocol transition");
            return Ok(());
        };
        if let Some(session) = self.session.as_mut() {
            session
                .reinit(protocol_version, self.user_id, self.channel_id, None)
                .map_err(|error| format!("DAVE session reinit failed: {error}"))?;
        } else {
            self.session = Some(
                DaveSession::new(protocol_version, self.user_id, self.channel_id, None)
                    .map_err(|error| format!("DAVE session init failed: {error}"))?,
            );
        }
        self.protocol_version = Some(protocol_version);
        logging::debug(
            "voice",
            format!("DAVE session initialized: protocol_version={protocol_version}"),
        );
        Ok(())
    }

    fn reinit_current(&mut self) -> Result<(), String> {
        let protocol_version = self
            .protocol_version
            .map(NonZeroU16::get)
            .ok_or_else(|| "DAVE protocol version is not active".to_owned())?;
        self.reinit(protocol_version)
    }

    fn execute_transition(&mut self, transition_id: u16) -> Result<(), String> {
        let Some(protocol_version) = self.pending_transitions.remove(&transition_id) else {
            logging::debug(
                "voice",
                format!("DAVE execute transition ignored: transition_id={transition_id}"),
            );
            return Ok(());
        };
        if protocol_version == 0 {
            if let Some(session) = self.session.as_mut() {
                session.set_passthrough_mode(true, Some(10));
            }
            self.protocol_version = None;
        } else {
            self.protocol_version = NonZeroU16::new(protocol_version);
            if let Some(session) = self.session.as_mut() {
                session.set_passthrough_mode(true, Some(10));
            }
        }
        logging::debug(
            "voice",
            format!(
                "DAVE transition executed: transition_id={} protocol_version={}",
                transition_id, protocol_version
            ),
        );
        Ok(())
    }

    async fn send_key_package(&mut self, writer: &VoiceWriter) -> Result<(), String> {
        let key_package = self
            .session_mut()?
            .create_key_package()
            .map_err(|error| format!("DAVE key package creation failed: {error}"))?;
        send_voice_binary(writer, VOICE_OP_DAVE_MLS_KEY_PACKAGE, key_package).await?;
        logging::debug("voice", "DAVE key package sent");
        Ok(())
    }

    fn session_mut(&mut self) -> Result<&mut DaveSession, String> {
        self.session
            .as_mut()
            .ok_or_else(|| "DAVE session is not initialized".to_owned())
    }

    pub(super) fn unwrap_media_payload_for_ssrc(
        &mut self,
        ssrc: u32,
        payload: &[u8],
    ) -> VoiceMediaPayload {
        if !self.dave_media_active() {
            return VoiceMediaPayload::Plain(payload.to_vec());
        }
        if !looks_like_dave_media_frame(payload) {
            return VoiceMediaPayload::DaveUnexpectedPlain {
                payload_len: payload.len(),
            };
        }
        let Some(user_id) = self.ssrc_user_ids.get(&ssrc).copied() else {
            return VoiceMediaPayload::DaveMissingUser {
                payload_len: payload.len(),
            };
        };
        let Some(session) = self.session.as_mut() else {
            return VoiceMediaPayload::DaveNotReady {
                user_id,
                payload_len: payload.len(),
            };
        };
        if !session.is_ready() {
            return VoiceMediaPayload::DaveNotReady {
                user_id,
                payload_len: payload.len(),
            };
        }
        match session.decrypt(user_id, MediaType::AUDIO, payload) {
            Ok(opus) => VoiceMediaPayload::DaveDecrypted { user_id, opus },
            Err(error) => VoiceMediaPayload::DaveDecryptFailed {
                user_id,
                message: error.to_string(),
            },
        }
    }

    pub(super) fn user_id_for_ssrc(&self, ssrc: u32) -> Option<Id<UserMarker>> {
        self.ssrc_user_ids
            .get(&ssrc)
            .copied()
            .and_then(Id::<UserMarker>::new_checked)
    }

    #[allow(dead_code)]
    pub(super) fn prepare_outbound_opus(&mut self, opus: &[u8]) -> VoiceDaveOutboundPayload {
        if self.protocol_version.is_none() {
            return VoiceDaveOutboundPayload::Plain(opus.to_vec());
        }
        let Some(session) = self.session.as_mut() else {
            return VoiceDaveOutboundPayload::Blocked(
                VoiceOutboundSendBlockReason::DaveOutboundMissingSession,
            );
        };
        if !session.is_ready() {
            return VoiceDaveOutboundPayload::Blocked(
                VoiceOutboundSendBlockReason::DaveOutboundNotReady,
            );
        }
        match session.encrypt_opus(opus) {
            Ok(encrypted) => VoiceDaveOutboundPayload::Encrypted(encrypted.into_owned()),
            Err(_) => VoiceDaveOutboundPayload::Blocked(
                VoiceOutboundSendBlockReason::DaveOutboundEncryptFailed,
            ),
        }
    }

    pub(super) fn dave_media_active(&self) -> bool {
        self.protocol_version.is_some() && self.session.is_some()
    }

    pub(super) fn record_speaking_state(&mut self, speaking: VoiceSpeakingState) {
        if let (Some(ssrc), Some(user_id)) = (speaking.ssrc, speaking.user_id) {
            self.ssrc_user_ids.insert(ssrc, user_id);
            self.known_user_ids.insert(user_id);
        }
    }
}

async fn send_dave_transition_ready(
    writer: &VoiceWriter,
    transition_id: u16,
) -> Result<(), String> {
    send_voice_text(
        writer,
        json!({
            "op": VOICE_OP_DAVE_TRANSITION_READY,
            "d": {
                "transition_id": transition_id,
            },
        })
        .to_string(),
    )
    .await?;
    logging::debug(
        "voice",
        format!("DAVE transition ready sent: transition_id={transition_id}"),
    );
    Ok(())
}

async fn send_dave_commit_welcome(
    writer: &VoiceWriter,
    commit_welcome: davey::CommitWelcome,
) -> Result<(), String> {
    let mut payload = commit_welcome.commit;
    if let Some(mut welcome) = commit_welcome.welcome {
        payload.append(&mut welcome);
    }
    send_voice_binary(writer, VOICE_OP_DAVE_MLS_COMMIT_WELCOME, payload).await?;
    logging::debug("voice", "DAVE commit welcome sent");
    Ok(())
}

async fn send_dave_invalid_commit_welcome(
    writer: &VoiceWriter,
    transition_id: u16,
) -> Result<(), String> {
    send_voice_text(
        writer,
        json!({
            "op": VOICE_OP_DAVE_MLS_INVALID_COMMIT_WELCOME,
            "d": {
                "transition_id": transition_id,
            },
        })
        .to_string(),
    )
    .await?;
    logging::debug(
        "voice",
        format!("DAVE invalid commit welcome sent: transition_id={transition_id}"),
    );
    Ok(())
}

fn split_transition_payload(payload: &[u8]) -> Option<(u16, &[u8])> {
    if payload.len() < 2 {
        return None;
    }
    Some((u16::from_be_bytes([payload[0], payload[1]]), &payload[2..]))
}

fn json_u64(value: &Value, key: &str) -> Result<u64, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing numeric field: {key}"))
}

fn json_u16(value: &Value, key: &str) -> Result<u16, String> {
    json_u64(value, key).and_then(|value| {
        u16::try_from(value).map_err(|_| format!("numeric field does not fit u16: {key}"))
    })
}

fn voice_user_ids(value: &Value) -> Vec<u64> {
    voice_data(value)
        .and_then(|data| data.get("user_ids"))
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(voice_user_id_value).collect())
        .unwrap_or_default()
}

fn voice_user_id(value: &Value) -> Option<u64> {
    voice_data(value)
        .and_then(|data| data.get("user_id"))
        .and_then(voice_user_id_value)
}

fn parse_voice_speaking(value: &Value) -> VoiceSpeakingState {
    VoiceSpeakingState {
        user_id: voice_user_id(value),
        ssrc: voice_data_u32(value, "ssrc"),
        speaking: voice_data_u64(value, "speaking"),
    }
}

pub(super) fn voice_speaking_microphone_active(speaking: u64) -> bool {
    speaking & 1 != 0
}

fn voice_data(value: &Value) -> Option<&Value> {
    value.get("d")
}

fn voice_data_u64(value: &Value, key: &str) -> Option<u64> {
    voice_data(value)
        .and_then(|data| data.get(key))
        .and_then(Value::as_u64)
}

fn voice_data_u32(value: &Value, key: &str) -> Option<u32> {
    voice_data_u64(value, key).and_then(|value| u32::try_from(value).ok())
}

fn voice_data_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    voice_data(value)
        .and_then(|data| data.get(key))
        .and_then(Value::as_str)
}

fn voice_data_field_count(value: &Value) -> usize {
    voice_data(value)
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len)
}

fn voice_user_id_value(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

pub(super) fn looks_like_dave_media_frame(payload: &[u8]) -> bool {
    payload.len() >= DAVE_MIN_SUPPLEMENTAL_BYTES
        && payload[payload.len() - DAVE_MAGIC_MARKER.len()..] == DAVE_MAGIC_MARKER
}
