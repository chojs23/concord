#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD};

const MAX_NOTIFICATION_TEXT_BYTES: usize = 2_048;
#[cfg(target_os = "macos")]
static NEXT_NOTIFICATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NotificationProtocol {
    Osc9,
    Osc777,
    Osc99,
}

#[derive(Debug, Default)]
struct TerminalEnvironment<'a> {
    term_program: Option<&'a str>,
    kitty_window_id: bool,
    wezterm_pane: bool,
    iterm_session_id: bool,
    tmux: bool,
    screen: bool,
    zellij: bool,
}

/// Builds a desktop notification request for the terminal that owns the TUI.
///
/// Unsupported terminals return `None` so the caller can use the native macOS
/// fallback. Multiplexers that cannot reliably forward notifications also use
/// the fallback rather than silently swallowing the request.
#[cfg(target_os = "macos")]
pub(crate) fn sequence_for_current_terminal(title: &str, body: &str) -> Option<Vec<u8>> {
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let environment = TerminalEnvironment {
        term_program: term_program.as_deref(),
        kitty_window_id: std::env::var_os("KITTY_WINDOW_ID").is_some(),
        wezterm_pane: std::env::var_os("WEZTERM_PANE").is_some(),
        iterm_session_id: std::env::var_os("ITERM_SESSION_ID").is_some(),
        tmux: std::env::var_os("TMUX").is_some(),
        screen: std::env::var_os("STY").is_some(),
        zellij: std::env::var_os("ZELLIJ").is_some()
            || std::env::var_os("ZELLIJ_SESSION_NAME").is_some(),
    };
    let protocol = select_protocol(&environment)?;
    let notification_id = format!(
        "concord-{}-{}",
        std::process::id(),
        NEXT_NOTIFICATION_ID.fetch_add(1, Ordering::Relaxed)
    );
    Some(build_sequence(protocol, title, body, &notification_id))
}

fn select_protocol(environment: &TerminalEnvironment<'_>) -> Option<NotificationProtocol> {
    if environment.tmux || environment.screen || environment.zellij {
        return None;
    }
    if environment.kitty_window_id {
        return Some(NotificationProtocol::Osc99);
    }
    if environment.wezterm_pane {
        return Some(NotificationProtocol::Osc777);
    }
    if environment.iterm_session_id {
        return Some(NotificationProtocol::Osc9);
    }

    match environment
        .term_program
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("ghostty" | "wezterm" | "warp" | "warpterminal") => Some(NotificationProtocol::Osc777),
        Some("iterm.app" | "iterm2") => Some(NotificationProtocol::Osc9),
        Some("kitty") => Some(NotificationProtocol::Osc99),
        _ => None,
    }
}

fn build_sequence(
    protocol: NotificationProtocol,
    title: &str,
    body: &str,
    notification_id: &str,
) -> Vec<u8> {
    match protocol {
        NotificationProtocol::Osc9 => build_osc9_sequence(title, body),
        NotificationProtocol::Osc777 => build_osc777_sequence(title, body),
        NotificationProtocol::Osc99 => build_osc99_sequence(title, body, notification_id),
    }
}

fn build_osc9_sequence(title: &str, body: &str) -> Vec<u8> {
    let message = match (title.trim().is_empty(), body.trim().is_empty()) {
        (false, false) => format!("{title}: {body}"),
        (false, true) => title.to_owned(),
        (true, false) => body.to_owned(),
        (true, true) => "Concord".to_owned(),
    };
    format!("\x1b]9;{}\x1b\\", sanitize_text(&message)).into_bytes()
}

fn build_osc777_sequence(title: &str, body: &str) -> Vec<u8> {
    let title = sanitize_osc777_field(title);
    let title = if title.is_empty() {
        "Concord".to_owned()
    } else {
        title
    };
    let body = sanitize_osc777_field(body);
    format!("\x1b]777;notify;{title};{body}\x1b\\").into_bytes()
}

fn build_osc99_sequence(title: &str, body: &str, notification_id: &str) -> Vec<u8> {
    let title = sanitize_text(title);
    let title = if title.is_empty() {
        "Concord".to_owned()
    } else {
        title
    };
    let body = sanitize_text(body);
    let encoded_title = STANDARD.encode(title);
    let encoded_body = STANDARD.encode(body);

    format!(
        concat!(
            "\x1b]99;i={notification_id}:d=0:p=title:e=1;{encoded_title}\x1b\\",
            "\x1b]99;i={notification_id}:d=1:p=body:e=1;{encoded_body}\x1b\\",
        ),
        notification_id = notification_id,
        encoded_title = encoded_title,
        encoded_body = encoded_body
    )
    .into_bytes()
}

