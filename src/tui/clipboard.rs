use std::{env, fmt, io::Cursor, io::stdout, path::PathBuf};

use crate::discord::{MAX_UPLOAD_FILE_BYTES, MessageAttachmentUpload};
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

    pub(super) fn clipboard_file_uploads(
        &mut self,
    ) -> Result<Vec<MessageAttachmentUpload>, ClipboardError> {
        if is_remote_session() {
            return Err(ClipboardError {
                details: "native clipboard file paste is only available in local sessions"
                    .to_owned(),
            });
        }
        clipboard_file_paths()
            .and_then(file_uploads_from_paths)
            .map_err(|details| ClipboardError { details })
    }

    pub(super) fn clipboard_text(&mut self) -> Result<String, ClipboardError> {
        if is_remote_session() {
            return Err(ClipboardError {
                details: "native clipboard text paste is only available in local sessions"
                    .to_owned(),
            });
        }
        self.native_clipboard()
            .map_err(|details| ClipboardError { details })?
            .get_text()
            .map_err(|error| ClipboardError {
                details: format!("clipboard text unavailable: {error}"),
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

fn file_uploads_from_paths(paths: Vec<PathBuf>) -> Result<Vec<MessageAttachmentUpload>, String> {
    let mut uploads = Vec::new();
    for path in paths {
        if !path.is_file() {
            return Err(format!("clipboard path is not a file: {}", path.display()));
        }
        let metadata = path
            .metadata()
            .map_err(|error| format!("stat clipboard file {} failed: {error}", path.display()))?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("attachment")
            .to_owned();
        uploads.push(MessageAttachmentUpload::from_path(
            path,
            filename,
            metadata.len(),
        ));
    }

    if uploads.is_empty() {
        return Err("clipboard has no copied files".to_owned());
    }
    Ok(uploads)
}

#[cfg(target_os = "macos")]
fn clipboard_file_paths() -> Result<Vec<PathBuf>, String> {
    let script = r#"
use framework "AppKit"
property NSURL : a reference to current application's NSURL
property NSPasteboard : a reference to current application's NSPasteboard
property text item delimiters : linefeed

set pasteboard to NSPasteboard's generalPasteboard()
set urls to (pasteboard's readObjectsForClasses:[NSURL] options:[]) as list
set paths to {}
repeat with fileUrl in urls
    if (fileUrl's isFileURL()) as boolean then
        set end of paths to (fileUrl's |path|()) as text
    end if
end repeat
paths as text
"#;
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| format!("read macOS clipboard files failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "read macOS clipboard files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("read macOS clipboard files returned invalid UTF-8: {error}"))?;
    let paths = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("clipboard has no copied files".to_owned());
    }
    Ok(paths)
}

#[cfg(not(target_os = "macos"))]
fn clipboard_file_paths() -> Result<Vec<PathBuf>, String> {
    Err("native clipboard file paste is only implemented on macOS".to_owned())
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.details)
    }
}

impl ClipboardError {
    pub(super) fn is_empty_file_clipboard(&self) -> bool {
        self.details == "clipboard has no copied files"
            || self.details == "native clipboard file paste is only implemented on macOS"
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
    if encoded.get_ref().len() as u64 > MAX_UPLOAD_FILE_BYTES {
        return Err(format!(
            "clipboard image exceeds Discord's 10 MiB upload limit: {} bytes",
            encoded.get_ref().len()
        ));
    }
    Ok(MessageAttachmentUpload::from_bytes(
        "clipboard-image.png".to_owned(),
        encoded.into_inner(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        CopyTextBackend, copy_text_backend_order, file_uploads_from_paths, png_attachment_from_rgba,
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
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

    #[test]
    fn file_uploads_from_paths_builds_file_backed_attachments() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("concord-clipboard-{unique}"));
        fs::create_dir_all(&directory).expect("temp upload directory can be created");
        let path = directory.join("clip.pdf");
        fs::write(&path, b"pdf").expect("temp upload file can be written");

        let uploads =
            file_uploads_from_paths(vec![path.clone()]).expect("existing file path becomes upload");

        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].filename, "clip.pdf");
        assert_eq!(uploads[0].size_bytes, 3);
        assert_eq!(uploads[0].path(), Some(path.as_path()));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(directory);
    }
}
