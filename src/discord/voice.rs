use std::{fmt, sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::sleep,
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::discord::{
    CurrentVoiceConnectionState, VoiceServerInfo, VoiceStateInfo,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, UserMarker},
    },
};
use crate::logging;

use super::events::AppEvent;

const VOICE_GATEWAY_VERSION: u8 = 9;

type VoiceGatewayStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type VoiceWriter = Arc<Mutex<futures::stream::SplitSink<VoiceGatewayStream, WsMessage>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum VoiceRuntimeEvent {
    Requested(Option<CurrentVoiceConnectionState>),
    CurrentUserReady(Option<Id<UserMarker>>),
    VoiceState(VoiceStateInfo),
    VoiceServer(VoiceServerInfo),
    Shutdown,
}

#[derive(Clone, Eq, PartialEq)]
struct VoiceGatewaySession {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    user_id: Id<UserMarker>,
    session_id: String,
    endpoint: String,
    token: String,
}

impl fmt::Debug for VoiceGatewaySession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceGatewaySession")
            .field("guild_id", &self.guild_id)
            .field("channel_id", &self.channel_id)
            .field("user_id", &self.user_id)
            .field("session_id", &"<redacted>")
            .field("endpoint", &self.endpoint)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Eq, PartialEq)]
enum VoiceRuntimeAction {
    Connect(VoiceGatewaySession),
    Close,
}

#[derive(Default)]
struct VoiceRuntimeState {
    current_user_id: Option<Id<UserMarker>>,
    requested: Option<CurrentVoiceConnectionState>,
    current_voice: Option<ObservedSelfVoiceState>,
    server: Option<VoiceServerInfo>,
    active: Option<VoiceGatewaySession>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedSelfVoiceState {
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    session_id: String,
}

impl VoiceRuntimeState {
    fn apply(&mut self, event: VoiceRuntimeEvent) -> Option<VoiceRuntimeAction> {
        match event {
            VoiceRuntimeEvent::Requested(requested) => {
                self.requested = requested;
                if self.requested.is_none() {
                    self.current_voice = None;
                    self.server = None;
                    return self.close_active();
                }
            }
            VoiceRuntimeEvent::CurrentUserReady(user_id) => {
                self.current_user_id = user_id;
            }
            VoiceRuntimeEvent::VoiceState(state) => {
                if let Some(action) = self.record_voice_state(state) {
                    return Some(action);
                }
            }
            VoiceRuntimeEvent::VoiceServer(server) => {
                if server.endpoint.is_none() {
                    self.server = None;
                    return self.close_active();
                }
                self.server = Some(server);
            }
            VoiceRuntimeEvent::Shutdown => return self.close_active(),
        }

        self.connect_if_ready()
    }

    fn record_voice_state(&mut self, state: VoiceStateInfo) -> Option<VoiceRuntimeAction> {
        if self.current_user_id != Some(state.user_id) {
            return None;
        }
        let requested = self.requested?;
        if state.guild_id != requested.guild_id {
            return None;
        }
        let Some(channel_id) = state.channel_id else {
            self.current_voice = None;
            self.server = None;
            return self.close_active();
        };
        let session_id = state
            .session_id
            .filter(|session_id| !session_id.is_empty())?;
        self.current_voice = Some(ObservedSelfVoiceState {
            guild_id: state.guild_id,
            channel_id,
            session_id,
        });
        None
    }

    fn connect_if_ready(&mut self) -> Option<VoiceRuntimeAction> {
        let requested = self.requested?;
        let voice = self.current_voice.as_ref()?;
        if requested.guild_id != voice.guild_id || requested.channel_id != voice.channel_id {
            return self.close_active();
        }
        let server = self.server.as_ref()?;
        if server.guild_id != requested.guild_id {
            return None;
        }
        let endpoint = server.endpoint.as_ref()?.trim_end_matches('/').to_owned();
        if endpoint.is_empty() || server.token.is_empty() {
            return None;
        }
        let session = VoiceGatewaySession {
            guild_id: requested.guild_id,
            channel_id: requested.channel_id,
            user_id: self.current_user_id?,
            session_id: voice.session_id.clone(),
            endpoint,
            token: server.token.clone(),
        };
        if self.active.as_ref() == Some(&session) {
            return None;
        }
        self.active = Some(session.clone());
        Some(VoiceRuntimeAction::Connect(session))
    }

