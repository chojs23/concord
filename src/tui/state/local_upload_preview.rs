use crate::discord::MessageAttachmentUpload;
use crate::tui::state::LocalUploadPreviewView;
use ratatui_image::protocol::Protocol;

#[derive(Debug)]
pub(in crate::tui::state) struct LocalUploadPreviewState {
    pub(in crate::tui::state) attachment_index: usize,
    pub(in crate::tui::state) generation: u64,
    pub(in crate::tui::state) filename: String,
    pub(in crate::tui::state) state: LocalUploadPreviewStatus,
}

pub(in crate::tui::state) enum LocalUploadPreviewStatus {
    Pending,
    Loading,
    Ready(Protocol),
    Failed(String),
}

impl std::fmt::Debug for LocalUploadPreviewStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => formatter.write_str("Pending"),
            Self::Loading => formatter.write_str("Loading"),
            Self::Ready(_) => formatter.write_str("Ready(<protocol>)"),
            Self::Failed(message) => formatter.debug_tuple("Failed").field(message).finish(),
        }
    }
}

pub(in crate::tui::state) fn local_upload_preview_view(
    preview: &LocalUploadPreviewState,
) -> LocalUploadPreviewView<'_> {
    match &preview.state {
        LocalUploadPreviewStatus::Pending | LocalUploadPreviewStatus::Loading => {
            LocalUploadPreviewView::Loading {
                filename: preview.filename.clone(),
            }
        }
        LocalUploadPreviewStatus::Ready(protocol) => LocalUploadPreviewView::Ready { protocol },
        LocalUploadPreviewStatus::Failed(message) => LocalUploadPreviewView::Failed {
            filename: preview.filename.clone(),
            message: message.clone(),
        },
    }
}

pub(in crate::tui::state) fn local_upload_preview_candidate(
    attachment: &MessageAttachmentUpload,
) -> bool {
    let Some(extension) = attachment
        .filename
        .rsplit('.')
        .next()
        .filter(|extension| *extension != attachment.filename)
    else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
    )
}
