mod render;
mod state;
mod terminal_events;

#[cfg(test)]
mod tests;

use crossterm::event::EventStream;
use futures::StreamExt;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    AppError, Result,
    discord::{
        password_auth::{self, MfaMethod, PasswordAuthEvent},
        qr_auth::{self, QrEvent},
    },
};

use self::{
    render::render,
    state::{LoginScreen, LoginState},
    terminal_events::{LoginAction, handle_terminal},
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
