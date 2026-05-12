use crate::discord::{
    AppCommand, DownloadAttachmentSource, InlinePreviewInfo, ids::Id, ids::marker::MessageMarker,
};

use super::scroll::clamp_selected_index;
use super::{DashboardState, ImageViewerItem};
use crate::tui::state::popups::ImageViewerState;

impl DashboardState {
    pub fn is_image_viewer_open(&self) -> bool {
        self.image_viewer.is_some()
    }

    pub fn open_image_viewer_for_selected_message(&mut self) -> bool {
        if !self.show_images() {
            return false;
        }

        let Some(message) = self.selected_message_state() else {
            return false;
        };
        if message.inline_previews().is_empty() {
            return false;
        }

        self.image_viewer = Some(ImageViewerState {
            message_id: message.id,
            selected: 0,
            download_message: None,
        });
        true
    }

    pub fn close_image_viewer(&mut self) {
        self.image_viewer = None;
    }

    pub fn move_image_viewer_previous(&mut self) {
        if let Some(viewer) = &mut self.image_viewer {
            viewer.selected = viewer.selected.saturating_sub(1);
        }
    }

    pub fn move_image_viewer_next(&mut self) {
        let Some((message_id, selected)) = self
            .image_viewer
            .as_ref()
            .map(|viewer| (viewer.message_id, viewer.selected))
        else {
            return;
        };
        let count = self.image_viewer_preview_count(message_id);
        if count == 0 {
            self.close_image_viewer();
            return;
        }
        if let Some(viewer) = &mut self.image_viewer {
            viewer.selected = selected.saturating_add(1).min(count.saturating_sub(1));
        }
    }

    pub fn selected_image_viewer_item(&self) -> Option<ImageViewerItem> {
        let viewer = self.image_viewer.as_ref()?;
        let previews = self.image_viewer_previews(viewer.message_id)?;
        let selected = clamp_selected_index(viewer.selected, previews.len());
        let preview = previews.get(selected)?;
        Some(ImageViewerItem {
            index: selected.saturating_add(1),
            total: previews.len(),
            filename: preview.filename.to_owned(),
            url: preview.url.to_owned(),
        })
    }

    pub(in crate::tui) fn selected_image_viewer_preview(
        &self,
    ) -> Option<(Id<MessageMarker>, usize, InlinePreviewInfo<'_>)> {
        let viewer = self.image_viewer.as_ref()?;
        let previews = self.image_viewer_previews(viewer.message_id)?;
        let selected = clamp_selected_index(viewer.selected, previews.len());
        let preview = previews.get(selected).copied()?;
        Some((viewer.message_id, selected, preview))
    }

    pub fn image_viewer_download_message(&self) -> Option<&str> {
        self.image_viewer
            .as_ref()
            .and_then(|viewer| viewer.download_message.as_deref())
    }

    pub fn record_image_viewer_download_completed(&mut self, path: &str) {
        if let Some(viewer) = &mut self.image_viewer {
            viewer.download_message = Some(format!("Downloaded to {path}"));
        }
    }

    pub fn download_selected_image_viewer_image(&mut self) -> Option<AppCommand> {
        let item = self.selected_image_viewer_item()?;
        if let Some(viewer) = &mut self.image_viewer {
            viewer.download_message = Some("Downloading image...".to_owned());
        }
        Some(AppCommand::DownloadAttachment {
            url: item.url,
            filename: item.filename,
            source: DownloadAttachmentSource::ImageViewer,
        })
    }

    fn image_viewer_previews(
        &self,
        message_id: Id<MessageMarker>,
    ) -> Option<Vec<InlinePreviewInfo<'_>>> {
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
            .map(|message| message.inline_previews())
    }

    fn image_viewer_preview_count(&self, message_id: Id<MessageMarker>) -> usize {
        match self.image_viewer_previews(message_id) {
            Some(previews) => previews.len(),
            None => 0,
        }
    }
}
