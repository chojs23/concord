//! Local Rich Presence (RPC/IPC) server. Detected apps are relayed to the
//! user's profile automatically — the most recently updated activity wins,
//! like the native client. The profile settings picker can pin a specific
//! app or opt out entirely with a manual activity.

mod codec;
mod protocol;
mod registry;
mod socket;

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::tokio::Stream;
use interprocess::local_socket::traits::tokio::Listener as _;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;

use crate::discord::events::{AppEvent, PresenceEventFields};
use crate::discord::{ActivityInfo, ActivityKind, DiscordClient, RichPresenceSelection};
use crate::logging;

use codec::{Opcode, read_frame, write_frame};
use protocol::Command;
use registry::ActivityRegistry;

type SharedRegistry = Arc<Mutex<ActivityRegistry>>;

type NameCache = Arc<Mutex<HashMap<String, String>>>;

type AssetCache = Arc<Mutex<HashMap<String, HashMap<String, String>>>>;

/// Discord rate-limits presence updates (~5 per 20s) while apps like presence.nvim
/// push one per keystroke. Coalesce to at most one broadcast per interval.
const MIN_PRESENCE_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Clone)]
struct RpcContext {
    client: DiscordClient,
    registry: SharedRegistry,
    names: NameCache,
    assets: AssetCache,
}

pub(crate) async fn run_rich_presence(client: DiscordClient, serve_rpc: bool) {
    let context = RpcContext {
        client,
        registry: Arc::new(Mutex::new(ActivityRegistry::default())),
        names: Arc::new(Mutex::new(HashMap::new())),
        assets: Arc::new(Mutex::new(HashMap::new())),
    };
    // IPC availability only controls detection. Switching away from a manual
    // activity must still work when sharing is disabled or binding fails.
    tokio::join!(presence_debounce_loop(context.clone()), async {
        if serve_rpc {
            run_rpc_server(context).await;
        }
    });
}

async fn run_rpc_server(context: RpcContext) {
    let client = &context.client;
    let bound = match socket::bind_first_available() {
        Ok(bound) => bound,
        Err(error) => {
            let message = format!(
                "Rich Presence is disabled: no discord-ipc socket could be bound ({error})"
            );
            logging::error("rpc", &message);
            client
                .publish_event(AppEvent::RichPresenceWarning { message })
                .await;
            return;
        }
    };
    // RPC apps probe discord-ipc-0 first and connect to the first socket that
    // answers, so a nonzero slot usually means another local client receives
    // the activity instead of concord. Still serve the slot for apps that
    // reach it, but make the steal visible.
    if bound.slot > 0 {
        let message = format!(
            "Another client owns discord-ipc-0; Rich Presence apps may relay through it instead of concord (concord listens on discord-ipc-{})",
            bound.slot
        );
        logging::error("rpc", &message);
        client
            .publish_event(AppEvent::RichPresenceWarning { message })
            .await;
    }
    logging::debug(
        "rpc",
        format!(
            "rich presence server listening on discord-ipc-{}",
            bound.slot
        ),
    );
    let _socket_cleanup = socket::SocketCleanup::new(bound.path.clone());

    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = bound.listener.accept() => match accepted {
                Ok(stream) => {
                    connections.spawn(handle_connection(stream, context.clone()));
                }
                Err(error) => {
                    logging::error("rpc", format!("accept failed: {error}"));
                    break;
                }
            },
            Some(_) = connections.join_next() => {}
        }
    }
    connections.shutdown().await;
    {
        let mut registry = context.registry.lock().await;
        *registry = ActivityRegistry::default();
        if let RichPresenceSelection::App(client_id) = client.rich_presence_selection() {
            client.clear_rich_presence_pin(&client_id);
        }
    }
    publish_detected(&context).await;
    client.notify_rich_presence_dirty();
}

async fn handle_connection(mut stream: Stream, context: RpcContext) {
    if let Err(error) = serve_connection(&mut stream, &context).await {
        logging::debug("rpc", format!("connection closed: {error}"));
    }
}

