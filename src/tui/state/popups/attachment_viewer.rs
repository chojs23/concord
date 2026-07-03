use crate::discord::{
    AppCommand, AttachmentInfo, AttachmentMediaType, DownloadAttachmentSource, InlinePreviewInfo,
    MediaPlaybackSource, MediaPlaybackTarget,
    ids::{Id, marker::MessageMarker},
};

use super::super::{AttachmentViewerItem, DashboardState};
use crate::tui::state::popups::{
    ActiveModalPopupKind, AttachmentViewerState, AttachmentViewerZoom, ModalPopup,
};

struct SelectedAttachmentViewerAttachment<'a> {
    message_id: Id<MessageMarker>,
    index: usize,
    total: usize,
    attachment: &'a AttachmentInfo,
}

impl DashboardState {
    pub fn open_attachment_viewer_for_selected_message(&mut self) -> bool {
        let Some(message) = self.selected_message_state() else {
            return false;
        };
        if message.attachments_in_display_order().next().is_none() {
            return false;
        }

        self.popups.modal = Some(ModalPopup::AttachmentViewer(AttachmentViewerState {
            message_id: message.id,
            selection: Default::default(),
            zoom: AttachmentViewerZoom::default(),
        }));
        true
    }

    pub fn close_attachment_viewer(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::AttachmentViewer) {
            self.popups.clear_modal();
        }
    }

    pub fn attachment_viewer_zoom(&self) -> AttachmentViewerZoom {
        self.popups
            .attachment_viewer()
            .map(|viewer| viewer.zoom)
            .unwrap_or_default()
    }

    pub fn toggle_attachment_viewer_fullscreen(&mut self) {
        if let Some(viewer) = self.popups.attachment_viewer_mut() {
            viewer.zoom = viewer.zoom.toggle_fullscreen();
        }
    }

    pub fn zoom_attachment_viewer_in(&mut self) {
        if let Some(viewer) = self.popups.attachment_viewer_mut() {
            viewer.zoom = viewer.zoom.zoom_in();
        }
    }

    pub fn zoom_attachment_viewer_out(&mut self) {
        if let Some(viewer) = self.popups.attachment_viewer_mut() {
            viewer.zoom = viewer.zoom.zoom_out();
        }
    }

    pub fn move_attachment_viewer_previous(&mut self) {
        if let Some(viewer) = self.popups.attachment_viewer_mut() {
            viewer.selection.move_up();
        }
    }

    pub fn move_attachment_viewer_next(&mut self) {
        let Some((message_id, selected)) = self
            .popups
            .attachment_viewer()
            .map(|viewer| (viewer.message_id, viewer.selection.selected()))
        else {
            return;
        };
        let count = self
            .attachment_viewer_attachments(message_id)
            .map_or(0, |attachments| attachments.len());
        if count == 0 {
            self.close_attachment_viewer();
            return;
        }
        if let Some(viewer) = self.popups.attachment_viewer_mut() {
            viewer
                .selection
                .select(selected.saturating_add(1).min(count.saturating_sub(1)));
        }
    }

    pub fn selected_attachment_viewer_item(&self) -> Option<AttachmentViewerItem> {
        let selected = self.selected_attachment_viewer_attachment()?;
        Some(AttachmentViewerItem {
            index: selected.index.saturating_add(1),
            total: selected.total,
            filename: selected.attachment.filename.clone(),
            url: selected.attachment.preferred_url().map(str::to_owned),
            size_bytes: selected.attachment.size,
            media_type: selected.attachment.media_type(),
        })
    }

    pub(in crate::tui) fn selected_attachment_viewer_preview(
        &self,
    ) -> Option<(Id<MessageMarker>, usize, InlinePreviewInfo<'_>)> {
        let selected = self.selected_attachment_viewer_attachment()?;
        let preview = selected.attachment.inline_preview_info()?;
        Some((selected.message_id, selected.index, preview))
    }

    pub fn download_selected_attachment_viewer_attachment(&mut self) -> Option<AppCommand> {
        let item = self.selected_attachment_viewer_item()?;
        let url = item.url?;
        let id = self.next_attachment_download_id();
        Some(AppCommand::DownloadAttachment {
            id,
            url,
            filename: item.filename,
            source: DownloadAttachmentSource::AttachmentViewer,
        })
    }

    pub fn play_selected_attachment_viewer_attachment(&mut self) -> Option<AppCommand> {
        if !self.media_playback_enabled() {
            self.show_media_playback_disabled_toast(std::time::Instant::now());
            return None;
        }
        let item = self.selected_attachment_viewer_item()?;
        if !matches!(
            item.media_type,
            Some(AttachmentMediaType::Video | AttachmentMediaType::Audio)
        ) {
            return None;
        }
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: item.url?,
                label: item.filename,
                source: MediaPlaybackSource::AttachmentViewer,
            },
            request_id: None,
        })
    }

    fn attachment_viewer_attachments(
        &self,
        message_id: Id<MessageMarker>,
    ) -> Option<Vec<&AttachmentInfo>> {
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
            .map(|message| message.attachments_in_display_order().collect())
    }

    fn selected_attachment_viewer_attachment(
        &self,
    ) -> Option<SelectedAttachmentViewerAttachment<'_>> {
        let viewer = self.popups.attachment_viewer()?;
        let attachments = self.attachment_viewer_attachments(viewer.message_id)?;
        let index = viewer.selection.selected_for_len(attachments.len());
        let attachment = *attachments.get(index)?;
        Some(SelectedAttachmentViewerAttachment {
            message_id: viewer.message_id,
            index,
            total: attachments.len(),
            attachment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AttachmentViewerZoom;

    #[test]
    fn zoom_in_steps_default_large_fullscreen_and_caps() {
        let zoom = AttachmentViewerZoom::Default;
        let zoom = zoom.zoom_in();
        assert_eq!(zoom, AttachmentViewerZoom::Large);
        let zoom = zoom.zoom_in();
        assert_eq!(zoom, AttachmentViewerZoom::Fullscreen);
        let zoom = zoom.zoom_in();
        assert_eq!(zoom, AttachmentViewerZoom::Fullscreen);
    }

    #[test]
    fn zoom_out_steps_fullscreen_large_default_and_caps() {
        let zoom = AttachmentViewerZoom::Fullscreen;
        let zoom = zoom.zoom_out();
        assert_eq!(zoom, AttachmentViewerZoom::Large);
        let zoom = zoom.zoom_out();
        assert_eq!(zoom, AttachmentViewerZoom::Default);
        let zoom = zoom.zoom_out();
        assert_eq!(zoom, AttachmentViewerZoom::Default);
    }

    #[test]
    fn toggle_fullscreen_round_trips() {
        let zoom = AttachmentViewerZoom::Default;
        let zoom = zoom.toggle_fullscreen();
        assert_eq!(zoom, AttachmentViewerZoom::Fullscreen);
        let zoom = zoom.toggle_fullscreen();
        assert_eq!(zoom, AttachmentViewerZoom::Default);

        let zoom = AttachmentViewerZoom::Large.toggle_fullscreen();
        assert_eq!(zoom, AttachmentViewerZoom::Fullscreen);
    }
}
