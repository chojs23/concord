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
    discord::{
        password_auth::{self, MfaChallenge, MfaMethod, PasswordAuthEvent},
        qr_auth::{self, QrEvent},
    },
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
    let mut password_handle: Option<PasswordAuthHandle> = None;

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
                        if let Some(handle) = password_handle.take() {
                            handle.handle.abort();
                        }
                        return Err(AppError::LoginCancelled);
                    }
                    Some(LoginAction::StartPasswordLogin { login, password }) => {
                        if let Some(handle) = password_handle.take() {
                            handle.handle.abort();
                        }
                        let (tx, rx) = mpsc::channel(8);
                        let handle = password_auth::spawn_login(login, password, tx);
                        password_handle = Some(PasswordAuthHandle { rx, handle });
                        state.password.in_progress = true;
                        state.password.status = "Authenticating with Discord...".to_string();
                        state.error = None;
                    }
                    Some(LoginAction::StartMfaVerify { method, code, ticket, login_instance_id }) => {
                        if let Some(handle) = password_handle.take() {
                            handle.handle.abort();
                        }
                        let (tx, rx) = mpsc::channel(8);
                        let handle = password_auth::spawn_mfa_verify(method, code, ticket, login_instance_id, tx);
                        password_handle = Some(PasswordAuthHandle { rx, handle });
                        state.password.in_progress = true;
                        state.password.status = "Verifying multi-factor authentication...".to_string();
                        state.error = None;
                    }
                    Some(LoginAction::SendMfaSms { ticket }) => {
                        if let Some(handle) = password_handle.take() {
                            handle.handle.abort();
                        }
                        let (tx, rx) = mpsc::channel(8);
                        let handle = password_auth::spawn_sms_send(ticket, tx);
                        password_handle = Some(PasswordAuthHandle { rx, handle });
                        state.password.in_progress = true;
                        state.password.status = "Requesting SMS code from Discord...".to_string();
                        state.error = None;
                    }
                    Some(LoginAction::CancelPasswordLogin) => {
                        if let Some(handle) = password_handle.take() {
                            handle.handle.abort();
                        }
                        state.password.reset_sensitive();
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
            password_msg = async {
                if let Some(handle) = password_handle.as_mut() {
                    handle.rx.recv().await
                } else {
                    std::future::pending::<Option<PasswordAuthEvent>>().await
                }
            } => {
                let Some(message) = password_msg else {
                    password_handle = None;
                    state.password.in_progress = false;
                    state.error = Some("Password login channel closed unexpectedly.".to_string());
                    continue;
                };
                match message {
                    PasswordAuthEvent::Status(status) => {
                        state.password.in_progress = true;
                        state.password.status = status;
                    }
                    PasswordAuthEvent::MfaRequired(challenge) => {
                        password_handle = None;
                        state.password.in_progress = false;
                        state.password.password.clear();
                        state.password.mfa = Some(challenge);
                        state.password.mfa_method = None;
                        state.password.mfa_code.clear();
                        state.password.status = "Choose a multi-factor authentication method.".to_string();
                        state.screen = LoginScreen::MfaSelect;
                    }
                    PasswordAuthEvent::SmsSent { phone } => {
                        password_handle = None;
                        state.password.in_progress = false;
                        state.password.mfa_method = Some(MfaMethod::Sms);
                        state.password.mfa_code.clear();
                        state.password.status = match phone {
                            Some(phone) => format!("SMS sent to {phone}. Enter the code below."),
                            None => "SMS sent. Enter the code below.".to_string(),
                        };
                        state.screen = LoginScreen::MfaCode;
                    }
                    PasswordAuthEvent::Token(token) => {
                        if let Some(handle) = password_handle.take() {
                            let _ = handle.handle.await;
                        }
                        state.password.reset_sensitive();
                        return Ok(token);
                    }
                    PasswordAuthEvent::Failed(reason) => {
                        password_handle = None;
                        state.password.in_progress = false;
                        state.password.status.clear();
                        state.error = Some(format!("Password login failed: {reason}"));
                    }
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

struct PasswordAuthHandle {
    rx: mpsc::Receiver<PasswordAuthEvent>,
    handle: JoinHandle<()>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LoginScreen {
    ModeSelect,
    TokenInput,
    PasswordInput,
    MfaSelect,
    MfaCode,
    Qr,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PasswordField {
    Login,
    Password,
}

struct LoginState {
    screen: LoginScreen,
    notice: Option<String>,
    error: Option<String>,
    token_input: String,
    password: PasswordViewState,
    qr: QrViewState,
}

struct PasswordViewState {
    login: String,
    password: String,
    active_field: PasswordField,
    status: String,
    mfa: Option<MfaChallenge>,
    mfa_method: Option<MfaMethod>,
    mfa_code: String,
    in_progress: bool,
}

impl PasswordViewState {
    fn new() -> Self {
        Self {
            login: String::new(),
            password: String::new(),
            active_field: PasswordField::Login,
            status: String::new(),
            mfa: None,
            mfa_method: None,
            mfa_code: String::new(),
            in_progress: false,
        }
    }

    fn reset_sensitive(&mut self) {
        self.password.clear();
        self.mfa = None;
        self.mfa_method = None;
        self.mfa_code.clear();
        self.status.clear();
        self.in_progress = false;
    }
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
            password: PasswordViewState::new(),
            qr: QrViewState::new(),
        }
    }
}

enum LoginAction {
    Submit(String),
    Cancel,
    StartPasswordLogin {
        login: String,
        password: String,
    },
    StartMfaVerify {
        method: MfaMethod,
        code: String,
        ticket: String,
        login_instance_id: String,
    },
    SendMfaSms {
        ticket: String,
    },
    CancelPasswordLogin,
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
            KeyCode::Char('e') | KeyCode::Char('E') => {
                state.screen = LoginScreen::PasswordInput;
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
        LoginScreen::PasswordInput => {
            if state.password.in_progress {
                return match key.code {
                    KeyCode::Esc => {
                        state.screen = LoginScreen::ModeSelect;
                        Some(LoginAction::CancelPasswordLogin)
                    }
                    _ => None,
                };
            }
            match key.code {
                KeyCode::Enter => {
                    let login = state.password.login.trim().to_string();
                    let password = state.password.password.clone();
                    if login.is_empty() || password.is_empty() {
                        state.error = Some("Email/phone and password are required".to_string());
                        None
                    } else {
                        state.password.password.clear();
                        Some(LoginAction::StartPasswordLogin { login, password })
                    }
                }
                KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                    state.password.active_field = match state.password.active_field {
                        PasswordField::Login => PasswordField::Password,
                        PasswordField::Password => PasswordField::Login,
                    };
                    state.error = None;
                    None
                }
                KeyCode::Esc => {
                    state.screen = LoginScreen::ModeSelect;
                    state.error = None;
                    Some(LoginAction::CancelPasswordLogin)
                }
                KeyCode::Backspace => {
                    active_password_input(state).pop();
                    state.error = None;
                    None
                }
                KeyCode::Char(value) => {
                    active_password_input(state).push(value);
                    state.error = None;
                    None
                }
                _ => None,
            }
        }
        LoginScreen::MfaSelect => {
            if state.password.in_progress {
                return match key.code {
                    KeyCode::Esc => {
                        state.screen = LoginScreen::PasswordInput;
                        Some(LoginAction::CancelPasswordLogin)
                    }
                    _ => None,
                };
            }
            match key.code {
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    if mfa_supports(&state.password.mfa, MfaMethod::Totp) {
                        state.password.mfa_method = Some(MfaMethod::Totp);
                        state.password.mfa_code.clear();
                        state.password.status =
                            "Enter the TOTP code from your authenticator app.".to_string();
                        state.screen = LoginScreen::MfaCode;
                    }
                    None
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    if mfa_supports(&state.password.mfa, MfaMethod::Sms)
                        && let Some(challenge) = &state.password.mfa
                    {
                        return Some(LoginAction::SendMfaSms {
                            ticket: challenge.ticket.clone(),
                        });
                    }
                    None
                }
                KeyCode::Esc => {
                    state.screen = LoginScreen::PasswordInput;
                    state.password.reset_sensitive();
                    state.error = None;
                    None
                }
                _ => None,
            }
        }
        LoginScreen::MfaCode => {
            if state.password.in_progress {
                return match key.code {
                    KeyCode::Esc => {
                        state.screen = LoginScreen::PasswordInput;
                        state.error = None;
                        Some(LoginAction::CancelPasswordLogin)
                    }
                    _ => None,
                };
            }
            match key.code {
                KeyCode::Enter => {
                    let code = state.password.mfa_code.trim().to_string();
                    if code.is_empty() {
                        state.error = Some("MFA code cannot be empty".to_string());
                        return None;
                    }
                    let Some(challenge) = &state.password.mfa else {
                        state.error =
                            Some("MFA challenge is missing; restart password login".to_string());
                        return None;
                    };
                    let Some(method) = state.password.mfa_method else {
                        state.error =
                            Some("MFA method is missing; choose a method first".to_string());
                        return None;
                    };
                    state.password.mfa_code.clear();
                    Some(LoginAction::StartMfaVerify {
                        method,
                        code,
                        ticket: challenge.ticket.clone(),
                        login_instance_id: challenge.login_instance_id.clone(),
                    })
                }
                KeyCode::Esc => {
                    state.screen = LoginScreen::MfaSelect;
                    state.password.mfa_code.clear();
                    state.error = None;
                    None
                }
                KeyCode::Backspace => {
                    state.password.mfa_code.pop();
                    state.error = None;
                    None
                }
                KeyCode::Char(value) => {
                    state.password.mfa_code.push(value);
                    state.error = None;
                    None
                }
                _ => None,
            }
        }
        LoginScreen::Qr => match key.code {
            KeyCode::Esc => {
                state.screen = LoginScreen::ModeSelect;
                Some(LoginAction::CancelQr)
            }
            _ => None,
        },
    }
}

fn active_password_input(state: &mut LoginState) -> &mut String {
    match state.password.active_field {
        PasswordField::Login => &mut state.password.login,
        PasswordField::Password => &mut state.password.password,
    }
}

fn mfa_supports(challenge: &Option<MfaChallenge>, method: MfaMethod) -> bool {
    challenge
        .as_ref()
        .is_some_and(|challenge| challenge.methods.contains(&method))
}

fn mask_chars(value: &str) -> String {
    "•".repeat(value.chars().count())
}

fn render(frame: &mut Frame, state: &LoginState) {
    match state.screen {
        LoginScreen::ModeSelect => render_mode_select(frame, state),
        LoginScreen::TokenInput => render_token_input(frame, state),
        LoginScreen::PasswordInput => render_password_input(frame, state),
        LoginScreen::MfaSelect => render_mfa_select(frame, state),
        LoginScreen::MfaCode => render_mfa_code(frame, state),
        LoginScreen::Qr => render_qr(frame, state),
    }
}

fn render_mode_select(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 18, frame.area());

    let mut lines = vec![
        Line::from(Span::styled("Discord login", accent_style())),
        Line::from(""),
        Line::from("Choose how you want to log in:"),
        Line::from(""),
        choice_line("[t] ", "Use Discord token (paste an existing token)"),
        choice_line("[e] ", "Login with email/phone and password"),
        choice_line("[q] ", "Login with QR code (scan with the mobile app)"),
        Line::from(""),
    ];

    if let Some(notice) = &state.notice {
        lines.push(notice_line(notice));
        lines.push(Line::from(""));
    }
    if let Some(error) = &state.error {
        lines.push(error_line(error));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "Esc cancel | Ctrl-C quit",
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " Login ", lines);
}

