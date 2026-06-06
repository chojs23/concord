use std::{env, fmt, io::stdout, path::PathBuf};

use crossterm::clipboard::CopyToClipboard;
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

use crate::discord::MessageAttachmentUpload;

const CLIPBOARD_IMAGE_FILENAME: &str = "clipboard-image.png";

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

pub(super) struct ClipboardPasteData {
    pub(super) file_attachments: Option<Vec<MessageAttachmentUpload>>,
    pub(super) text: Option<String>,
    pub(super) image_attachment: Option<MessageAttachmentUpload>,
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

    fn read_image_attachment() -> Result<MessageAttachmentUpload, ClipboardError> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| {
            ClipboardError::new(format!("native clipboard unavailable: {error}"))
        })?;
        let image = clipboard.get_image().map_err(|error| {
            ClipboardError::new(format!("native clipboard image read failed: {error}"))
        })?;
        let bytes = encode_image_as_png(image.width, image.height, image.bytes.as_ref())
            .map_err(ClipboardError::new)?;

        Ok(MessageAttachmentUpload::from_bytes(
            CLIPBOARD_IMAGE_FILENAME.to_owned(),
            bytes,
        ))
    }

    pub(super) fn read_paste_data_with_progress(
        on_attachment_processing: impl FnOnce(),
    ) -> Result<ClipboardPasteData, ClipboardError> {
        let file_attachments = Self::read_file_attachments().ok();
        if file_attachments.is_some() {
            return paste_data_from_parts(file_attachments, None, None);
        }

        let image_attachment = Self::read_image_attachment().ok();
        if image_attachment.is_some() {
            on_attachment_processing();
            return paste_data_from_parts(None, image_attachment, None);
        }

        let text = Self::read_text_once().ok().filter(|text| !text.is_empty());

        paste_data_from_parts(None, None, text)
    }

    fn read_file_attachments() -> Result<Vec<MessageAttachmentUpload>, ClipboardError> {
        let paths = clipboard_file_paths()?;
        let attachments: Vec<_> = paths
            .into_iter()
            .filter_map(|path| attachment_from_path(path).ok())
            .collect();
        if attachments.is_empty() {
            return Err(ClipboardError::new(
                "native clipboard has no file attachments",
            ));
        }
        Ok(attachments)
    }

    fn read_text_once() -> Result<String, ClipboardError> {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| {
            ClipboardError::new(format!("native clipboard unavailable: {error}"))
        })?;
        clipboard.get_text().map_err(|error| {
            ClipboardError::new(format!("native clipboard text read failed: {error}"))
        })
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

fn paste_data_from_parts(
    file_attachments: Option<Vec<MessageAttachmentUpload>>,
    image_attachment: Option<MessageAttachmentUpload>,
    text: Option<String>,
) -> Result<ClipboardPasteData, ClipboardError> {
    if let Some(file_attachments) = file_attachments {
        return Ok(ClipboardPasteData {
            file_attachments: Some(file_attachments),
            text: None,
            image_attachment: None,
        });
    }

    if let Some(image_attachment) = image_attachment {
        return Ok(ClipboardPasteData {
            file_attachments: None,
            text: None,
            image_attachment: Some(image_attachment),
        });
    }

    if let Some(text) = text {
        return Ok(ClipboardPasteData {
            file_attachments: None,
            text: Some(text),
            image_attachment: None,
        });
    }

    Err(ClipboardError::new(
        "native clipboard has no pasteable content",
    ))
}

impl ClipboardError {
    fn new(details: impl Into<String>) -> Self {
        Self {
            details: details.into(),
        }
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

fn attachment_from_path(path: PathBuf) -> Result<MessageAttachmentUpload, ClipboardError> {
    if !path.is_file() {
        return Err(ClipboardError::new(format!(
            "clipboard path is not a file: {}",
            path.display()
        )));
    }
    let path_display = path.display().to_string();
    MessageAttachmentUpload::from_existing_path(path).map_err(|error| {
        ClipboardError::new(format!(
            "stat clipboard file {} failed: {error}",
            path_display
        ))
    })
}

fn arboard_file_paths() -> Result<Vec<PathBuf>, ClipboardError> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|error| ClipboardError::new(format!("native clipboard unavailable: {error}")))?;
    clipboard.get().file_list().map_err(|error| {
        ClipboardError::new(format!("native clipboard file list read failed: {error}"))
    })
}

