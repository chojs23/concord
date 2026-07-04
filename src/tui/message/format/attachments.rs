use crate::discord::{AttachmentInfo, AttachmentMediaType};

pub(in crate::tui) fn format_attachment_summary(attachments: &[AttachmentInfo]) -> String {
    format_attachment_summary_lines(attachments).join(" | ")
}

pub(super) fn format_attachment_summary_lines(attachments: &[AttachmentInfo]) -> Vec<String> {
    attachments.iter().map(format_attachment).collect()
}

fn format_attachment(attachment: &AttachmentInfo) -> String {
    let kind = attachment
        .media_type()
        .map_or("file", |media_type| match media_type {
            AttachmentMediaType::Image => "image",
            AttachmentMediaType::Video => "video",
            AttachmentMediaType::Audio => "audio",
        });

    let dimensions = match (attachment.width, attachment.height) {
        (Some(width), Some(height)) => format!(" {width}x{height}"),
        _ => String::new(),
    };

    format!("[{kind}: {}]{}", attachment.filename, dimensions)
}
