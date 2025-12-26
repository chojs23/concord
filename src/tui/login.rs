use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    AppError, Result,
    discord::qr_auth::{self, QrEvent},
};

use super::TerminalRestoreGuard;

pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    let mut terminal = ratatui::init();
    let _restore_guard = match TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };
    let mut state = LoginState::new(notice);
    let mut events = EventStream::new();
    let mut qr_handle: Option<QrHandle> = None;

    loop {
        terminal.draw(|frame| render(frame, &state))?;

        tokio::select! {
            terminal_event = events.next() => {
                let event = match terminal_event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => return Err(error.into()),
                    None => return Err(AppError::LoginCancelled),
                };
                match handle_terminal(&mut state, event) {
                    Some(LoginAction::Submit(token)) => return Ok(token),
                    Some(LoginAction::Cancel) => {
                        if let Some(handle) = qr_handle.take() {
                            handle.handle.abort();
                        }
                        return Err(AppError::LoginCancelled);
                    }
                    Some(LoginAction::StartQr) => {
                        let (tx, rx) = mpsc::channel(8);
                        let handle = qr_auth::spawn(tx);
                        qr_handle = Some(QrHandle { rx, handle });
                        state.qr.reset();
                        state.qr.status = "Starting QR login...".to_string();
                    }
                    Some(LoginAction::CancelQr) => {
                        if let Some(handle) = qr_handle.take() {
                            handle.handle.abort();
                        }
                        state.qr.reset();
                    }
                    None => {}
                }
            }
            qr_msg = async {
                if let Some(handle) = qr_handle.as_mut() {
                    handle.rx.recv().await
                } else {
                    std::future::pending::<Option<QrEvent>>().await
                }
            } => {
                let Some(message) = qr_msg else {
                    qr_handle = None;
                    state.screen = LoginScreen::ModeSelect;
                    state.error = Some("QR login channel closed unexpectedly.".to_string());
                    continue;
                };
                match message {
                    QrEvent::Status(status) => state.qr.status = status,
                    QrEvent::QrBitmap(bitmap) => state.qr.bitmap = Some(bitmap),
                    QrEvent::UserPending { username, discriminator } => {
                        let display = if discriminator == "0" {
                            username
                        } else {
                            format!("{username}#{discriminator}")
                        };
                        state.qr.pending_user = Some(display);
                    }
                    QrEvent::Token(token) => {
                        if let Some(handle) = qr_handle.take() {
                            let _ = handle.handle.await;
                        }
                        return Ok(token);
                    }
                    QrEvent::Cancelled => {
                        qr_handle = None;
                        state.screen = LoginScreen::ModeSelect;
                        state.error = Some("QR login was cancelled in the Discord mobile app.".to_string());
                    }
                    QrEvent::Failed(reason) => {
                        qr_handle = None;
                        state.screen = LoginScreen::ModeSelect;
                        state.error = Some(format!("QR login failed: {reason}"));
                    }
                }
            }
        }
    }
}

struct QrHandle {
    rx: mpsc::Receiver<QrEvent>,
    handle: JoinHandle<()>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LoginScreen {
    ModeSelect,
    TokenInput,
    Qr,
}

struct LoginState {
    screen: LoginScreen,
    notice: Option<String>,
    error: Option<String>,
    token_input: String,
    qr: QrViewState,
}

struct QrViewState {
    status: String,
    bitmap: Option<Vec<Vec<bool>>>,
    pending_user: Option<String>,
}

impl QrViewState {
    fn new() -> Self {
        Self {
            status: String::new(),
            bitmap: None,
            pending_user: None,
        }
    }

    fn reset(&mut self) {
        self.status.clear();
        self.bitmap = None;
        self.pending_user = None;
    }
}

impl LoginState {
    fn new(notice: Option<String>) -> Self {
        Self {
            screen: LoginScreen::ModeSelect,
            notice,
            error: None,
            token_input: String::new(),
            qr: QrViewState::new(),
        }
    }
}

enum LoginAction {
    Submit(String),
    Cancel,
    StartQr,
    CancelQr,
}

fn handle_terminal(state: &mut LoginState, event: TerminalEvent) -> Option<LoginAction> {
    let TerminalEvent::Key(key) = event else {
        return None;
    };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(LoginAction::Cancel);
    }

