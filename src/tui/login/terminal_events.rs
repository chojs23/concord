use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers};

use crate::discord::{
    password_auth::{MfaChallenge, MfaMethod},
    validate_token_header,
};

use super::state::{LoginScreen, LoginState, PasswordField};

pub(super) enum LoginAction {
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

pub(super) fn handle_terminal(state: &mut LoginState, event: TerminalEvent) -> Option<LoginAction> {
    let key = match event {
        TerminalEvent::Key(key) => key,
        TerminalEvent::Paste(text) => return handle_paste(state, &text),
        _ => return None,
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
                } else if let Err(error) = validate_token_header(token) {
                    state.error = Some(format!("Token is invalid: {error}"));
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

fn handle_paste(state: &mut LoginState, text: &str) -> Option<LoginAction> {
    match state.screen {
        LoginScreen::TokenInput => {
            append_paste(&mut state.token_input, text);
            state.error = None;
        }
        LoginScreen::PasswordInput if !state.password.in_progress => {
            append_paste(active_password_input(state), text);
            state.error = None;
        }
        LoginScreen::MfaCode if !state.password.in_progress => {
            append_paste(&mut state.password.mfa_code, text);
            state.error = None;
        }
        LoginScreen::ModeSelect
        | LoginScreen::PasswordInput
        | LoginScreen::MfaSelect
        | LoginScreen::MfaCode
        | LoginScreen::Qr => {}
    }
    None
}

fn active_password_input(state: &mut LoginState) -> &mut String {
    match state.password.active_field {
        PasswordField::Login => &mut state.password.login,
        PasswordField::Password => &mut state.password.password,
    }
}

fn append_paste(target: &mut String, text: &str) {
    target.extend(text.chars().filter(|value| !matches!(value, '\r' | '\n')));
}

pub(super) fn mfa_supports(challenge: &Option<MfaChallenge>, method: MfaMethod) -> bool {
    challenge
        .as_ref()
        .is_some_and(|challenge| challenge.methods.contains(&method))
}