fn clipboard_file_paths() -> Result<Vec<PathBuf>, ClipboardError> {
    match arboard_file_paths() {
        Ok(paths) if !paths.is_empty() => Ok(paths),
        Ok(_) => fallback_clipboard_file_paths(),
        Err(error) => fallback_clipboard_file_paths()
            .map_err(|fallback_error| ClipboardError::new(format!("{error}; {fallback_error}"))),
    }
}

#[cfg(target_os = "macos")]
fn fallback_clipboard_file_paths() -> Result<Vec<PathBuf>, ClipboardError> {
    let output = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (the clipboard as \"furl\")"])
        .output()
        .map_err(|error| {
            ClipboardError::new(format!("macOS clipboard file read failed: {error}"))
        })?;
    if !output.status.success() {
        return Err(ClipboardError::new("macOS clipboard has no file URL"));
    }
    let text = String::from_utf8(output.stdout).map_err(|error| {
        ClipboardError::new(format!("macOS clipboard file URL is not UTF-8: {error}"))
    })?;
    let paths = macos_furl_output_paths(&text);
    if paths.is_empty() {
        return Err(ClipboardError::new("macOS clipboard file URL is empty"));
    }
    Ok(paths)
}

#[cfg(not(target_os = "macos"))]
fn fallback_clipboard_file_paths() -> Result<Vec<PathBuf>, ClipboardError> {
    Err(ClipboardError::new(
        "no platform clipboard file fallback is available",
    ))
}

#[cfg(target_os = "macos")]
fn macos_furl_output_paths(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn encode_image_as_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "clipboard image dimensions are too large".to_owned())?;
    if rgba.len() != expected_len {
        return Err(format!(
            "clipboard image has {} RGBA bytes, expected {expected_len}",
            rgba.len()
        ));
    }

    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(rgba, width as u32, height as u32, ExtendedColorType::Rgba8)
        .map_err(|error| format!("clipboard image PNG encode failed: {error}"))?;
    Ok(png)
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

#[cfg(test)]
mod tests {
    use crate::discord::MessageAttachmentUpload;

    use image::{GenericImageView, ImageFormat};

    use super::{
        CopyTextBackend, copy_text_backend_order, encode_image_as_png, paste_data_from_parts,
    };

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
    fn clipboard_paste_data_prefers_image_before_text() {
        let image = MessageAttachmentUpload::from_bytes("clipboard.png".to_owned(), vec![1, 2]);

        let data = paste_data_from_parts(None, Some(image.clone()), Some("plain text".to_owned()))
            .expect("image-backed clipboard data is pasteable");

        assert_eq!(data.image_attachment, Some(image));
        assert_eq!(data.text, None);
        assert_eq!(data.file_attachments, None);
    }

    #[test]
    fn clipboard_paste_data_prefers_file_list_before_image() {
        let file =
            MessageAttachmentUpload::from_path("/tmp/note.txt".into(), "note.txt".to_owned(), 1);
        let image = MessageAttachmentUpload::from_bytes("clipboard.png".to_owned(), vec![1, 2]);

        let data = paste_data_from_parts(Some(vec![file.clone()]), Some(image), None)
            .expect("file-backed clipboard data is pasteable");

        assert_eq!(data.file_attachments, Some(vec![file]));
        assert_eq!(data.image_attachment, None);
        assert_eq!(data.text, None);
    }

    #[test]
    fn encodes_rgba_clipboard_image_as_png() {
        let png =
            encode_image_as_png(1, 1, &[10, 20, 30, 255]).expect("valid RGBA pixels encode as PNG");

        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png)
            .expect("encoded clipboard image is a valid PNG");

        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.to_rgba8().as_raw(), &[10, 20, 30, 255]);
    }

    #[test]
    fn rejects_rgba_clipboard_image_with_wrong_byte_count() {
        let error = encode_image_as_png(2, 1, &[0, 0, 0, 255])
            .expect_err("invalid RGBA length must fail before encoding");

        assert!(error.contains("expected 8"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_macos_furl_paths() {
        let paths = super::macos_furl_output_paths("/Users/me/Pictures/cat.png\n");

        assert_eq!(
            paths,
            vec![std::path::PathBuf::from("/Users/me/Pictures/cat.png")]
        );
    }
}
