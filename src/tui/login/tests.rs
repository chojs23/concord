use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use crate::discord::password_auth::{MfaChallenge, MfaMethod};

use super::{
    render::render_mfa_code,
    state::{LoginScreen, LoginState},
    terminal_events::{LoginAction, handle_terminal},
};

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