    fn close_active(&mut self) -> Option<VoiceRuntimeAction> {
        self.active.take().map(|_| VoiceRuntimeAction::Close)
    }
}

pub(crate) fn forward_app_event(
    sender: &mpsc::UnboundedSender<VoiceRuntimeEvent>,
    event: &AppEvent,
) {
    let runtime_event = match event {
        AppEvent::Ready { user_id, .. } => VoiceRuntimeEvent::CurrentUserReady(*user_id),
        AppEvent::VoiceStateUpdate { state } => VoiceRuntimeEvent::VoiceState(state.clone()),
        AppEvent::VoiceServerUpdate { server } => VoiceRuntimeEvent::VoiceServer(server.clone()),
        _ => return,
    };
    let _ = sender.send(runtime_event);
}

pub(crate) async fn run_voice_runtime(mut events: mpsc::UnboundedReceiver<VoiceRuntimeEvent>) {
    let mut state = VoiceRuntimeState::default();
    let mut connection_task: Option<JoinHandle<()>> = None;

    while let Some(event) = events.recv().await {
        let shutdown = matches!(event, VoiceRuntimeEvent::Shutdown);
        if let Some(action) = state.apply(event) {
            match action {
                VoiceRuntimeAction::Connect(session) => {
                    if let Some(task) = connection_task.take() {
                        task.abort();
                    }
                    connection_task = Some(tokio::spawn(run_voice_gateway_session(session)));
                }
                VoiceRuntimeAction::Close => {
                    if let Some(task) = connection_task.take() {
                        task.abort();
                    }
                }
            }
        }
        if shutdown {
            break;
        }
    }

    if let Some(task) = connection_task {
        task.abort();
    }
}

async fn run_voice_gateway_session(session: VoiceGatewaySession) {
    if let Err(error) = connect_voice_gateway(session).await {
        logging::error("voice", error);
    }
}

async fn connect_voice_gateway(session: VoiceGatewaySession) -> Result<(), String> {
    let url = voice_gateway_url(&session.endpoint)?;
    logging::debug("voice", format!("connecting voice websocket: {url}"));
    let (ws, _) = connect_async(&url)
        .await
        .map_err(|error| format!("voice websocket connect failed: {error}"))?;
    let (writer, mut reader) = ws.split();
    let writer = Arc::new(Mutex::new(writer));
    let mut heartbeat_task: Option<JoinHandle<()>> = None;
    let last_sequence = Arc::new(Mutex::new(None));
    let mut identified = false;

    while let Some(frame) = reader.next().await {
        let frame = frame.map_err(|error| format!("voice websocket read failed: {error}"))?;
        match frame {
            WsMessage::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|error| format!("voice websocket JSON parse failed: {error}"))?;
                if let Some(sequence) = value.get("seq").and_then(Value::as_i64) {
                    *last_sequence.lock().await = Some(sequence);
                }
                match value.get("op").and_then(Value::as_u64).unwrap_or_default() {
                    2 => logging::debug("voice", "voice gateway ready"),
                    6 => {}
                    8 => {
                        if let Some(task) = heartbeat_task.take() {
                            task.abort();
                        }
                        let interval = value
                            .get("d")
                            .and_then(|data| data.get("heartbeat_interval"))
                            .and_then(Value::as_u64)
                            .map(Duration::from_millis)
                            .ok_or_else(|| "voice hello missing heartbeat interval".to_owned())?;
                        heartbeat_task = Some(tokio::spawn(run_voice_heartbeat(
                            Arc::clone(&writer),
                            interval,
                            Arc::clone(&last_sequence),
                        )));
                        if !identified {
                            send_voice_text(&writer, voice_identify_payload(&session)).await?;
                            identified = true;
                        }
                    }
                    other => logging::debug("voice", format!("unhandled voice gateway op={other}")),
                }
            }
            WsMessage::Ping(payload) => {
                let mut writer = writer.lock().await;
                writer
                    .send(WsMessage::Pong(payload))
                    .await
                    .map_err(|error| format!("voice websocket pong failed: {error}"))?;
            }
            WsMessage::Close(_) => break,
            WsMessage::Binary(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => {}
        }
    }

    if let Some(task) = heartbeat_task {
        task.abort();
    }
    Ok(())
}

async fn run_voice_heartbeat(
    writer: VoiceWriter,
    interval: Duration,
    last_sequence: Arc<Mutex<Option<i64>>>,
) {
    loop {
        let sequence = last_sequence.lock().await.unwrap_or(-1);
        if let Err(error) = send_voice_text(&writer, voice_heartbeat_payload(sequence)).await {
            logging::error("voice", format!("voice heartbeat send failed: {error}"));
            break;
        }
        sleep(interval).await;
    }
}

async fn send_voice_text(writer: &VoiceWriter, payload: String) -> Result<(), String> {
    let mut writer = writer.lock().await;
    writer
        .send(WsMessage::Text(payload.into()))
        .await
        .map_err(|error| format!("voice websocket send failed: {error}"))
}

fn voice_gateway_url(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint
        .trim()
        .trim_start_matches("wss://")
        .trim_start_matches("https://")
        .trim_start_matches("ws://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if endpoint.is_empty() {
        return Err("voice endpoint is empty".to_owned());
    }
    Ok(format!("wss://{endpoint}/?v={VOICE_GATEWAY_VERSION}"))
}

fn voice_identify_payload(session: &VoiceGatewaySession) -> String {
    json!({
        "op": 0,
        "d": {
            "server_id": session.guild_id.to_string(),
            "user_id": session.user_id.to_string(),
            "channel_id": session.channel_id.to_string(),
            "session_id": session.session_id,
            "token": session.token,
            "max_dave_protocol_version": 0,
        },
    })
    .to_string()
}

