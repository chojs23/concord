//! Read-only voice snapshots for local overlays, separate from Discord's RPC protocol.

mod socket;

use std::{io, sync::Arc, time::Duration};

use serde::Serialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::watch,
    task::JoinSet,
    time::{sleep, timeout},
};

use super::{DiscordClient, DiscordState};
use crate::logging;

const MAX_CLIENTS: usize = 16;
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
type StateLine = Arc<[u8]>;

pub(super) async fn run(client: DiscordClient, mut enabled: watch::Receiver<bool>) {
    let mut last_error = None;
    loop {
        if !*enabled.borrow_and_update() {
            last_error = None;
            if enabled.changed().await.is_err() {
                return;
            }
            continue;
        }

        let listener = tokio::select! {
            biased;
            changed = enabled.changed() => {
                if changed.is_err() {
                    return;
                }
                continue;
            }
            listener = socket::bind() => listener,
        };
        let result = match listener {
            Ok(listener) => serve(&client, listener, &mut enabled).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => continue,
            Err(error) => {
                let message = format!("voice state export unavailable: {error}");
                if last_error.as_ref() != Some(&message) {
                    logging::error("voice_export", &message);
                    last_error = Some(message);
                }
            }
        }
        // A temporary bind failure must not permanently disable the feature.
        tokio::select! {
            changed = enabled.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = sleep(RETRY_INTERVAL) => {}
        }
    }
}

async fn serve(
    client: &DiscordClient,
    listener: socket::BoundListener,
    enabled: &mut watch::Receiver<bool>,
) -> io::Result<()> {
    let mut snapshots = client.subscribe_snapshots();
    let mut navigation_revision = snapshots.borrow_and_update().navigation;
    let (updates, _) = watch::channel(state_line(client)?);
    let mut tasks = JoinSet::new();
    loop {
        if !*enabled.borrow_and_update() {
            return Ok(());
        }
        tokio::select! {
            changed = enabled.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
            accepted = listener.accept() => {
                let stream = accepted?;
                if tasks.len() >= MAX_CLIENTS {
                    continue;
                }
                // Read again on connect, even if a state notification is still pending.
                publish(&updates, state_line(client)?);
                tasks.spawn(serve_client(stream, updates.subscribe()));
            }
            changed = snapshots.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                let revision = snapshots.borrow_and_update().navigation;
                if revision != navigation_revision {
                    navigation_revision = revision;
                    if !tasks.is_empty() {
                        publish(&updates, state_line(client)?);
                    }
                }
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

fn publish(updates: &watch::Sender<StateLine>, next: StateLine) {
    updates.send_if_modified(|latest| {
        if *latest == next {
            false
        } else {
            *latest = next;
            true
        }
    });
}

async fn serve_client<S>(mut stream: S, mut updates: watch::Receiver<StateLine>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(&mut stream);
    let mut input = [0u8; 1];
    tokio::select! {
        // This protocol accepts no commands. EOF or incoming data ends the connection.
        _ = reader.read(&mut input) => {}
        () = async {
            loop {
                // Keep only the latest snapshot, not a queue of obsolete speaking edges.
                let line = Arc::clone(&updates.borrow_and_update());
                if !matches!(timeout(WRITE_TIMEOUT, writer.write_all(&line)).await, Ok(Ok(()))) {
                    break;
                }
                if updates.changed().await.is_err() {
                    break;
                }
            }
        } => {}
    }
}

fn state_line(client: &DiscordClient) -> io::Result<StateLine> {
    // Clone the shared snapshot under a short read lock. Collecting members and
    // serializing JSON then happen without holding the live Discord state lock.
    let state = client.current_discord_snapshot().to_state();
    let mut line = serde_json::to_vec(&VoiceSnapshot::from_state(&state))?;
    line.push(b'\n');
    Ok(line.into())
}

#[derive(Debug, Serialize)]
struct VoiceSnapshot {
    v: u8,
    self_id: Option<String>,
    channel: Option<Channel>,
    members: Vec<Member>,
}

#[derive(Debug, Serialize)]
struct Channel {
    id: String,
    name: String,
    guild_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct Member {
    id: String,
    name: String,
    avatar: Option<String>,
    speaking: bool,
    mute: bool,
    deaf: bool,
    self_mute: bool,
    self_deaf: bool,
    stream: bool,
}

impl VoiceSnapshot {
    fn from_state(state: &DiscordState) -> Self {
        let mut snapshot = Self {
            v: 1,
            self_id: state.current_user_id().map(|id| id.to_string()),
            channel: None,
            members: Vec::new(),
        };
        let Some(connection) = state.current_user_voice_connection() else {
            return snapshot;
        };
        let channel = state.channel(connection.channel_id);
        let guild_id = connection.guild_id();
        snapshot.channel = Some(Channel {
            id: connection.channel_id.to_string(),
            name: channel.map_or_else(
                || format!("channel-{}", connection.channel_id),
                |channel| channel.name.clone(),
            ),
            guild_id: guild_id.map(|id| id.to_string()),
        });
        let mut participants = match guild_id {
            Some(guild_id) => state.voice_participants_for_channel(guild_id, connection.channel_id),
            None => state.voice_participants_for_private_channel(connection.channel_id),
        };
        // Stable wire ordering avoids updates caused only by display-name sorting.
        participants.sort_by_key(|participant| participant.user_id.get());
        snapshot.members = participants
            .into_iter()
            .map(|participant| {
                let avatar = match guild_id {
                    Some(guild_id) => state
                        .member_for_guild(guild_id, participant.user_id)
                        .and_then(|member| member.avatar_url.clone()),
                    None => channel
                        .and_then(|channel| {
                            channel
                                .recipients
                                .iter()
                                .find(|recipient| recipient.user_id == participant.user_id)
                        })
                        .and_then(|recipient| recipient.avatar_url.clone()),
                };
                Member {
                    id: participant.user_id.to_string(),
                    name: participant.display_name,
                    avatar,
                    speaking: participant.speaking,
                    mute: participant.mute,
                    deaf: participant.deaf,
                    self_mute: participant.self_mute,
                    self_deaf: participant.self_deaf,
                    stream: participant.self_stream,
                }
            })
            .collect();
        snapshot
    }
}

#[cfg(test)]
mod tests;
