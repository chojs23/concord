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
                filename: &preview.filename,
            }
        }
        LocalUploadPreviewStatus::Ready(protocol) => LocalUploadPreviewView::Ready { protocol },
        LocalUploadPreviewStatus::Failed(message) => LocalUploadPreviewView::Failed {
            filename: &preview.filename,
            message,
        },
    }
}

pub(in crate::tui::state) fn sync_local_upload_previews(
    attachments: &[MessageAttachmentUpload],
    previews: &mut Vec<LocalUploadPreviewState>,
    generation: &mut u64,
    enabled: bool,
) {
    if !enabled {
        previews.clear();
        return;
    }

    let mut previous = std::mem::take(previews);
    *previews = attachments
        .iter()
        .enumerate()
        .filter(|(_, attachment)| local_upload_preview_candidate(attachment))
        .map(|(attachment_index, attachment)| {
            if let Some(previous_index) = previous.iter().position(|preview| {
                preview.attachment_index == attachment_index
                    && preview.filename == attachment.filename
            }) {
                return previous.remove(previous_index);
            }
            *generation = generation.saturating_add(1);
            LocalUploadPreviewState {
                attachment_index,
                generation: *generation,
                filename: attachment.filename.clone(),
                state: LocalUploadPreviewStatus::Pending,
            }
        })
        .collect();
}

pub(in crate::tui::state) fn take_pending_local_upload_preview(
    previews: &mut [LocalUploadPreviewState],
    attachments: &[MessageAttachmentUpload],
) -> Option<(usize, u64, String, MessageAttachmentUpload)> {
    let preview = previews
        .iter_mut()
        .find(|preview| matches!(preview.state, LocalUploadPreviewStatus::Pending))?;
    let attachment = attachments.get(preview.attachment_index)?.clone();
    preview.state = LocalUploadPreviewStatus::Loading;
    Some((
        preview.attachment_index,
        preview.generation,
        preview.filename.clone(),
        attachment,
    ))
}

pub(in crate::tui::state) fn store_local_upload_preview_result(
    previews: &mut [LocalUploadPreviewState],
    attachment_index: usize,
    generation: u64,
    filename: String,
    result: Result<Protocol, String>,
) {
    let Some(preview) = previews.iter_mut().find(|preview| {
        preview.attachment_index == attachment_index && preview.generation == generation
    }) else {
        return;
    };
    preview.filename = filename;
    preview.state = match result {
        Ok(protocol) => LocalUploadPreviewStatus::Ready(protocol),
        Err(message) => LocalUploadPreviewStatus::Failed(message),
    };
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