fn render_token_input(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 14, frame.area());
    let masked = mask_chars(&state.token_input);

    let persistence_text = if state.notice.is_some() {
        "Paste your token below. It will be used for this session."
    } else {
        "Paste your token below. It will be saved to ~/.concord/credential."
    };

    let mut lines = vec![
        Line::from(Span::styled("Token login", accent_style())),
        Line::from(""),
        Line::from(persistence_text),
        Line::from(""),
        Line::from(vec![
            Span::styled("Token  ", dim_style()),
            Span::styled(masked, Style::default().fg(Color::Green)),
        ]),
    ];

    if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(""));
    }

    if let Some(notice) = &state.notice {
        lines.push(notice_line(notice));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter save | Esc back | Ctrl-C quit",
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " Token ", lines);
}

fn render_password_input(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(82, 18, frame.area());
    let password_mask = mask_chars(&state.password.password);
    let login_style = if state.password.active_field == PasswordField::Login {
        active_style()
    } else {
        plain_input_style()
    };
    let password_style = if state.password.active_field == PasswordField::Password {
        active_style()
    } else {
        plain_input_style()
    };

    let mut lines = vec![
        Line::from(Span::styled("Email/password login", accent_style())),
        Line::from(""),
        Line::from("Credentials are used only to request a Discord token."),
        Line::from("They are not saved. Captcha is not supported here."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Email/phone  ", dim_style()),
            Span::styled(state.password.login.clone(), login_style),
        ]),
        Line::from(vec![
            Span::styled("Password     ", dim_style()),
            Span::styled(password_mask, password_style),
        ]),
        Line::from(""),
    ];

    if state.password.in_progress && !state.password.status.is_empty() {
        lines.push(status_line(&state.password.status));
    } else if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab switch field | Enter login | Esc back | Ctrl-C quit",
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " Email Login ", lines);
}

