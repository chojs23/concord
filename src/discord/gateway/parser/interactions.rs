use serde_json::Value;

use crate::discord::{ApplicationCommandChoiceInfo, events::AppEvent, ids::marker::GuildMarker};

use super::shared::parse_id;

pub(super) fn parse_application_command_index_update(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::ApplicationCommandIndexUpdated {
        guild_id: parse_id::<GuildMarker>(data.get("guild_id")?)?,
    })
}

pub(super) fn parse_interaction_success(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::InteractionSucceeded {
        interaction_id: parse_raw_id(data.get("id")?)?,
        nonce: parse_nonce(data),
        correlated: false,
    })
}

pub(super) fn parse_interaction_failure(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::InteractionFailed {
        interaction_id: parse_raw_id(data.get("id")?)?,
        nonce: parse_nonce(data),
        reason_code: data.get("reason_code").and_then(Value::as_u64).unwrap_or(1),
        correlated: false,
    })
}

pub(super) fn parse_application_command_autocomplete_response(data: &Value) -> Option<AppEvent> {
    let choices = data
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| {
            Some(ApplicationCommandChoiceInfo {
                name: choice.get("name")?.as_str()?.to_owned(),
                value: choice.get("value")?.clone(),
            })
        })
        .collect();
    Some(AppEvent::ApplicationCommandAutocompleteResponse {
        nonce: parse_nonce(data),
        choices,
    })
}

fn parse_raw_id(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|value| value.parse().ok())
        .or_else(|| value.as_u64())
}

fn parse_nonce(data: &Value) -> Option<String> {
    data.get("nonce").and_then(|nonce| match nonce {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}
