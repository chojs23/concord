use serde_json::{Value, json};

use crate::discord::ActivityInfo;
use crate::discord::gateway::parse_activity;

pub(super) enum Command {
    SetActivity {
        pid: i64,
        activity: Option<Box<ActivityInfo>>,
        echo: Value,
        nonce: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CommandError {
    pub cmd: Option<String>,
    pub nonce: Option<String>,
    pub code: u16,
    pub message: &'static str,
}

impl CommandError {
    fn invalid_payload(cmd: Option<String>, nonce: Option<String>) -> Self {
        Self {
            cmd,
            nonce,
            code: 4000,
            message: "Invalid payload",
        }
    }

    fn invalid_command(cmd: String, nonce: Option<String>) -> Self {
        Self {
            cmd: Some(cmd),
            nonce,
            code: 4002,
            message: "Invalid command",
        }
    }
}

pub(super) fn parse_command(payload: &[u8], client_id: &str) -> Result<Command, CommandError> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| CommandError::invalid_payload(None, None))?;
    let object = value
        .as_object()
        .ok_or_else(|| CommandError::invalid_payload(None, None))?;
    let cmd = object
        .get("cmd")
        .and_then(Value::as_str)
        .filter(|cmd| !cmd.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::invalid_payload(None, command_nonce(object)))?;
    let nonce = match object.get("nonce") {
        Some(Value::String(nonce)) => Some(nonce.clone()),
        Some(Value::Null) | None => None,
        Some(_) => {
            return Err(CommandError::invalid_payload(Some(cmd), None));
        }
    };

    if cmd != "SET_ACTIVITY" {
        return Err(CommandError::invalid_command(cmd, nonce));
    }
    let args = object
        .get("args")
        .and_then(Value::as_object)
        .ok_or_else(|| CommandError::invalid_payload(Some(cmd.clone()), nonce.clone()))?;
    let pid = args
        .get("pid")
        .and_then(Value::as_i64)
        .ok_or_else(|| CommandError::invalid_payload(Some(cmd.clone()), nonce.clone()))?;
    let raw_activity = args
        .get("activity")
        .cloned()
        .ok_or_else(|| CommandError::invalid_payload(Some(cmd.clone()), nonce.clone()))?;
    let activity = match &raw_activity {
        Value::Null => None,
        Value::Object(_) => Some(Box::new(
            build_activity(&raw_activity, client_id)
                .ok_or_else(|| CommandError::invalid_payload(Some(cmd), nonce.clone()))?,
        )),
        _ => {
            return Err(CommandError::invalid_payload(Some(cmd), nonce));
        }
    };
    Ok(Command::SetActivity {
        pid,
        activity,
        echo: raw_activity,
        nonce,
    })
}

