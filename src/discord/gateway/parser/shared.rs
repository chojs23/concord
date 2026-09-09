use serde_json::Value;

use crate::discord::{PresenceStatus, ids::Id};

pub(super) use crate::discord::display_name::{
    display_name_from_parts, display_name_from_parts_or_unknown,
};
pub(super) use crate::discord::json::extra_fields;

pub(super) fn parse_status(value: &str) -> PresenceStatus {
    match value {
        "online" => PresenceStatus::Online,
        "idle" => PresenceStatus::Idle,
        "dnd" => PresenceStatus::DoNotDisturb,
        "offline" | "invisible" => PresenceStatus::Offline,
        _ => PresenceStatus::Unknown,
    }
}

pub(super) fn parse_id<M>(value: &Value) -> Option<Id<M>> {
    value
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
}

pub(super) fn parse_id_array<M>(value: Option<&Value>) -> Vec<Id<M>> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(parse_id::<M>).collect())
        .unwrap_or_default()
}

pub(super) fn parse_mute_config(value: &Value) -> (Option<String>, Option<i64>) {
    let config = value.get("mute_config");
    let end_time = config
        .and_then(|config| config.get("end_time"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let selected_time_window = config
        .and_then(|config| config.get("selected_time_window"))
        .and_then(Value::as_i64);
    (end_time, selected_time_window)
}

pub(super) fn parse_nonnegative_i64(value: &Value) -> Option<i64> {
    value.as_u64().and_then(|value| i64::try_from(value).ok())
}
