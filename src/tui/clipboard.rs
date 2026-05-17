use std::{env, fmt, io::Cursor, io::stdout};

use crate::discord::MessageAttachmentUpload;
use crossterm::clipboard::CopyToClipboard;

#[derive(Default)]
pub(super) struct ClipboardService {
    native: Option<arboard::Clipboard>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CopyTextBackend {
    Native,
    Osc52,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ClipboardError {
    details: String,
}

impl ClipboardService {
    pub(super) fn copy_text(&mut self, content: &str) -> Result<CopyTextBackend, ClipboardError> {
        let mut failures = Vec::new();
        for backend in copy_text_backend_order(is_remote_session()) {
            let result = match backend {
                CopyTextBackend::Native => self.copy_text_native(content),
                CopyTextBackend::Osc52 => copy_text_osc52(content),
            };
            match result {
                Ok(()) => return Ok(backend),
                Err(error) => failures.push(error),
            }
        }

        Err(ClipboardError {
            details: failures.join("; "),
        })
    }

    fn copy_text_native(&mut self, content: &str) -> Result<(), String> {
        let clipboard = self.native_clipboard()?;
        if let Err(error) = clipboard.set_text(content) {
            self.native = None;
            return Err(format!("native clipboard write failed: {error}"));
        }
        Ok(())
    }

    pub(super) fn clipboard_image_upload(
        &mut self,
    ) -> Result<MessageAttachmentUpload, ClipboardError> {
        if is_remote_session() {
            return Err(ClipboardError {
                details: "clipboard image upload is only available in local sessions".to_owned(),
            });
        }
        let image = self
            .native_clipboard()
            .map_err(|details| ClipboardError { details })?
            .get_image()
            .map_err(|error| ClipboardError {
                details: format!("clipboard image unavailable: {error}"),
            })?;
        png_attachment_from_rgba(image.width, image.height, image.bytes.into_owned())
            .map_err(|details| ClipboardError { details })
    }

    fn native_clipboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.native.is_none() {
            self.native = Some(
                arboard::Clipboard::new()
                    .map_err(|error| format!("native clipboard unavailable: {error}"))?,
            );
        }

        Ok(self
            .native
            .as_mut()
            .expect("native clipboard was initialized above"))
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.details)
    }
}

fn copy_text_osc52(content: &str) -> Result<(), String> {
    crossterm::execute!(stdout(), CopyToClipboard::to_clipboard_from(content))
        .map_err(|error| format!("OSC52 clipboard write failed: {error}"))
}

fn copy_text_backend_order(remote_session: bool) -> [CopyTextBackend; 2] {
    if remote_session {
        [CopyTextBackend::Osc52, CopyTextBackend::Native]
    } else {
        [CopyTextBackend::Native, CopyTextBackend::Osc52]
    }
}

fn is_remote_session() -> bool {
    env::var_os("SSH_CONNECTION").is_some() || env::var_os("SSH_TTY").is_some()
}

fn png_attachment_from_rgba(
    width: usize,
    height: usize,
    bytes: Vec<u8>,
) -> Result<MessageAttachmentUpload, String> {
    let Some(image) = image::RgbaImage::from_raw(width as u32, height as u32, bytes) else {
        return Err("clipboard image dimensions do not match pixel data".to_owned());
    };
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("encode clipboard image failed: {error}"))?;
    Ok(MessageAttachmentUpload::from_bytes(
        "clipboard-image.png".to_owned(),
        encoded.into_inner(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{CopyTextBackend, copy_text_backend_order, png_attachment_from_rgba};

    #[test]
    fn local_sessions_try_native_clipboard_before_osc52() {
        assert_eq!(
            copy_text_backend_order(false),
            [CopyTextBackend::Native, CopyTextBackend::Osc52]
        );
    }

    #[test]
    fn remote_sessions_try_osc52_before_native_clipboard() {
        assert_eq!(
            copy_text_backend_order(true),
            [CopyTextBackend::Osc52, CopyTextBackend::Native]
        );
    }

    #[test]
    fn clipboard_image_upload_encodes_rgba_pixels_as_png_attachment() {
        let upload = png_attachment_from_rgba(1, 1, vec![255, 0, 0, 255])
            .expect("one RGBA pixel can be encoded as PNG");

        assert_eq!(upload.filename, "clipboard-image.png");
        assert_eq!(
            upload.size_bytes,
            upload.bytes().expect("upload is memory backed").len() as u64
        );
        image::load_from_memory(upload.bytes().expect("upload is memory backed"))
            .expect("clipboard image upload contains valid PNG bytes");
    }
}