fn render_mfa_select(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(82, 16, frame.area());
    let mut lines = vec![
        Line::from(Span::styled("Multi-factor authentication", accent_style())),
        Line::from(""),
        Line::from("Discord requires another verification step."),
        Line::from(""),
    ];

    if mfa_supports(&state.password.mfa, MfaMethod::Totp) {
        lines.push(choice_line("[t] ", "Use TOTP authenticator code"));
    }
    if mfa_supports(&state.password.mfa, MfaMethod::Sms) {
        lines.push(choice_line("[s] ", "Send SMS verification code"));
    }
    lines.push(Line::from(""));

    if state.password.in_progress && !state.password.status.is_empty() {
        lines.push(status_line(&state.password.status));
    } else if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else if !state.password.status.is_empty() {
        lines.push(Line::from(Span::raw(state.password.status.clone())));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Esc back | Ctrl-C quit",
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " MFA ", lines);
}

fn render_mfa_code(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(82, 15, frame.area());
    let method = match state.password.mfa_method {
        Some(MfaMethod::Totp) => "TOTP code",
        Some(MfaMethod::Sms) => "SMS code",
        None => "MFA code",
    };
    let mut lines = vec![
        Line::from(Span::styled("Multi-factor authentication", accent_style())),
        Line::from(""),
        Line::from(state.password.status.clone()),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{method}  "), dim_style()),
            Span::styled(
                mask_chars(&state.password.mfa_code),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(""),
    ];

    if let Some(error) = &state.error {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter verify | Esc choose method | Ctrl-C quit",
        dim_style(),
    )));

    render_wrapped_login_panel(frame, area, " MFA Code ", lines);
}

