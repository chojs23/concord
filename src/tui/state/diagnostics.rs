use super::DashboardState;

impl DashboardState {
    pub fn update_available_version(&self) -> Option<&str> {
        self.discord.update_available_version.as_deref()
    }

    pub fn gateway_error(&self) -> Option<&str> {
        self.runtime.gateway_error.as_deref()
    }

    pub fn request_open_composer_in_editor(&mut self) {
        self.runtime.open_composer_in_editor_requested = true;
    }

    pub fn take_open_composer_in_editor_request(&mut self) -> bool {
        std::mem::take(&mut self.runtime.open_composer_in_editor_requested)
    }

    pub fn request_paste_clipboard(&mut self) {
        self.runtime.paste_clipboard_requested = true;
    }

    pub fn take_paste_clipboard_request(&mut self) -> bool {
        std::mem::take(&mut self.runtime.paste_clipboard_requested)
    }

    pub fn accepts_clipboard_paste(&self) -> bool {
        self.is_composing()
            || self.is_user_profile_popup_editing()
            || self.accepts_user_profile_avatar_paste()
    }

    pub fn begin_clipboard_paste(&mut self) -> bool {
        if !self.accepts_clipboard_paste() || self.runtime.clipboard_paste_pending {
            return false;
        }
        self.runtime.clipboard_paste_pending = true;
        true
    }

    pub fn finish_clipboard_paste(&mut self) {
        self.runtime.clipboard_paste_pending = false;
    }

    pub fn clipboard_paste_pending(&self) -> bool {
        self.runtime.clipboard_paste_pending
    }

    pub fn pending_composer_upload_line_count(&self) -> usize {
        self.composer.pending_composer_attachments.len()
            + usize::from(self.runtime.clipboard_paste_pending)
    }
}