    match state.screen {
        LoginScreen::ModeSelect => match key.code {
            KeyCode::Char('t') | KeyCode::Char('T') => {
                state.screen = LoginScreen::TokenInput;
                state.error = None;
                None
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                state.screen = LoginScreen::Qr;
                state.error = None;
                Some(LoginAction::StartQr)
            }
            KeyCode::Esc => Some(LoginAction::Cancel),
            _ => None,
        },
        LoginScreen::TokenInput => match key.code {
            KeyCode::Enter => {
                let token = state.token_input.trim();
                if token.is_empty() {
                    state.error = Some("Token cannot be empty".to_string());
                    None
                } else {
                    Some(LoginAction::Submit(token.to_string()))
                }
            }
            KeyCode::Esc => {
                state.screen = LoginScreen::ModeSelect;
                state.token_input.clear();
                state.error = None;
                None
            }
            KeyCode::Backspace => {
                state.token_input.pop();
                state.error = None;
                None
            }
            KeyCode::Char(value) => {
                state.token_input.push(value);
                state.error = None;
                None
            }
            _ => None,
        },
        LoginScreen::Qr => match key.code {
            KeyCode::Esc => {
                state.screen = LoginScreen::ModeSelect;
                Some(LoginAction::CancelQr)
            }
            _ => None,
        },
    }
}

fn render(frame: &mut Frame, state: &LoginState) {
    match state.screen {
        LoginScreen::ModeSelect => render_mode_select(frame, state),
        LoginScreen::TokenInput => render_token_input(frame, state),
        LoginScreen::Qr => render_qr(frame, state),
    }
}

fn render_mode_select(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 18, frame.area());
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    let mut lines = vec![
        Line::from(Span::styled("Discord login", accent)),
        Line::from(""),
        Line::from("Choose how you want to log in:"),
        Line::from(""),
        Line::from(vec![
            Span::styled("[t] ", Style::default().fg(Color::Cyan)),
            Span::raw("Use Discord token (paste an existing token)"),
        ]),
        Line::from(vec![
            Span::styled("[q] ", Style::default().fg(Color::Cyan)),
            Span::raw("Login with QR code (scan with the mobile app)"),
        ]),
        Line::from(""),
    ];

    if let Some(notice) = &state.notice {
        lines.push(Line::from(Span::styled(
            notice.clone(),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
    }
    if let Some(error) = &state.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled("Esc cancel | Ctrl-C quit", dim)));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(login_block(" Login ")),
        area,
    );
}

fn render_token_input(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 14, frame.area());
    let masked = "•".repeat(state.token_input.chars().count());
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    let persistence_text = if state.notice.is_some() {
        "Paste your token below. It will be used for this session."
    } else {
        "Paste your token below. It will be saved to ~/.discord-rs/credential."
    };

    let mut lines = vec![
        Line::from(Span::styled("Token login", accent)),
        Line::from(""),
        Line::from(persistence_text),
        Line::from(""),
        Line::from(vec![
            Span::styled("Token  ", dim),
            Span::styled(masked, Style::default().fg(Color::Green)),
        ]),
    ];

    if let Some(error) = &state.error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(""));
    }

    if let Some(notice) = &state.notice {
        lines.push(Line::from(Span::styled(
            notice.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter save | Esc back | Ctrl-C quit",
        dim,
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(login_block(" Token ")),
        area,
    );
}

fn render_qr(frame: &mut Frame, state: &LoginState) {
    let area = frame.area();
    let dim = Style::default().fg(Color::DarkGray);
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut lines = vec![
        Line::from(Span::styled("Discord QR login", accent)),
        Line::from(""),
    ];

    if let Some(bitmap) = &state.qr.bitmap {
        for row_pair in bitmap.chunks(2) {
            let top = &row_pair[0];
            let bottom = row_pair.get(1);
            let mut line = String::with_capacity(top.len());
            for x in 0..top.len() {
                let upper = top[x];
                let lower = bottom.map(|row| row[x]).unwrap_or(false);
                let ch = match (upper, lower) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                };
                line.push(ch);
            }
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(Color::White),
            )));
        }
        lines.push(Line::from(""));
    }

    if !state.qr.status.is_empty() {
        lines.push(Line::from(Span::raw(state.qr.status.clone())));
    }
    if let Some(user) = &state.qr.pending_user {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Confirming login as {user}"),
            Style::default().fg(Color::Green),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Esc cancel | Ctrl-C quit", dim)));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(login_block(" QR Login ")),
        area,
    );
}

fn login_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::Cyan))
        .title_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);

    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    horizontal
}

#[cfg(test)]
mod tests {
    use super::LoginState;

    #[test]
    fn token_input_starts_empty() {
        let state = LoginState::new(None);
        assert!(state.token_input.is_empty());
    }
}
