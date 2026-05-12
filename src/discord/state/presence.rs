use crate::discord::PresenceStatus;

#[derive(Clone, Debug)]
pub(super) struct PresenceState {
    pub current: PresenceStatus,
}

impl PresenceState {
    pub fn new() -> Self {
        Self {
            current: PresenceStatus::Unknown,
        }
    }

    /// The string sent in gateway op 3 `status` field.
    pub fn gateway_value(&self) -> &'static str {
        match self.current {
            PresenceStatus::Online => "online",
            PresenceStatus::Idle => "idle",
            PresenceStatus::DoNotDisturb => "dnd",
            PresenceStatus::Offline => "invisible",
            PresenceStatus::Unknown => "unknown",
        }
    }
}
