use crate::discord::ChannelVisibilityStats;

use super::{ActiveGuildScope, DashboardState};
use crate::logging;

impl DashboardState {
    pub fn update_available_version(&self) -> Option<&str> {
        self.update_available_version.as_deref()
    }

    pub fn is_debug_log_popup_open(&self) -> bool {
        self.debug_log_popup_open
    }

    pub fn toggle_debug_log_popup(&mut self) {
        self.debug_log_popup_open = !self.debug_log_popup_open;
    }

    pub fn close_debug_log_popup(&mut self) {
        self.debug_log_popup_open = false;
    }

    pub fn request_open_composer_in_editor(&mut self) {
        self.open_composer_in_editor_requested = true;
    }

    pub fn take_open_composer_in_editor_request(&mut self) -> bool {
        std::mem::take(&mut self.open_composer_in_editor_requested)
    }

    pub fn debug_log_lines(&self) -> Vec<String> {
        logging::error_entries()
            .into_iter()
            .map(|entry| entry.line())
            .collect()
    }

    /// Visible vs. permission-hidden channel counts for the active scope.
    /// Surfaced in the debug-log popup so the user can verify whether a
    /// missing channel is actually being filtered by `can_view_channel` or
    /// just isn't in the cache. DM scope always reports `(N, 0)`.
    pub fn debug_channel_visibility(&self) -> ChannelVisibilityStats {
        match self.active_guild {
            ActiveGuildScope::Unset => ChannelVisibilityStats::default(),
            ActiveGuildScope::DirectMessages => self.discord.channel_visibility_stats(None),
            ActiveGuildScope::Guild(guild_id) => {
                self.discord.channel_visibility_stats(Some(guild_id))
            }
        }
    }
}