async fn serve_connection<S>(stream: &mut S, context: &RpcContext) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let handshake = read_frame(stream).await?;
    if handshake.opcode != Opcode::Handshake {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected HANDSHAKE as the first RPC frame",
        ));
    }
    let Some(client_id) = protocol::parse_handshake_client_id(&handshake.payload) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HANDSHAKE client_id must be a numeric snowflake",
        ));
    };
    logging::debug("rpc", format!("handshake from client_id={client_id}"));
    let user = context.client.current_user_rpc_identity();
    write_frame(stream, Opcode::Frame, &protocol::build_ready_payload(user)).await?;

    // Keep the entry revision owned by this connection. An old connection's
    // cleanup must not remove an activity already replaced by a reconnect.
    let mut active_pids: HashMap<i64, u64> = HashMap::new();
    let outcome = loop {
        let frame = match read_frame(stream).await {
            Ok(frame) => frame,
            Err(error) => break Err(error),
        };
        match frame.opcode {
            Opcode::Ping => {
                if let Err(error) = write_frame(stream, Opcode::Pong, &frame.payload).await {
                    break Err(error);
                }
            }
            Opcode::Close => break Ok(()),
            Opcode::Frame => {
                if let Err(error) = handle_command(
                    stream,
                    &frame.payload,
                    &client_id,
                    &mut active_pids,
                    context,
                )
                .await
                {
                    break Err(error);
                }
            }
            Opcode::Handshake | Opcode::Pong => {}
        }
    };

    if !active_pids.is_empty() {
        let mut registry = context.registry.lock().await;
        for (pid, sequence) in &active_pids {
            registry.clear_if_current(&client_id, *pid, *sequence);
        }
        if registry.activity_for_client(&client_id).is_none() {
            context.client.clear_rich_presence_pin(&client_id);
        }
        drop(registry);
        publish_detected(context).await;
        context.client.notify_rich_presence_dirty();
    }
    outcome
}

async fn handle_command<S>(
    stream: &mut S,
    payload: &[u8],
    client_id: &str,
    active_pids: &mut HashMap<i64, u64>,
    context: &RpcContext,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let command = match protocol::parse_command(payload, client_id) {
        Ok(command) => command,
        Err(error) => {
            return write_frame(
                stream,
                Opcode::Frame,
                &protocol::build_command_error(&error),
            )
            .await;
        }
    };
    match command {
        Command::SetActivity {
            pid,
            activity,
            echo,
            nonce,
        } => {
            // Resolve name and assets before locking the registry, so no lock
            // is held across the REST round-trips.
            match activity {
                Some(mut activity) => {
                    activity.name = resolve_app_name(context, client_id).await;
                    resolve_asset_keys(context, client_id, &mut activity).await;
                    let sequence =
                        context
                            .registry
                            .lock()
                            .await
                            .set(client_id.to_owned(), pid, *activity);
                    active_pids.insert(pid, sequence);
                }
                None => {
                    let mut registry = context.registry.lock().await;
                    if pid == 0 {
                        // A zero-PID clear applies to this connection, not every
                        // process using the same application ID.
                        for (pid, sequence) in active_pids.drain() {
                            registry.clear_if_current(client_id, pid, sequence);
                        }
                    } else {
                        // An explicit PID clear can arrive on a new connection.
                        active_pids.remove(&pid);
                        registry.clear(client_id, pid);
                    }
                    if registry.activity_for_client(client_id).is_none() {
                        context.client.clear_rich_presence_pin(client_id);
                    }
                }
            }
            publish_detected(context).await;
            context.client.notify_rich_presence_dirty();
            write_frame(
                stream,
                Opcode::Frame,
                &protocol::build_command_ack("SET_ACTIVITY", nonce.as_deref(), echo),
            )
            .await
        }
    }
}

async fn publish_detected(context: &RpcContext) {
    let activities = context.registry.lock().await.activities();
    context
        .client
        .publish_event(AppEvent::RichPresenceDetected { activities })
        .await;
}

async fn presence_debounce_loop(context: RpcContext) {
    let dirty = context.client.rich_presence_dirty();
    let mut snapshots = context.client.subscribe_snapshots();
    loop {
        dirty.notified().await;
        // Keep this dirty update pending until READY supplies self presence.
        // Watching snapshots avoids polling and needs no second RPC update.
        while context
            .client
            .read_state()
            .session
            .current_user_session_status
            .is_none()
        {
            if snapshots.changed().await.is_err() {
                return;
            }
        }
        broadcast_selected_now(&context).await;
        tokio::time::sleep(MIN_PRESENCE_INTERVAL).await;
    }
}

