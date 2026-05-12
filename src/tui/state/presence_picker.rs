use crate::discord::{AppCommand, PresenceStatus};

use super::{DashboardState, popups::PresencePickerState, scroll::clamp_selected_index};

pub const PRESENCE_PICKER_ITEMS: &[(PresenceStatus, &str)] = &[
    (PresenceStatus::Online, "Online"),
    (PresenceStatus::Idle, "Idle"),
    (PresenceStatus::DoNotDisturb, "Do Not Disturb"),
    (PresenceStatus::Offline, "Invisible"),
];

impl DashboardState {
    pub fn is_presence_picker_open(&self) -> bool {
        self.presence_picker.is_some()
    }

    pub fn open_presence_picker(&mut self) {
        let current = self.discord.self_status();
        let selected = PRESENCE_PICKER_ITEMS
            .iter()
            .position(|(s, _)| *s == current)
            .unwrap_or(0);
        self.presence_picker = Some(PresencePickerState { selected });
    }

    pub fn close_presence_picker(&mut self) {
        self.presence_picker = None;
    }

    pub fn presence_picker_selected(&self) -> usize {
        self.presence_picker
            .as_ref()
            .map(|p| clamp_selected_index(p.selected, PRESENCE_PICKER_ITEMS.len()))
            .unwrap_or(0)
    }

    pub fn move_presence_picker_down(&mut self) {
        if let Some(picker) = self.presence_picker.as_mut() {
            let next = picker.selected.saturating_add(1);
            picker.selected = next.min(PRESENCE_PICKER_ITEMS.len().saturating_sub(1));
        }
    }

    pub fn move_presence_picker_up(&mut self) {
        if let Some(picker) = self.presence_picker.as_mut() {
            picker.selected = picker.selected.saturating_sub(1);
        }
    }

    pub fn activate_presence_picker(&mut self) -> Option<AppCommand> {
        let picker = self.presence_picker.as_ref()?;
        let selected = clamp_selected_index(picker.selected, PRESENCE_PICKER_ITEMS.len());
        let (status, _) = PRESENCE_PICKER_ITEMS[selected];
        self.discord.set_self_status(status);
        self.close_presence_picker();
        Some(AppCommand::SetPresence { status })
    }
}
