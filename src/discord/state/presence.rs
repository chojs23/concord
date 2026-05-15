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
}