/// Relays the current RPC activity to the user's presence, mirroring the
/// native client: the selected app wins (falling back to the most recent
/// one when it is gone), and the user's custom status survives alongside it.
async fn broadcast_selected_now(context: &RpcContext) {
    if context
        .client
        .read_state()
        .session
        .current_user_session_status
        .is_none()
    {
        return;
    }
    let (selection, rpc_activity) = {
        let registry = context.registry.lock().await;
        let mut selection = context.client.rich_presence_selection();
        let activity = match &selection {
            // A manual activity is owned by the user; RPC never overrides it.
            RichPresenceSelection::Manual => return,
            RichPresenceSelection::App(client_id) => {
                if let Some(activity) = registry.activity_for_client(client_id) {
                    Some(activity)
                } else {
                    context.client.clear_rich_presence_pin(client_id);
                    selection = RichPresenceSelection::Automatic;
                    registry.latest_activity()
                }
            }
            RichPresenceSelection::Automatic => registry.latest_activity(),
        };
        (selection, activity)
    };

    // Turn raw external image URLs into `mp:` refs here (only for the app we
    // broadcast), since the gateway needs them registered first.
    let mut rpc_activity = rpc_activity;
    if let Some(activity) = rpc_activity.as_mut() {
        context
            .client
            .resolve_activity_external_assets(activity)
            .await;
    }

    // Asset lookup can yield to a manual selection, re-identify, or a status
    // change. Never publish the old selection or a guessed Online status.
    if context.client.rich_presence_selection() != selection {
        return;
    }
    let (user_id, status, mut activities) = {
        let state = context.client.read_state();
        let Some(status) = state.session.current_user_session_status else {
            context.client.notify_rich_presence_dirty();
            return;
        };
        let Some(user_id) = state.current_user_id() else {
            return;
        };
        let custom_status = state
            .user_activities(user_id)
            .iter()
            .filter(|activity| activity.kind == ActivityKind::Custom)
            .cloned()
            .collect::<Vec<_>>();
        (user_id, status, custom_status)
    };
    activities.extend(rpc_activity);
    if let Err(error) = context
        .client
        .update_presence_activity(status, activities.clone())
    {
        logging::error("rpc", format!("live presence update failed: {error}"));
        return;
    }
    context
        .client
        .publish_event(AppEvent::PresenceUpdate {
            guild_id: None,
            presence: PresenceEventFields {
                user_id,
                status,
                activities,
            },
        })
        .await;
}

async fn resolve_app_name(context: &RpcContext, client_id: &str) -> String {
    if let Some(name) = context.names.lock().await.get(client_id).cloned() {
        return name;
    }
    let resolved = context
        .client
        .application_display_name(client_id)
        .await
        .unwrap_or_else(|| client_id.to_owned());
    context
        .names
        .lock()
        .await
        .insert(client_id.to_owned(), resolved.clone());
    resolved
}

/// Swap app-asset keys ("neovim") for the numeric CDN ids viewers resolve, or
/// the icon renders broken. External URLs and `mp:`/id refs are left for later.
async fn resolve_asset_keys(context: &RpcContext, client_id: &str, activity: &mut ActivityInfo) {
    let Some(assets) = activity.assets.as_mut() else {
        return;
    };
    if !needs_key_lookup(&assets.large_image) && !needs_key_lookup(&assets.small_image) {
        return;
    }
    let map = asset_map(context, client_id).await;
    if map.is_empty() {
        return;
    }
    let resolve = |image: &mut Option<String>| {
        if let Some(key) = image.as_deref()
            && let Some(id) = map.get(key)
        {
            *image = Some(id.clone());
        }
    };
    resolve(&mut assets.large_image);
    resolve(&mut assets.small_image);
}

fn is_external_image_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

fn needs_key_lookup(image: &Option<String>) -> bool {
    image
        .as_deref()
        .is_some_and(|value| !value.starts_with("mp:") && !is_external_image_url(value))
}

