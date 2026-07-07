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
