use std::time::Instant;

use crate::discord::ids::{Id, marker::ChannelMarker};
use crate::discord::{
    AttachmentDownloadId, DownloadAttachmentSource, MediaPlaybackRequestId, VoiceScope,
};

use super::{AttachmentDownloadProgressView, DashboardState, ToastKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ToastMessage {
    pub(super) text: String,
    pub(super) kind: ToastKind,
    pub(super) expires_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VoiceConnectionUiState {
    pub(super) scope: VoiceScope,
    pub(super) channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AttachmentDownloadUiState {
    pub(super) id: AttachmentDownloadId,
    pub(super) filename: String,
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: Option<u64>,
    pub(super) source: DownloadAttachmentSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MediaPlaybackPreparingUiState {
    pub(super) request_id: MediaPlaybackRequestId,
    pub(super) url: String,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeUiState {
    pub(super) toast_message: Option<ToastMessage>,
    pub(super) media_playback_preparing: Option<MediaPlaybackPreparingUiState>,
    pub(super) gateway_error: Option<String>,
    pub(super) voice_connection: Option<VoiceConnectionUiState>,
    pub(super) open_composer_in_editor_requested: bool,
    pub(super) open_forum_post_body_in_editor_requested: bool,
    pub(super) paste_clipboard_requested: bool,
    pub(super) clipboard_paste_pending: bool,
    /// Pending clipboard copy: the text plus the success toast to show. Used by
    /// message copy, forum post link/id copy, and similar one-shot copies.
    pub(super) copy_text_requested: Option<(String, &'static str)>,
    pub(super) attachment_downloads: Vec<AttachmentDownloadUiState>,
    pub(super) next_attachment_download_id: u64,
    pub(super) next_media_playback_request_id: u64,
    pub(super) should_quit: bool,
    pub(super) should_sign_out: bool,
    /// Inverted so the `Default` of `false` means "focused"; terminals that
    /// never report focus events keep the current notification behavior.
    pub(super) terminal_focus_lost: bool,
}

impl DashboardState {
    pub fn quit(&mut self) {
        self.runtime.should_quit = true;
    }

    pub(in crate::tui) fn sign_out(&mut self) {
        self.runtime.should_sign_out = true;
    }

    pub fn should_quit(&self) -> bool {
        self.runtime.should_quit || self.runtime.should_sign_out
    }

    pub(in crate::tui) fn should_sign_out(&self) -> bool {
        self.runtime.should_sign_out
    }

    pub fn set_terminal_focused(&mut self, focused: bool) {
        self.runtime.terminal_focus_lost = !focused;
    }

    pub(super) fn terminal_focused(&self) -> bool {
        !self.runtime.terminal_focus_lost
    }

    pub(in crate::tui) fn next_attachment_download_id(&mut self) -> AttachmentDownloadId {
        let id = AttachmentDownloadId::new(self.runtime.next_attachment_download_id);
        self.runtime.next_attachment_download_id =
            self.runtime.next_attachment_download_id.saturating_add(1);
        id
    }

    pub(in crate::tui) fn next_media_playback_request_id(&mut self) -> MediaPlaybackRequestId {
        let id = MediaPlaybackRequestId::new(self.runtime.next_media_playback_request_id);
        self.runtime.next_media_playback_request_id = self
            .runtime
            .next_media_playback_request_id
            .saturating_add(1);
        id
    }

    pub(in crate::tui) fn record_attachment_download_started(
        &mut self,
        id: AttachmentDownloadId,
        filename: String,
        total_bytes: Option<u64>,
        source: DownloadAttachmentSource,
    ) {
        if let Some(download) = self
            .runtime
            .attachment_downloads
            .iter_mut()
            .find(|download| download.id == id)
        {
            download.filename = filename;
            download.downloaded_bytes = 0;
            download.total_bytes = total_bytes;
            download.source = source;
            return;
        }
        self.runtime
            .attachment_downloads
            .push(AttachmentDownloadUiState {
                id,
                filename,
                downloaded_bytes: 0,
                total_bytes,
                source,
            });
    }

    pub(in crate::tui) fn record_attachment_download_progress(
        &mut self,
        id: AttachmentDownloadId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    ) {
        if let Some(download) = self
            .runtime
            .attachment_downloads
            .iter_mut()
            .find(|download| download.id == id)
        {
            download.downloaded_bytes = downloaded_bytes;
            if total_bytes.is_some() {
                download.total_bytes = total_bytes;
            }
        }
    }

    pub(in crate::tui) fn remove_attachment_download(
        &mut self,
        id: AttachmentDownloadId,
    ) -> Option<String> {
        let index = self
            .runtime
            .attachment_downloads
            .iter()
            .position(|download| download.id == id)?;
        Some(self.runtime.attachment_downloads.remove(index).filename)
    }

    pub fn attachment_downloads(&self) -> Vec<AttachmentDownloadProgressView> {
        self.runtime
            .attachment_downloads
            .iter()
            .map(|download| AttachmentDownloadProgressView {
                id: download.id,
                filename: download.filename.clone(),
                downloaded_bytes: download.downloaded_bytes,
                total_bytes: download.total_bytes,
            })
            .collect()
    }
}