fn voice_heartbeat_payload(sequence: i64) -> String {
    json!({
        "op": 3,
        "d": {
            "t": chrono::Utc::now().timestamp_millis(),
            "seq_ack": sequence,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested_voice() -> CurrentVoiceConnectionState {
        CurrentVoiceConnectionState {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            self_mute: true,
            self_deaf: false,
        }
    }

    fn voice_state(user_id: u64, channel_id: Option<Id<ChannelMarker>>) -> VoiceStateInfo {
        VoiceStateInfo {
            guild_id: Id::new(1),
            channel_id,
            user_id: Id::new(user_id),
            session_id: Some("voice-session".to_owned()),
            member: None,
            deaf: false,
            mute: false,
            self_deaf: false,
            self_mute: false,
            self_stream: false,
        }
    }

    fn voice_server() -> VoiceServerInfo {
        VoiceServerInfo {
            guild_id: Id::new(1),
            endpoint: Some("voice.example.com".to_owned()),
            token: "secret-token".to_owned(),
        }
    }

    #[test]
    fn voice_runtime_assembles_local_voice_session() {
        let mut state = VoiceRuntimeState::default();

        assert_eq!(
            state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10)))),
            None
        );
        assert_eq!(
            state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice()))),
            None
        );
        assert_eq!(
            state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
                10,
                Some(Id::new(10))
            ))),
            None
        );
        let action = state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        match action {
            Some(VoiceRuntimeAction::Connect(session)) => {
                assert_eq!(session.guild_id, Id::new(1));
                assert_eq!(session.channel_id, Id::new(10));
                assert_eq!(session.user_id, Id::new(10));
                assert_eq!(session.endpoint, "voice.example.com");
            }
            other => panic!("expected connect action, got {other:?}"),
        }
    }

    #[test]
    fn voice_runtime_ignores_other_user_voice_state() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        assert_eq!(
            state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
                99,
                Some(Id::new(10))
            ))),
            None
        );
    }

    #[test]
    fn voice_runtime_closes_on_leave() {
        let mut state = VoiceRuntimeState::default();
        state.apply(VoiceRuntimeEvent::CurrentUserReady(Some(Id::new(10))));
        state.apply(VoiceRuntimeEvent::Requested(Some(requested_voice())));
        state.apply(VoiceRuntimeEvent::VoiceState(voice_state(
            10,
            Some(Id::new(10)),
        )));
        state.apply(VoiceRuntimeEvent::VoiceServer(voice_server()));

        assert_eq!(
            state.apply(VoiceRuntimeEvent::Requested(None)),
            Some(VoiceRuntimeAction::Close)
        );
    }

    #[test]
    fn voice_gateway_session_debug_redacts_secrets() {
        let session = VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "secret-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "secret-token".to_owned(),
        };

        let debug = format!("{session:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-session"));
        assert!(!debug.contains("secret-token"));
    }

    #[test]
    fn voice_state_debug_redacts_session_id() {
        let state = voice_state(10, Some(Id::new(10)));

        let debug = format!("{state:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("voice-session"));
    }

    #[test]
    fn voice_identify_payload_matches_expected_shape() {
        let session = VoiceGatewaySession {
            guild_id: Id::new(1),
            channel_id: Id::new(10),
            user_id: Id::new(20),
            session_id: "voice-session".to_owned(),
            endpoint: "voice.example.com".to_owned(),
            token: "voice-token".to_owned(),
        };
        let payload: Value = serde_json::from_str(&voice_identify_payload(&session))
            .expect("voice identify payload is valid JSON");

        assert_eq!(payload["op"].as_u64(), Some(0));
        assert_eq!(payload["d"]["server_id"].as_str(), Some("1"));
        assert_eq!(payload["d"]["user_id"].as_str(), Some("20"));
        assert_eq!(payload["d"]["channel_id"].as_str(), Some("10"));
        assert_eq!(payload["d"]["session_id"].as_str(), Some("voice-session"));
        assert_eq!(payload["d"]["token"].as_str(), Some("voice-token"));
        assert_eq!(payload["d"]["max_dave_protocol_version"].as_u64(), Some(0));
    }

    #[test]
    fn voice_gateway_url_normalizes_endpoint() {
        assert_eq!(
            voice_gateway_url("voice.example.com:2048/").as_deref(),
            Ok("wss://voice.example.com:2048/?v=9")
        );
        assert_eq!(
            voice_gateway_url("wss://voice.example.com").as_deref(),
            Ok("wss://voice.example.com/?v=9")
        );
        assert_eq!(
            voice_gateway_url("https://voice.example.com").as_deref(),
            Ok("wss://voice.example.com/?v=9")
        );
        assert_eq!(
            voice_gateway_url("   /").expect_err("empty endpoint should be rejected"),
            "voice endpoint is empty"
        );
    }
}
