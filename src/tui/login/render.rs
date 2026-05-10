use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

fn render_app_header(frame: &mut Frame) {
    let area = frame.area();
    if area.height == 0 {
        return;
    }
    let header = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let title = format!(" Concord - v{} ", env!("CARGO_PKG_VERSION"));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Left),
        header,
    );
}

use crate::discord::password_auth::MfaMethod;

use super::{
    state::{LoginScreen, LoginState, PasswordField},
    terminal_events::mfa_supports,
};

pub(super) fn render(frame: &mut Frame, state: &LoginState) {
    match state.screen {
        LoginScreen::ModeSelect => render_mode_select(frame, state),
        LoginScreen::TokenInput => render_token_input(frame, state),
        LoginScreen::PasswordInput => render_password_input(frame, state),
        LoginScreen::MfaSelect => render_mfa_select(frame, state),
        LoginScreen::MfaCode => render_mfa_code(frame, state),
        LoginScreen::Qr => render_qr(frame, state),
    }
    render_app_header(frame);
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
        "Paste your token below. It will be used for this session.".to_owned()
    } else {
        format!(
            "Paste your token below. It will be saved to {}.",
            crate::token_store::credential_path_display()
        )
    };

    let mut lines = vec![
        Line::from(Span::styled("Token login", accent_style())),
        Line::from(""),
        Line::from(persistence_text),
        Line::from(""),
        Line::from(vec![
            Span::styled("> Token  ", dim_style()),
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
    let login_active = state.password.active_field == PasswordField::Login;
    let password_active = state.password.active_field == PasswordField::Password;
    let login_style = if login_active {
        active_style()
    } else {
        plain_input_style()
    };
    let password_style = if password_active {
        active_style()
    } else {
        plain_input_style()
    };
    let login_marker = if login_active { "> " } else { "  " };
    let password_marker = if password_active { "> " } else { "  " };

    let mut lines = vec![
        Line::from(Span::styled("Email/password login", accent_style())),
        Line::from(""),
        Line::from("Credentials are used only to request a Discord token."),
        Line::from("They are not saved. Captcha is not supported here."),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{login_marker}Email/phone  "), dim_style()),
            Span::styled(state.password.login.clone(), login_style),
        ]),
        Line::from(vec![
            Span::styled(format!("{password_marker}Password     "), dim_style()),
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

pub(super) fn render_mfa_code(frame: &mut Frame, state: &LoginState) {
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

fn mask_chars(value: &str) -> String {
    "•".repeat(value.chars().count())
}