fn sanitize_osc777_field(value: &str) -> String {
    sanitize_text(value).replace(';', ",")
}

fn sanitize_text(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len().min(MAX_NOTIFICATION_TEXT_BYTES));
    let mut pending_space = false;

    for character in value.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }

        let added_bytes = character.len_utf8() + usize::from(pending_space);
        if sanitized.len() + added_bytes > MAX_NOTIFICATION_TEXT_BYTES {
            break;
        }
        if pending_space {
            sanitized.push(' ');
            pending_space = false;
        }
        sanitized.push(character);
    }

    sanitized
}

#[cfg(test)]
mod tests {
    use super::{NotificationProtocol, TerminalEnvironment, build_sequence, select_protocol};

    #[test]
    fn terminal_environment_selects_supported_protocols_and_native_fallbacks() {
        for (environment, expected) in [
            (
                TerminalEnvironment {
                    term_program: Some("ghostty"),
                    ..TerminalEnvironment::default()
                },
                Some(NotificationProtocol::Osc777),
            ),
            (
                TerminalEnvironment {
                    term_program: Some("WezTerm"),
                    ..TerminalEnvironment::default()
                },
                Some(NotificationProtocol::Osc777),
            ),
            (
                TerminalEnvironment {
                    term_program: Some("iTerm.app"),
                    ..TerminalEnvironment::default()
                },
                Some(NotificationProtocol::Osc9),
            ),
            (
                TerminalEnvironment {
                    term_program: Some("WarpTerminal"),
                    ..TerminalEnvironment::default()
                },
                Some(NotificationProtocol::Osc777),
            ),
            (
                TerminalEnvironment {
                    kitty_window_id: true,
                    ..TerminalEnvironment::default()
                },
                Some(NotificationProtocol::Osc99),
            ),
            (
                TerminalEnvironment {
                    wezterm_pane: true,
                    term_program: Some("tmux"),
                    tmux: true,
                    ..TerminalEnvironment::default()
                },
                None,
            ),
            (
                TerminalEnvironment {
                    iterm_session_id: true,
                    term_program: Some("tmux"),
                    tmux: true,
                    ..TerminalEnvironment::default()
                },
                None,
            ),
            (
                TerminalEnvironment {
                    term_program: Some("Apple_Terminal"),
                    ..TerminalEnvironment::default()
                },
                None,
            ),
            (
                TerminalEnvironment {
                    term_program: Some("FutureTerminal"),
                    ..TerminalEnvironment::default()
                },
                None,
            ),
            (
                TerminalEnvironment {
                    term_program: Some("ghostty"),
                    zellij: true,
                    ..TerminalEnvironment::default()
                },
                None,
            ),
            (
                TerminalEnvironment {
                    term_program: Some("ghostty"),
                    screen: true,
                    ..TerminalEnvironment::default()
                },
                None,
            ),
        ] {
            assert_eq!(select_protocol(&environment), expected, "{environment:?}");
        }
    }

    #[test]
    fn protocols_encode_safe_notification_text() {
        assert_eq!(
            build_sequence(
                NotificationProtocol::Osc9,
                "Concord\nAlert",
                "Hello\x1b world",
                "test-id",
            ),
            b"\x1b]9;Concord Alert: Hello world\x1b\\"
        );
        assert_eq!(
            build_sequence(
                NotificationProtocol::Osc777,
                "Concord;Alert",
                "Hello;\nworld",
                "test-id",
            ),
            b"\x1b]777;notify;Concord,Alert;Hello, world\x1b\\"
        );
        assert_eq!(
            build_sequence(NotificationProtocol::Osc99, "Concord", "Hello", "test-id",),
            concat!(
                "\x1b]99;i=test-id:d=0:p=title:e=1;Q29uY29yZA==\x1b\\",
                "\x1b]99;i=test-id:d=1:p=body:e=1;SGVsbG8=\x1b\\",
            )
            .as_bytes()
        );
    }
}
