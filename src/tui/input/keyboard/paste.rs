use std::path::{Path, PathBuf};

use crate::discord::MessageAttachmentUpload;
use crate::tui::state::DashboardState;

pub fn handle_paste(state: &mut DashboardState, text: &str) -> bool {
    if handle_pasted_user_profile_avatar(state, text) {
        return true;
    }

    if state.is_user_profile_popup_editing() {
        let pasted: String = text.chars().filter(|value| *value != '\r').collect();
        if pasted.is_empty() {
            return false;
        }
        state.insert_user_profile_edit_text(&pasted);
        return true;
    }

    if state.is_forum_post_composer_active() {
        if state.is_forum_post_composer_editing() {
            if state.forum_post_composer_accepts_attachment_paste()
                && handle_pasted_file_attachments(state, text)
            {
                return true;
            }
            return state.insert_forum_post_text(text);
        }
        return false;
    }

    if state.is_thread_edit_title_editing() {
        return state.insert_thread_edit_text(text);
    }

    if !state.is_composing() {
        return false;
    }

    if handle_pasted_file_attachments(state, text) {
        return true;
    }

    let pasted: String = text.chars().filter(|value| *value != '\r').collect();
    if pasted.is_empty() {
        return false;
    }
    state.insert_composer_text_at_cursor(&pasted);
    true
}

pub fn handle_pasted_user_profile_avatar(state: &mut DashboardState, text: &str) -> bool {
    if !state.accepts_user_profile_avatar_paste() {
        return false;
    }
    let Some(mut attachments) = pasted_file_attachments(text) else {
        return false;
    };
    if attachments.is_empty() {
        return false;
    }
    let first = attachments.remove(0);
    state.set_user_profile_avatar_from_attachment(first)
}

pub fn handle_pasted_file_attachments(state: &mut DashboardState, text: &str) -> bool {
    if state.forum_post_composer_accepts_attachment_paste() {
        let Some(attachments) = pasted_file_attachments(text) else {
            return false;
        };
        state.add_pending_forum_post_attachments(attachments);
        return true;
    }

    if !state.is_composing() || !state.composer_accepts_attachments() {
        return false;
    }
    let Some(attachments) = pasted_file_attachments(text) else {
        return false;
    };
    state.add_pending_composer_attachments(attachments);
    true
}

fn pasted_file_attachments(text: &str) -> Option<Vec<MessageAttachmentUpload>> {
    let mut attachments = Vec::new();
    for line in meaningful_paste_lines(text) {
        let values = if let Some(path) = pasted_file_path(line).filter(|path| path.is_file()) {
            vec![path.to_string_lossy().into_owned()]
        } else {
            shell_path_words(line)?
        };
        for value in values {
            let path = pasted_file_path(&value)?;
            if !path.is_file() {
                return None;
            }
            attachments.push(MessageAttachmentUpload::from_existing_path(path).ok()?);
        }
    }
    (!attachments.is_empty()).then_some(attachments)
}

fn meaningful_paste_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| *line != "copy" && *line != "cut")
        .filter(|line| *line != "x-special/gnome-copied-files")
        .filter(|line| !line.starts_with('#'))
}

fn shell_path_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(value) = chars.next() {
        match value {
            '\\' if !in_single_quote => {
                current.push(chars.next()?);
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            value if value.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(value),
        }
    }

    if in_single_quote || in_double_quote {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

fn pasted_file_path(value: &str) -> Option<PathBuf> {
    if let Some(uri_path) = value.strip_prefix("file://") {
        return file_uri_path(uri_path);
    }

    let path = Path::new(value);
    path.is_absolute().then(|| path.to_path_buf())
}

fn file_uri_path(uri_path: &str) -> Option<PathBuf> {
    let path = uri_path.strip_prefix("localhost").unwrap_or(uri_path);
    if !path.starts_with('/') {
        return None;
    }
    percent_decode(path).map(PathBuf::from)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex_value(high)? * 16 + hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
