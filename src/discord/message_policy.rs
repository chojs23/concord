use crate::discord::{MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload, MessageSendLimits};
use crate::{AppError, Result};

/// Validate the stable parts of an outbound message before a transport builds
/// its request. UI callers use this for immediate feedback and REST callers
/// repeat it as a final safety boundary.
pub(crate) fn validate_message_payload(
    content: &str,
    attachments: &[MessageAttachmentUpload],
    limits: MessageSendLimits,
) -> Result<()> {
    if content.trim().is_empty() && attachments.is_empty() {
        return Err(AppError::EmptyMessageContent);
    }

    validate_message_content_length(content, limits.max_content_chars)?;
    validate_attachment_sizes(
        attachments.len(),
        attachments
            .iter()
            .map(|attachment| (attachment.filename.as_str(), attachment.size_bytes)),
        limits.max_attachment_bytes,
    )
}

pub(crate) fn validate_message_content(content: &str, max_content_chars: usize) -> Result<()> {
    if content.trim().is_empty() {
        return Err(AppError::EmptyMessageContent);
    }
    validate_message_content_length(content, max_content_chars)
}

pub(crate) fn validate_message_content_length(
    content: &str,
    max_content_chars: usize,
) -> Result<()> {
    let len = content.chars().count();
    if len > max_content_chars {
        return Err(AppError::MessageTooLong {
            len,
            limit: max_content_chars,
        });
    }
    Ok(())
}

/// Discord applies its upload limit to each file rather than the sum. Accept
/// metadata instead of upload objects so generated and re-statted files can be
/// checked without cloning their bytes.
pub(crate) fn validate_attachment_sizes<'a>(
    attachment_count: usize,
    attachments: impl IntoIterator<Item = (&'a str, u64)>,
    upload_limit: u64,
) -> Result<()> {
    if attachment_count > MAX_UPLOAD_ATTACHMENT_COUNT {
        return Err(AppError::TooManyAttachments {
            count: attachment_count,
        });
    }

    for (filename, size) in attachments {
        if size > upload_limit {
            return Err(AppError::AttachmentTooLarge {
                filename: filename.to_owned(),
                size,
                limit: upload_limit,
            });
        }
    }

    Ok(())
}