fn render_qr(frame: &mut Frame, state: &LoginState) {
    let area = frame.area();

    let mut lines = vec![
        Line::from(Span::styled("Discord QR login", accent_style())),
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
    lines.push(Line::from(Span::styled(
        "Esc cancel | Ctrl-C quit",
        dim_style(),
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(login_block(" QR Login ")),
        area,
    );
}

fn render_wrapped_login_panel(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(login_block(title)),
        area,
    );
}

fn accent_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn active_style() -> Style {
    Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD)
}

fn plain_input_style() -> Style {
    Style::default().fg(Color::White)
}

fn error_line(value: impl AsRef<str>) -> Line<'static> {
    Line::from(Span::styled(
        value.as_ref().to_owned(),
        Style::default().fg(Color::Red),
    ))
}

fn notice_line(value: impl AsRef<str>) -> Line<'static> {
    Line::from(Span::styled(
        value.as_ref().to_owned(),
        Style::default().fg(Color::Yellow),
    ))
}

fn status_line(value: impl AsRef<str>) -> Line<'static> {
    notice_line(value)
}

fn choice_line(key: &'static str, text: &'static str) -> Line<'static> {
    Line::from(vec![Span::styled(key, key_style()), Span::raw(text)])
}

fn key_style() -> Style {
    Style::default().fg(Color::Cyan)
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
    use crossterm::event::{Event as TerminalEvent, KeyEvent};
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn press(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn mfa_challenge(methods: Vec<MfaMethod>) -> MfaChallenge {
        MfaChallenge {
            ticket: "ticket".to_string(),
            login_instance_id: "login-instance".to_string(),
            methods,
        }
    }

    #[test]
    fn token_input_starts_empty() {
        let state = LoginState::new(None);
        assert!(state.token_input.is_empty());
    }

    #[test]
    fn password_submit_starts_login_and_clears_password_field() {
        let mut state = LoginState::new(None);
        state.screen = LoginScreen::PasswordInput;
        state.password.login = "  user@example.com  ".to_string();
        state.password.password = "password".to_string();

        let action = handle_terminal(&mut state, press(KeyCode::Enter));

        assert!(matches!(
            action,
            Some(LoginAction::StartPasswordLogin { login, password })
                if login == "user@example.com" && password == "password"
        ));
        assert!(state.password.password.is_empty());
    }

    #[test]
    fn mfa_code_submit_starts_verify_and_clears_code_field() {
        let mut state = LoginState::new(None);
        state.screen = LoginScreen::MfaCode;
        state.password.mfa = Some(mfa_challenge(vec![MfaMethod::Totp]));
        state.password.mfa_method = Some(MfaMethod::Totp);
        state.password.mfa_code = " 123456 ".to_string();

        let action = handle_terminal(&mut state, press(KeyCode::Enter));

        assert!(matches!(
            action,
            Some(LoginAction::StartMfaVerify { method, code, ticket, login_instance_id })
                if method == MfaMethod::Totp
                    && code == "123456"
                    && ticket == "ticket"
                    && login_instance_id == "login-instance"
        ));
        assert!(state.password.mfa_code.is_empty());
    }

    #[test]
    fn mfa_code_esc_while_verifying_returns_to_valid_password_screen() {
        let mut state = LoginState::new(None);
        state.screen = LoginScreen::MfaCode;
        state.error = Some("old error".to_string());
        state.password.in_progress = true;
        state.password.status = "Verifying multi-factor authentication...".to_string();
        state.password.mfa = Some(mfa_challenge(vec![MfaMethod::Totp]));
        state.password.mfa_method = Some(MfaMethod::Totp);
        state.password.mfa_code = "123456".to_string();

        let action = handle_terminal(&mut state, press(KeyCode::Esc));

        assert!(matches!(action, Some(LoginAction::CancelPasswordLogin)));
        assert!(state.screen == LoginScreen::PasswordInput);
        assert!(state.error.is_none());

        state.password.reset_sensitive();
        assert!(state.screen == LoginScreen::PasswordInput);
        assert!(state.password.mfa.is_none());
        assert!(state.password.mfa_method.is_none());
        assert!(state.password.mfa_code.is_empty());
        assert!(!state.password.in_progress);
    }

    #[test]
    fn mfa_code_render_masks_entered_code() {
        let backend = TestBackend::new(82, 15);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        let mut state = LoginState::new(None);
        state.screen = LoginScreen::MfaCode;
        state.password.status = "Enter MFA code".to_string();
        state.password.mfa_method = Some(MfaMethod::Totp);
        state.password.mfa_code = "123456".to_string();

        terminal
            .draw(|frame| render_mfa_code(frame, &state))
            .expect("render should succeed");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(!rendered.contains("123456"));
        assert!(rendered.contains("••••••"));
    }
}
