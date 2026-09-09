use super::{ClipboardPasteRequest, ClipboardPasteTarget, DashboardState};

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
        let Some(target) = self.clipboard_paste_target() else {
            return;
        };
        self.runtime.next_clipboard_paste_request_id =
            self.runtime.next_clipboard_paste_request_id.wrapping_add(1);
        self.runtime.paste_clipboard_requested = Some(ClipboardPasteRequest {
            request_id: self.runtime.next_clipboard_paste_request_id,
            target,
        });
    }

    pub fn take_paste_clipboard_request(&mut self) -> Option<u64> {
        let request = self.runtime.paste_clipboard_requested.take()?;
        // A slow read can leave this request queued while the user changes editors.
        (self.clipboard_paste_target() == Some(request.target)).then_some(request.request_id)
    }

    pub(in crate::tui) fn request_terminal_refresh(&mut self) {
        self.runtime.terminal_refresh_requested = true;
    }

    pub(in crate::tui) fn take_terminal_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.runtime.terminal_refresh_requested)
    }

    pub fn accepts_clipboard_paste(&self) -> bool {
        self.is_composing()
            || self.is_forum_post_composer_active()
            || self.is_user_profile_popup_editing()
            || self.accepts_user_profile_avatar_paste()
    }

    pub(in crate::tui) fn start_clipboard_paste(&mut self, request_id: u64) -> bool {
        if !self.accepts_clipboard_paste() || self.runtime.clipboard_paste_request.is_some() {
            return false;
        }
        let Some(target) = self.clipboard_paste_target() else {
            return false;
        };
        self.runtime.clipboard_paste_request = Some(ClipboardPasteRequest { request_id, target });
        true
    }

    pub fn begin_clipboard_paste(&mut self, request_id: u64) -> bool {
        if self.runtime.clipboard_paste_pending || !self.clipboard_paste_request_matches(request_id)
        {
            return false;
        }
        self.runtime.clipboard_paste_pending = true;
        true
    }

    pub(in crate::tui) fn finish_clipboard_paste(&mut self, request_id: u64) -> bool {
        let accepted = self.clipboard_paste_request_matches(request_id);
        let owns_request = self
            .runtime
            .clipboard_paste_request
            .is_some_and(|request| request.request_id == request_id);
        if owns_request {
            self.runtime.clipboard_paste_request = None;
            self.runtime.clipboard_paste_pending = false;
        }
        accepted
    }

    pub(in crate::tui::state) fn cancel_clipboard_paste(&mut self) {
        self.runtime.paste_clipboard_requested = None;
        self.runtime.clipboard_paste_request = None;
        self.runtime.clipboard_paste_pending = false;
    }

    pub fn clipboard_paste_pending(&self) -> bool {
        self.runtime.clipboard_paste_pending
    }

    pub fn pending_composer_upload_line_count(&self) -> usize {
        self.composer.pending_composer_attachments.len()
            + usize::from(self.runtime.clipboard_paste_pending)
    }

    fn clipboard_paste_request_matches(&self, request_id: u64) -> bool {
        self.runtime.clipboard_paste_request.is_some_and(|request| {
            request.request_id == request_id
                && self.clipboard_paste_target() == Some(request.target)
        })
    }

    fn clipboard_paste_target(&self) -> Option<ClipboardPasteTarget> {
        if self.accepts_user_profile_avatar_paste()
            || self.is_user_profile_avatar_clipboard_paste_pending()
        {
            return Some(ClipboardPasteTarget::UserProfileAvatar);
        }
        if let Some(field) = self
            .popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.editing)
        {
            return Some(ClipboardPasteTarget::UserProfileText(field));
        }
        if let Some(popup) = self.popups.forum_post_composer() {
            return Some(ClipboardPasteTarget::ForumPost(popup.editing));
        }
        if self.active_modal_popup_kind().is_some() {
            return None;
        }
        self.is_composing()
            .then_some(ClipboardPasteTarget::Composer)
    }
}
