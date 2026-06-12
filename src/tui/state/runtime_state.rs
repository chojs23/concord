use std::time::Instant;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use super::{DashboardState, ToastKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ToastMessage {
    pub(super) text: String,
    pub(super) kind: ToastKind,
    pub(super) expires_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceConnectionUiState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeUiState {
    pub(super) toast_message: Option<ToastMessage>,
    pub(super) gateway_error: Option<String>,
    pub(super) voice_connection: Option<VoiceConnectionUiState>,
    pub(super) open_composer_in_editor_requested: bool,
    pub(super) paste_clipboard_requested: bool,
    pub(super) clipboard_paste_pending: bool,
    pub(super) copy_message_content_requested: Option<String>,
    pub(super) should_quit: bool,
    /// Inverted so the `Default` of `false` means "focused"; terminals that
    /// never report focus events keep the current notification behavior.
    pub(super) terminal_focus_lost: bool,
}

impl DashboardState {
    pub fn quit(&mut self) {
        self.runtime.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.runtime.should_quit
    }

    pub fn set_terminal_focused(&mut self, focused: bool) {
        self.runtime.terminal_focus_lost = !focused;
    }

    pub(super) fn terminal_focused(&self) -> bool {
        !self.runtime.terminal_focus_lost
    }
}
