use crate::discord::password_auth::{MfaChallenge, MfaMethod};

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
    pub(super) screen: LoginScreen,
    pub(super) notice: Option<String>,
    pub(super) error: Option<String>,
    pub(super) token_input: String,
    pub(super) password: PasswordViewState,
    pub(super) qr: QrViewState,
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

    pub(super) fn reset_sensitive(&mut self) {
        self.password.clear();
        self.mfa = None;
        self.mfa_method = None;
        self.mfa_code.clear();
        self.status.clear();
        self.in_progress = false;
    }
}

pub(super) struct QrViewState {
    pub(super) status: String,
    pub(super) bitmap: Option<Vec<Vec<bool>>>,
    pub(super) pending_user: Option<String>,
}

impl QrViewState {
    fn new() -> Self {
        Self {
            status: String::new(),
            bitmap: None,
            pending_user: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.status.clear();
        self.bitmap = None;
        self.pending_user = None;
    }
}

impl LoginState {
    pub(super) fn new(notice: Option<String>) -> Self {
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