fn command_nonce(object: &serde_json::Map<String, Value>) -> Option<String> {
    object
        .get("nonce")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The RPC payload carries no app name (Discord derives it from `client_id`),
/// so we stamp a placeholder that the caller resolves before broadcasting.
fn build_activity(value: &Value, client_id: &str) -> Option<ActivityInfo> {
    let mut activity = parse_activity(value)?;
    activity.application_id = Some(client_id.to_owned());
    if activity.name.is_empty() {
        activity.name = client_id.to_owned();
    }
    // Some RPC clients (e.g. presence.nvim) send seconds though the gateway
    // expects milliseconds. Normalize or the elapsed timer is off by 1000x.
    if let Some(timestamps) = activity.timestamps.as_mut() {
        timestamps.start = timestamps.start.map(normalize_millis);
        timestamps.end = timestamps.end.map(normalize_millis);
    }
    Some(activity)
}

/// A nonzero value below ~year 2001 in millis is really seconds, so scale it up.
fn normalize_millis(value: i64) -> i64 {
    const MILLIS_THRESHOLD: i64 = 1_000_000_000_000;
    if value != 0 && value.abs() < MILLIS_THRESHOLD {
        value * 1000
    } else {
        value
    }
}

pub(super) fn build_command_ack(cmd: &str, nonce: Option<&str>, data: Value) -> Vec<u8> {
    json!({
        "cmd": cmd,
        "evt": Value::Null,
        "nonce": nonce,
        "data": data,
    })
    .to_string()
    .into_bytes()
}

pub(super) fn build_command_error(error: &CommandError) -> Vec<u8> {
    json!({
        "cmd": error.cmd,
        "evt": "ERROR",
        "nonce": error.nonce,
        "data": {
            "code": error.code,
            "message": error.message,
        },
    })
    .to_string()
    .into_bytes()
}

/// Only a numeric snowflake is accepted: this id is interpolated into
/// authenticated REST paths, so a non-numeric value could inject a path.
pub(super) fn parse_handshake_client_id(payload: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    let id = value.get("client_id").and_then(|id| {
        id.as_str()
            .map(str::to_owned)
            .or_else(|| id.as_u64().map(|number| number.to_string()))
    })?;
    (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then_some(id)
}

/// The `user` object is required: clients like presence.nvim read
/// `data.user.username` immediately and crash if it is absent.
pub(super) fn build_ready_payload(user: Option<(String, String)>) -> Vec<u8> {
    let (id, username) = user.unwrap_or_else(|| ("0".to_owned(), "concord".to_owned()));
    json!({
        "cmd": "DISPATCH",
        "evt": "READY",
        "data": {
            "v": 1,
            "config": {
                "cdn_host": "cdn.discordapp.com",
                "api_endpoint": "//discord.com/api",
                "environment": "production",
            },
            "user": {
                "id": id,
                "username": username.clone(),
                "global_name": username,
                "discriminator": "0",
                "avatar": Value::Null,
                "bot": false,
                "flags": 0,
                "premium_type": 0,
            },
        },
    })
    .to_string()
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{Command, build_ready_payload, parse_command, parse_handshake_client_id};
    use crate::discord::ActivityKind;
    use serde_json::json;

    #[test]
    fn parse_command_maps_set_activity_into_rich_activity() {
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "nonce": "n1",
            "args": {
                "pid": 4321,
                "activity": {
                    "details": "Editing main.rs",
                    "state": "Workspace: concord",
                    "timestamps": { "start": 1_700_000_000_000i64 },
                    "assets": { "large_image": "rust", "large_text": "Rust" },
                    "buttons": [{ "label": "Repo", "url": "https://example.com" }]
                }
            }
        })
        .to_string();

        let command = parse_command(payload.as_bytes(), "999").expect("command parses");
        let Command::SetActivity {
            pid,
            activity,
            echo,
            nonce,
        } = command;
        assert_eq!(pid, 4321);
        assert_eq!(nonce.as_deref(), Some("n1"));
        assert_eq!(echo["details"].as_str(), Some("Editing main.rs"));
        let activity = activity.expect("activity present");
        assert_eq!(activity.application_id.as_deref(), Some("999"));
        assert_eq!(activity.name, "999");
        assert_eq!(activity.details.as_deref(), Some("Editing main.rs"));
        assert_eq!(
            activity.timestamps.and_then(|t| t.start),
            Some(1_700_000_000_000)
        );
        assert_eq!(activity.buttons[0].label, "Repo");
        assert_eq!(activity.buttons[0].url, "https://example.com");
    }

    #[test]
    fn parse_command_normalizes_second_timestamps_to_millis() {
        let seconds = 1_700_000_000i64;
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 1, "activity": { "timestamps": { "start": seconds } } }
        })
        .to_string();
        let command = parse_command(payload.as_bytes(), "999").expect("command parses");
        let Command::SetActivity { activity, .. } = command;
        let activity = activity.expect("activity present");
        assert_eq!(
            activity.timestamps.and_then(|t| t.start),
            Some(seconds * 1000)
        );

        let millis = 1_700_000_000_000i64;
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 1, "activity": { "timestamps": { "start": millis } } }
        })
        .to_string();
        let command = parse_command(payload.as_bytes(), "999").expect("command parses");
        let Command::SetActivity { activity, .. } = command;
        assert_eq!(
            activity
                .expect("activity present")
                .timestamps
                .and_then(|t| t.start),
            Some(millis)
        );
    }

    #[test]
    fn parse_command_preserves_rpc_activity_fields() {
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": 1,
                "activity": {
                    "type": 6,
                    "name": "Hang Status",
                    "details_url": "https://example.com/details",
                    "state_url": "https://example.com/state",
                    "supported_platforms": ["desktop", "xbox"],
                    "party": {
                        "id": "party-1",
                        "size": [2, 5],
                        "privacy": 1
                    },
                    "secrets": {
                        "join": "join-secret",
                        "spectate": "spectate-secret"
                    },
                    "instance": true,
                    "future_rpc_field": { "kept": true }
                }
            }
        })
        .to_string();

        let command = parse_command(payload.as_bytes(), "999").expect("command parses");
        let Command::SetActivity { activity, .. } = command;
        let activity = activity.expect("activity present");
        assert_eq!(activity.kind, ActivityKind::Hang);
        assert_eq!(
            activity.details_url.as_deref(),
            Some("https://example.com/details")
        );
        assert_eq!(
            activity.state_url.as_deref(),
            Some("https://example.com/state")
        );
        assert_eq!(activity.supported_platforms, ["desktop", "xbox"]);
        let party = activity.party.expect("party present");
        assert_eq!(party.id.as_deref(), Some("party-1"));
        assert_eq!(party.size, Some((2, 5)));
        assert_eq!(party.privacy, Some(1));
        let secrets = activity.secrets.expect("secrets present");
        assert_eq!(secrets.join.as_deref(), Some("join-secret"));
        assert_eq!(secrets.spectate.as_deref(), Some("spectate-secret"));
        assert_eq!(activity.instance, Some(true));
        assert_eq!(
            activity.extra_fields["future_rpc_field"],
            json!({ "kept": true })
        );
    }

    #[test]
    fn parse_command_treats_null_activity_as_clear() {
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": 1, "activity": null }
        })
        .to_string();

        let command = parse_command(payload.as_bytes(), "999").expect("command parses");
        let Command::SetActivity { activity, .. } = command;
        assert!(activity.is_none());
    }

    #[test]
    fn parse_handshake_client_id_reads_string_and_numeric_forms() {
        assert_eq!(
            parse_handshake_client_id(br#"{"v":1,"client_id":"12345"}"#).as_deref(),
            Some("12345")
        );
        assert_eq!(
            parse_handshake_client_id(br#"{"v":1,"client_id":12345}"#).as_deref(),
            Some("12345")
        );
        assert!(parse_handshake_client_id(br#"{"v":1,"client_id":"../users/@me"}"#).is_none());
        assert!(parse_handshake_client_id(br#"{"v":1,"client_id":""}"#).is_none());
    }

    #[test]
    fn ready_payload_announces_dispatch_ready_with_user() {
        let value: serde_json::Value = serde_json::from_slice(&build_ready_payload(Some((
            "42".to_owned(),
            "neo".to_owned(),
        ))))
        .expect("ready payload is json");
        assert_eq!(value["cmd"].as_str(), Some("DISPATCH"));
        assert_eq!(value["evt"].as_str(), Some("READY"));
        assert_eq!(value["data"]["v"].as_u64(), Some(1));
        assert_eq!(value["data"]["user"]["id"].as_str(), Some("42"));
        assert_eq!(value["data"]["user"]["username"].as_str(), Some("neo"));

        let fallback: serde_json::Value =
            serde_json::from_slice(&build_ready_payload(None)).expect("ready payload is json");
        assert!(fallback["data"]["user"]["username"].as_str().is_some());
    }
}
