//! Local Rich Presence (RPC/IPC) server. Detected apps are relayed to the
//! user's profile automatically — the most recently updated activity wins,
//! like the native client. The profile settings picker can pin a specific
//! app or opt out entirely with a manual activity.

mod codec;
mod protocol;
mod registry;
mod socket;

use std::collections::{HashMap, HashSet};
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

pub(crate) async fn run_rpc_server(client: DiscordClient) {
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

    let context = RpcContext {
        client,
        registry: Arc::new(Mutex::new(ActivityRegistry::default())),
        names: Arc::new(Mutex::new(HashMap::new())),
        assets: Arc::new(Mutex::new(HashMap::new())),
    };
    tokio::spawn(presence_debounce_loop(context.clone()));
    loop {
        match bound.listener.accept().await {
            Ok(stream) => {
                tokio::spawn(handle_connection(stream, context.clone()));
            }
            Err(error) => {
                logging::error("rpc", format!("accept failed: {error}"));
                break;
            }
        }
    }
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

    let mut active_pids: HashSet<i64> = HashSet::new();
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
        for pid in &active_pids {
            registry.clear(&client_id, *pid);
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
    active_pids: &mut HashSet<i64>,
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
                    context
                        .registry
                        .lock()
                        .await
                        .set(client_id.to_owned(), pid, *activity);
                    active_pids.insert(pid);
                }
                None => {
                    context.registry.lock().await.clear(client_id, pid);
                    active_pids.remove(&pid);
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
    loop {
        dirty.notified().await;
        broadcast_selected_now(&context).await;
        tokio::time::sleep(MIN_PRESENCE_INTERVAL).await;
    }
}

/// Relays the current RPC activity to the user's presence, mirroring the
/// native client: the selected app wins (falling back to the most recent
/// one when it is gone), and the user's custom status survives alongside it.
async fn broadcast_selected_now(context: &RpcContext) {
    let rpc_activity = {
        let registry = context.registry.lock().await;
        match context.client.rich_presence_selection() {
            // A manual activity is owned by the user; RPC never overrides it.
            RichPresenceSelection::Manual => return,
            RichPresenceSelection::App(client_id) => registry
                .activity_for_client(&client_id)
                .or_else(|| registry.latest_activity()),
            RichPresenceSelection::Automatic => registry.latest_activity(),
        }
    };

    // Keep the custom status alive next to the relayed activity, like the
    // native client does when a game is running.
    let mut activities: Vec<ActivityInfo> = context
        .client
        .current_user_activities()
        .into_iter()
        .filter(|activity| activity.kind == ActivityKind::Custom)
        .collect();
    // Turn raw external image URLs into `mp:` refs here (only for the app we
    // broadcast), since the gateway needs them registered first.
    if let Some(mut activity) = rpc_activity {
        context
            .client
            .resolve_activity_external_assets(&mut activity)
            .await;
        activities.push(activity);
    }

    let status = context.client.current_user_status();
    if let Err(error) = context
        .client
        .update_presence_activity(status, activities.clone())
    {
        logging::error("rpc", format!("live presence update failed: {error}"));
        return;
    }
    if let Some(user_id) = context.client.current_user_id() {
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

        // Automatic (the default) relays the most recently updated activity,
        // with the custom status still alongside it.
        broadcast_selected_now(&context).await;
        let (status, activities) =
            presence_update_command(gateway_commands.recv().await.expect("presence update"));
        assert_eq!(status, PresenceStatus::Online);
        assert_eq!(
            activities,
            vec![custom, ActivityInfo::test(ActivityKind::Playing, "Song B"),]
        );

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
