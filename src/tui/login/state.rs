use crate::discord::password_auth::{MfaChallenge, MfaMethod};

use super::super::keybindings::KeyBindings;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LoginScreen {
    ModeSelect,
    TokenInput,
    PasswordInput,
    MfaSelect,
    MfaCode,
    Qr,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PasswordField {
    Login,
    Password,
}

pub(super) struct LoginState {
    pub(super) key_bindings: KeyBindings,
    pub(super) screen: LoginScreen,
    pub(super) notice: Option<String>,
    pub(super) error: Option<String>,
    pub(super) token_input: String,
    pub(super) password: PasswordViewState,
    pub(super) qr: QrViewState,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            key_bindings: KeyBindings::default(),
            screen: LoginScreen::ModeSelect,
            notice: None,
            error: None,
            token_input: String::new(),
            password: PasswordViewState::default(),
            qr: QrViewState::default(),
        }
    }
}

pub(super) struct PasswordViewState {
    pub(super) login: String,
    pub(super) password: String,
    pub(super) active_field: PasswordField,
    pub(super) status: String,
    pub(super) mfa: Option<MfaChallenge>,
    pub(super) mfa_method: Option<MfaMethod>,
    pub(super) mfa_code: String,
    pub(super) in_progress: bool,
}

impl PasswordViewState {
    pub(super) fn reset_sensitive(&mut self) {
        self.password.clear();
        self.mfa = None;
        self.mfa_method = None;
        self.mfa_code.clear();
        self.status.clear();
        self.in_progress = false;
    }
}

impl Default for PasswordViewState {
    fn default() -> Self {
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
}

#[derive(Default)]
pub(super) struct QrViewState {
    pub(super) status: String,
    pub(super) bitmap: Option<Vec<Vec<bool>>>,
    pub(super) pending_user: Option<String>,
}

impl QrViewState {
    pub(super) fn reset(&mut self) {
        self.status.clear();
        self.bitmap = None;
        self.pending_user = None;
    }
}

impl LoginState {
    pub(super) fn new(notice: Option<String>) -> Self {
        Self {
            notice,
            ..Self::default()
        }
    }
}