/// A fetch failure is not cached (retried next update). A successful empty
/// result is cached, so a genuinely keyless app is not re-fetched.
async fn asset_map(context: &RpcContext, client_id: &str) -> HashMap<String, String> {
    if let Some(map) = context.assets.lock().await.get(client_id).cloned() {
        return map;
    }
    let Some(map) = context.client.application_asset_ids(client_id).await else {
        return HashMap::new();
    };
    context
        .assets
        .lock()
        .await
        .insert(client_id.to_owned(), map.clone());
    map
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::io::AsyncWriteExt;
    use tokio::sync::Mutex;

    use super::codec::{Opcode, encode_frame, read_frame};
    use super::registry::ActivityRegistry;
    use super::{
        RpcContext, broadcast_selected_now, is_external_image_url, needs_key_lookup,
        serve_connection,
    };
    use crate::discord::events::{AppEvent, PresenceEventFields};
    use crate::discord::gateway::GatewayCommand;
    use crate::discord::ids::Id;
    use crate::discord::{
        ActivityInfo, ActivityKind, DiscordClient, PresenceStatus, RichPresenceSelection,
    };

    fn context_with_client(client: DiscordClient) -> RpcContext {
        RpcContext {
            client,
            registry: Arc::new(Mutex::new(ActivityRegistry::default())),
            names: Arc::new(Mutex::new(HashMap::new())),
            assets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn test_context() -> RpcContext {
        context_with_client(
            DiscordClient::new("test-token".to_owned()).expect("valid token header"),
        )
    }

    #[test]
    fn image_reference_shape_drives_asset_resolution() {
        assert!(is_external_image_url("https://example.com/icon.png"));
        assert!(is_external_image_url("http://example.com/icon.png"));
        assert!(!is_external_image_url("neovim"));
        assert!(!is_external_image_url(
            "mp:external/abc/https/example.com/icon.png"
        ));

        assert!(needs_key_lookup(&Some("neovim".to_owned())));
        assert!(!needs_key_lookup(&Some(
            "https://example.com/icon.png".to_owned()
        )));
        assert!(!needs_key_lookup(&Some(
            "mp:external/abc/https/x/icon.png".to_owned()
        )));
        assert!(!needs_key_lookup(&None));
    }

    #[tokio::test]
    async fn serve_connection_completes_handshake_and_answers_ping() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let context = test_context();
        let (mut app, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            let _ = serve_connection(&mut server, &context).await;
        });

        app.write_all(&encode_frame(
            Opcode::Handshake,
            br#"{"v":1,"client_id":"123"}"#,
        ))
        .await
        .expect("send handshake");
        let ready = read_frame(&mut app).await.expect("ready frame");
        assert_eq!(ready.opcode, Opcode::Frame);
        let ready_json: serde_json::Value =
            serde_json::from_slice(&ready.payload).expect("ready is json");
        assert_eq!(ready_json["evt"].as_str(), Some("READY"));

        app.write_all(&encode_frame(Opcode::Ping, b"beat"))
            .await
            .expect("send ping");
        let pong = read_frame(&mut app).await.expect("pong frame");
        assert_eq!(pong.opcode, Opcode::Pong);
        assert_eq!(pong.payload, b"beat");

        app.write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("send close");
        server_task.await.expect("server task joins cleanly");
    }

    #[tokio::test]
    async fn rpc_command_errors_use_the_documented_error_event() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let context = test_context();
        let (mut app, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            let _ = serve_connection(&mut server, &context).await;
        });

        app.write_all(&encode_frame(
            Opcode::Handshake,
            br#"{"v":1,"client_id":"123"}"#,
        ))
        .await
        .expect("send handshake");
        read_frame(&mut app).await.expect("ready frame");

        let cases = [
            (
                br#"{"cmd":"GET_GUILD","nonce":"n1","args":{}}"#.as_slice(),
                Some("GET_GUILD"),
                Some("n1"),
                4002,
            ),
            (
                br#"{"cmd":"SET_ACTIVITY","nonce":"n2","args":{"pid":1,"activity":"invalid"}}"#
                    .as_slice(),
                Some("SET_ACTIVITY"),
                Some("n2"),
                4000,
            ),
            (br#"{"#.as_slice(), None, None, 4000),
        ];

        for (payload, cmd, nonce, code) in cases {
            app.write_all(&encode_frame(Opcode::Frame, payload))
                .await
                .expect("send RPC command");
            let response = read_frame(&mut app).await.expect("RPC error frame");
            let response: serde_json::Value =
                serde_json::from_slice(&response.payload).expect("RPC error should be JSON");

            assert_eq!(response["evt"].as_str(), Some("ERROR"));
            assert_eq!(response["cmd"].as_str(), cmd);
            assert_eq!(response["nonce"].as_str(), nonce);
            assert_eq!(response["data"]["code"].as_u64(), Some(code));
            assert!(response["data"]["message"].as_str().is_some());
        }

        app.write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("send close");
        server_task.await.expect("server task joins cleanly");
    }

    fn presence_update_command(command: GatewayCommand) -> (PresenceStatus, Vec<ActivityInfo>) {
        match command {
            GatewayCommand::UpdatePresence { status, activities } => (status, activities),
            other => panic!("expected UpdatePresence, got {other:?}"),
        }
    }

    async fn test_client_with_current_user()
    -> (DiscordClient, Id<crate::discord::ids::marker::UserMarker>) {
        let client = DiscordClient::new("test-token".to_owned()).expect("valid token header");
        let user_id = Id::new(10);
        client
            .publish_event(AppEvent::Ready {
                user: "neo".to_owned(),
                user_id: Some(user_id),
            })
            .await;
        (client, user_id)
    }

    #[tokio::test]
    async fn automatic_waits_for_initial_self_presence_and_replays_pending_activity() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let context = test_context();
        let client = context.client.clone();
        let mut commands = client.take_gateway_commands_for_test();
        let game = ActivityInfo::playing("Game A");
        context
            .registry
            .lock()
            .await
            .set("app-a".to_owned(), 1, game.clone());
        let task = tokio::spawn(super::presence_debounce_loop(context));
        client.notify_rich_presence_dirty();

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), commands.recv())
                .await
                .is_err()
        );
        let user_id = Id::new(10);
        client
            .publish_event(AppEvent::Ready {
                user: "neo".to_owned(),
                user_id: Some(user_id),
            })
            .await;
        // Guild presence is not the session's own status, and can arrive first.
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: Some(Id::new(20)),
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Online,
                    activities: Vec::new(),
                },
            })
            .await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), commands.recv())
                .await
                .is_err()
        );

        let custom = ActivityInfo::test(ActivityKind::Custom, "deep in thought");
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Offline,
                    activities: vec![custom.clone()],
                },
            })
            .await;
        // No second RPC update is needed to release the pending activity.
        let command = tokio::time::timeout(std::time::Duration::from_secs(1), commands.recv())
            .await
            .expect("readiness wakes the relay")
            .expect("presence update");
        assert_eq!(
            presence_update_command(command),
            (PresenceStatus::Offline, vec![custom, game])
        );
        task.abort();
    }

    #[tokio::test]
    async fn disconnected_pin_stays_automatic_after_the_app_restarts() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (client, user_id) = test_client_with_current_user().await;
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Online,
                    activities: Vec::new(),
                },
            })
            .await;
        let mut commands = client.take_gateway_commands_for_test();
        let context = context_with_client(client.clone());
        client.set_rich_presence_selection(RichPresenceSelection::App("app-a".to_owned()));
        context
            .registry
            .lock()
            .await
            .set("app-b".to_owned(), 2, ActivityInfo::playing("Game B"));
        broadcast_selected_now(&context).await;
        assert_eq!(
            client.rich_presence_selection(),
            RichPresenceSelection::Automatic
        );
        commands.try_recv().expect("fallback update");

        context.registry.lock().await.set(
            "app-a".to_owned(),
            1,
            ActivityInfo::playing("Game A restarted"),
        );
        context.registry.lock().await.set(
            "app-b".to_owned(),
            2,
            ActivityInfo::playing("Game B latest"),
        );
        broadcast_selected_now(&context).await;
        let (_, activities) = presence_update_command(commands.try_recv().expect("latest update"));
        assert_eq!(activities, vec![ActivityInfo::playing("Game B latest")]);
    }

    #[tokio::test]
    async fn automatic_clears_manual_activity_without_an_ipc_listener() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (client, user_id) = test_client_with_current_user().await;
        let custom = ActivityInfo::test(ActivityKind::Custom, "deep in thought");
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::DoNotDisturb,
                    activities: vec![custom.clone(), ActivityInfo::playing("Manual Game")],
                },
            })
            .await;
        let mut commands = client.take_gateway_commands_for_test();
        client.set_rich_presence_selection(RichPresenceSelection::Manual);
        let task = tokio::spawn(super::run_rich_presence(client.clone(), false));
        client.set_rich_presence_selection(RichPresenceSelection::Automatic);
        client.notify_rich_presence_dirty();

        let command = tokio::time::timeout(std::time::Duration::from_secs(1), commands.recv())
            .await
            .expect("automatic works without IPC")
            .expect("presence update");
        assert_eq!(
            presence_update_command(command),
            (PresenceStatus::DoNotDisturb, vec![custom])
        );
        assert!(
            !task.is_finished(),
            "the coordinator stays available without a listener"
        );
        task.abort();
    }

    #[tokio::test]
    async fn clearing_or_disconnecting_the_last_pinned_process_releases_the_pin() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for disconnect in [false, true] {
            let context = test_context();
            context
                .names
                .lock()
                .await
                .insert("123".to_owned(), "Game A".to_owned());
            let (mut app, mut server) = tokio::io::duplex(4096);
            let server_context = context.clone();
            let task =
                tokio::spawn(async move { serve_connection(&mut server, &server_context).await });
            app.write_all(&encode_frame(
                Opcode::Handshake,
                br#"{"v":1,"client_id":"123"}"#,
            ))
            .await
            .expect("send handshake");
            read_frame(&mut app).await.expect("ready frame");
            for pid in [1, 2] {
                let payload = serde_json::json!({"cmd":"SET_ACTIVITY","args":{"pid":pid,"activity":{"type":0}}});
                app.write_all(&encode_frame(Opcode::Frame, payload.to_string().as_bytes()))
                    .await
                    .expect("set activity");
                read_frame(&mut app).await.expect("activity acknowledged");
            }
            context
                .client
                .set_rich_presence_selection(RichPresenceSelection::App("123".to_owned()));
            let clear = |pid| {
                serde_json::json!({"cmd":"SET_ACTIVITY","args":{"pid":pid,"activity":null}})
                    .to_string()
            };
            app.write_all(&encode_frame(Opcode::Frame, clear(1).as_bytes()))
                .await
                .expect("clear first process");
            read_frame(&mut app).await.expect("clear acknowledged");
            assert_eq!(
                context.client.rich_presence_selection(),
                RichPresenceSelection::App("123".to_owned())
            );

            if !disconnect {
                app.write_all(&encode_frame(Opcode::Frame, clear(2).as_bytes()))
                    .await
                    .expect("clear final process");
                read_frame(&mut app).await.expect("clear acknowledged");
                assert_eq!(
                    context.client.rich_presence_selection(),
                    RichPresenceSelection::Automatic
                );
            }
            app.write_all(&encode_frame(Opcode::Close, b""))
                .await
                .expect("close app");
            task.await
                .expect("connection task joins")
                .expect("clean close");
            assert_eq!(
                context.client.rich_presence_selection(),
                RichPresenceSelection::Automatic
            );
        }
    }

    #[tokio::test]
    async fn explicit_clear_from_reconnected_client_removes_the_previous_activity() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (client, user_id) = test_client_with_current_user().await;
        let custom = ActivityInfo::test(ActivityKind::Custom, "deep in thought");
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Online,
                    activities: vec![custom.clone()],
                },
            })
            .await;
        let mut commands = client.take_gateway_commands_for_test();
        let context = context_with_client(client);
        context
            .names
            .lock()
            .await
            .insert("123".to_owned(), "Neovim".to_owned());

        let (mut app, mut server) = tokio::io::duplex(4096);
        let server_context = context.clone();
        let activity_task =
            tokio::spawn(async move { serve_connection(&mut server, &server_context).await });
        app.write_all(&encode_frame(
            Opcode::Handshake,
            br#"{"v":1,"client_id":"123"}"#,
        ))
        .await
        .expect("send activity connection handshake");
        read_frame(&mut app)
            .await
            .expect("activity connection ready");
        let set = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 42, "activity": { "type": 0, "details": "Editing" } }
        });
        app.write_all(&encode_frame(Opcode::Frame, set.to_string().as_bytes()))
            .await
            .expect("set activity");
        read_frame(&mut app).await.expect("set acknowledged");

        // An explicit clear is keyed by application and process, so it must
        // also work when it arrives over a replacement IPC connection.
        let (mut clearing_app, mut clearing_server) = tokio::io::duplex(4096);
        let clearing_context = context.clone();
        let clearing_task =
            tokio::spawn(
                async move { serve_connection(&mut clearing_server, &clearing_context).await },
            );
        clearing_app
            .write_all(&encode_frame(
                Opcode::Handshake,
                br#"{"v":1,"client_id":"123"}"#,
            ))
            .await
            .expect("send clearing connection handshake");
        read_frame(&mut clearing_app)
            .await
            .expect("clearing connection ready");
        let clear = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 42, "activity": null }
        });
        clearing_app
            .write_all(&encode_frame(Opcode::Frame, clear.to_string().as_bytes()))
            .await
            .expect("clear activity");
        read_frame(&mut clearing_app)
            .await
            .expect("clear acknowledged");

        let detected = context.registry.lock().await.activities();
        broadcast_selected_now(&context).await;
        let outbound = presence_update_command(commands.recv().await.expect("presence update"));
        let local_activities = context.client.current_user_activities();

        clearing_app
            .write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("close clearing connection");
        clearing_task
            .await
            .expect("clearing task joins")
            .expect("clearing connection closes cleanly");
        app.write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("close activity connection");
        activity_task
            .await
            .expect("activity task joins")
            .expect("activity connection closes cleanly");

        assert!(
            detected.is_empty(),
            "the cleared app must not stay detected"
        );
        assert_eq!(outbound, (PresenceStatus::Online, vec![custom.clone()]));
        assert_eq!(local_activities, vec![custom]);
    }

    #[tokio::test]
    async fn zero_pid_clear_removes_connection_activity_and_preserves_custom_status() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (client, user_id) = test_client_with_current_user().await;
        let custom = ActivityInfo::test(ActivityKind::Custom, "deep in thought");
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::DoNotDisturb,
                    activities: vec![custom.clone()],
                },
            })
            .await;
        let mut commands = client.take_gateway_commands_for_test();
        let context = context_with_client(client);
        context
            .names
            .lock()
            .await
            .insert("123".to_owned(), "Neovim".to_owned());

        let (mut app, mut server) = tokio::io::duplex(4096);
        let server_context = context.clone();
        let task =
            tokio::spawn(async move { serve_connection(&mut server, &server_context).await });
        app.write_all(&encode_frame(
            Opcode::Handshake,
            br#"{"v":1,"client_id":"123"}"#,
        ))
        .await
        .expect("send handshake");
        read_frame(&mut app).await.expect("connection ready");
        let set = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": 42,
                "activity": { "type": 0, "details": "Editing concord" }
            }
        });
        app.write_all(&encode_frame(Opcode::Frame, set.to_string().as_bytes()))
            .await
            .expect("set activity");
        read_frame(&mut app).await.expect("set acknowledged");

        // A persistent RPC process can clear the activities owned by this
        // connection with pid 0 and no activity field.
        let clear = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 0 }
        });
        app.write_all(&encode_frame(Opcode::Frame, clear.to_string().as_bytes()))
            .await
            .expect("clear activity");
        let clear_response = read_frame(&mut app).await.expect("clear acknowledged");
        let clear_response: serde_json::Value =
            serde_json::from_slice(&clear_response.payload).expect("clear response is JSON");

        let detected = context.registry.lock().await.activities();
        broadcast_selected_now(&context).await;
        let outbound = presence_update_command(commands.recv().await.expect("presence update"));
        let local_activities = context.client.current_user_activities();

        app.write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("close connection");
        task.await
            .expect("connection task joins")
            .expect("connection closes cleanly");

        assert!(
            clear_response["evt"].is_null(),
            "clear must be acknowledged"
        );
        assert!(
            detected.is_empty(),
            "the cleared app must not stay detected"
        );
        assert_eq!(
            outbound,
            (PresenceStatus::DoNotDisturb, vec![custom.clone()])
        );
        assert_eq!(local_activities, vec![custom]);
    }

    #[tokio::test]
    async fn zero_pid_clear_only_removes_entries_owned_by_the_connection() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let context = test_context();
        context
            .names
            .lock()
            .await
            .insert("123".to_owned(), "Neovim".to_owned());

        let (mut app, mut server) = tokio::io::duplex(4096);
        let server_context = context.clone();
        let task =
            tokio::spawn(async move { serve_connection(&mut server, &server_context).await });
        app.write_all(&encode_frame(
            Opcode::Handshake,
            br#"{"v":1,"client_id":"123"}"#,
        ))
        .await
        .expect("send handshake");
        read_frame(&mut app).await.expect("connection ready");
        for pid in [42, 43] {
            let set = serde_json::json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": pid, "activity": { "type": 0, "details": pid.to_string() } }
            });
            app.write_all(&encode_frame(Opcode::Frame, set.to_string().as_bytes()))
                .await
                .expect("set connection activity");
            read_frame(&mut app).await.expect("set acknowledged");
        }

        // Model a newer connection replacing one key, plus an unrelated app.
        // The pid-zero clear must not remove either entry.
        {
            let mut registry = context.registry.lock().await;
            registry.set(
                "123".to_owned(),
                43,
                ActivityInfo::playing("Replacement connection"),
            );
            registry.set("456".to_owned(), 9, ActivityInfo::playing("Unrelated app"));
        }

        let clear = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 0 }
        });
        app.write_all(&encode_frame(Opcode::Frame, clear.to_string().as_bytes()))
            .await
            .expect("clear connection activities");
        read_frame(&mut app).await.expect("clear acknowledged");

        // Inspect before closing so disconnect cleanup cannot mask a bad clear.
        let remaining = context.registry.lock().await.activities();
        app.write_all(&encode_frame(Opcode::Close, b""))
            .await
            .expect("close connection");
        task.await
            .expect("connection task joins")
            .expect("connection closes cleanly");

        let names = remaining
            .iter()
            .map(|activity| activity.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Unrelated app", "Replacement connection"]);
        assert_eq!(context.registry.lock().await.activities(), remaining);
    }

    #[tokio::test]
    async fn automatic_mode_broadcasts_latest_activity_and_keeps_custom_status() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (client, user_id) = test_client_with_current_user().await;
        let mut gateway_commands = client.take_gateway_commands_for_test();

        let custom = ActivityInfo::test(ActivityKind::Custom, "deep in thought");
        client
            .publish_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Online,
                    activities: vec![custom.clone()],
                },
            })
            .await;

        let context = context_with_client(client.clone());
        {
            let mut registry = context.registry.lock().await;
            registry.set(
                "app-a".to_owned(),
                1,
                ActivityInfo::test(ActivityKind::Playing, "Game A"),
            );
            registry.set(
                "app-b".to_owned(),
                2,
                ActivityInfo::test(ActivityKind::Playing, "Song B"),
            );
        }

        // Both automatic and pinned relay keep the custom status alongside it.
        for (selection, name) in [
            (RichPresenceSelection::Automatic, "Song B"),
            (RichPresenceSelection::App("app-a".to_owned()), "Game A"),
        ] {
            client.set_rich_presence_selection(selection);
            broadcast_selected_now(&context).await;
            let (status, activities) =
                presence_update_command(gateway_commands.recv().await.expect("presence update"));
            assert_eq!(status, PresenceStatus::Online);
            assert_eq!(
                activities,
                vec![
                    custom.clone(),
                    ActivityInfo::test(ActivityKind::Playing, name)
                ]
            );
        }

        // A pinned app that is gone falls back to the latest remaining one.
        client.set_rich_presence_selection(RichPresenceSelection::App("gone".to_owned()));
        broadcast_selected_now(&context).await;
        let (_status, activities) =
            presence_update_command(gateway_commands.recv().await.expect("presence update"));
        assert_eq!(
            activities.last().map(|activity| activity.name.as_str()),
            Some("Song B")
        );

        // Manual never broadcasts, so a manual activity cannot be overridden.
        client.set_rich_presence_selection(RichPresenceSelection::Manual);
        broadcast_selected_now(&context).await;
        assert!(
            gateway_commands.try_recv().is_err(),
            "manual mode must not broadcast"
        );

        // An empty registry in automatic mode clears the relayed activity but
        // keeps the custom status.
        client.set_rich_presence_selection(RichPresenceSelection::Automatic);
        context.registry.lock().await.clear("app-a", 1);
        context.registry.lock().await.clear("app-b", 2);
        broadcast_selected_now(&context).await;
        let (_status, activities) =
            presence_update_command(gateway_commands.recv().await.expect("presence update"));
        assert_eq!(
            activities
                .iter()
                .map(|activity| activity.name.as_str())
                .collect::<Vec<_>>(),
            ["deep in thought"]
        );
    }
}
