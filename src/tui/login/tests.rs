use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use crate::discord::password_auth::{MfaChallenge, MfaMethod};

use super::{
    render::{render, render_mfa_code},
    state::{LoginScreen, LoginState},
    terminal_events::{LoginAction, handle_terminal},
};

fn press(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn paste(text: &str) -> TerminalEvent {
    TerminalEvent::Paste(text.to_owned())
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
fn login_choices_use_shortcut_theme() {
    let state = LoginState::new(None);
    let custom = crate::tui::theme::Theme::default().with_style(
        crate::tui::theme::HighlightGroup::Shortcut,
        Style::default().fg(Color::LightMagenta),
    );

    crate::tui::theme::with_test_theme(custom, || {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("test terminal should build");
        terminal
            .draw(|frame| render(frame, &state))
            .expect("login choices should render");

        let buffer = terminal.backend().buffer();
        let shortcut = (0..buffer.area.height)
            .flat_map(|row| {
                (0..buffer.area.width.saturating_sub(1)).map(move |column| (column, row))
            })
            .find(|&(column, row)| {
                buffer[(column, row)].symbol() == "[" && buffer[(column + 1, row)].symbol() == "t"
            })
            .expect("token shortcut should render");
        assert_eq!(buffer[shortcut].fg, Color::LightMagenta);
    });
}

#[test]
fn token_input_rejects_invalid_header_value() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::TokenInput;
    state.token_input = "bad\ntoken".to_owned();

    let action = handle_terminal(&mut state, press(KeyCode::Enter));

    assert!(action.is_none());
    assert!(state.error.as_deref().is_some_and(|error| {
        error.contains("Token is invalid") && error.contains("valid HTTP authorization header")
    }));
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
fn password_input_accepts_paste_and_renders_field_states() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::PasswordInput;
    state.password.active_field = super::state::PasswordField::Password;
    state.password.login = "테스트用户@example.com".to_owned();
    state.error = Some("old error".to_string());

    let action = handle_terminal(&mut state, paste("ab[]{};\\cd\n"));

    assert!(action.is_none());
    assert_eq!(state.password.password, "ab[]{};\\cd");
    assert!(state.error.is_none());

    let backend = TestBackend::new(100, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");

    terminal
        .draw(|frame| render(frame, &state))
        .expect("password form should render");

    let buffer = terminal.backend().buffer();
    let email_start = (0..buffer.area.height)
        .flat_map(|row| (0..buffer.area.width).map(move |column| (column, row)))
        .find(|position| buffer[*position].symbol() == "테")
        .expect("inactive CJK email should render");
    let mut column = email_start.0;
    for character in "테스트用户@example.com".chars() {
        let cell = &buffer[(column, email_start.1)];
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
        column = column.saturating_add(character.to_string().width() as u16);
    }

    let password_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|column| buffer[(column, *row)].symbol())
                .collect::<String>()
                .contains("> Password")
        })
        .expect("active password field should render");
    let marker_column = (0..buffer.area.width)
        .find(|column| buffer[(*column, password_row)].symbol() == ">")
        .expect("active password marker should render");
    let active_value = "•".repeat(state.password.password.chars().count());
    let active_width = format!("> Password     {active_value}").width() as u16;
    for column in marker_column..marker_column.saturating_add(active_width) {
        assert_eq!(
            buffer[(column, password_row)].fg,
            crate::tui::theme::current().foreground(crate::tui::theme::HighlightGroup::ActiveField)
        );
    }
}

#[test]
fn token_input_accepts_bracketed_paste_text() {
    let mut state = LoginState::new(None);
    state.screen = LoginScreen::TokenInput;

    handle_terminal(&mut state, paste("token-part-1\ntoken-part-2"));

    assert_eq!(state.token_input, "token-part-1token-part-2");
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
