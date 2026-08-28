mod clipboard;
mod commands;
mod fuzzy;
#[cfg(feature = "voice-playback")]
mod global_push_to_talk;
mod input;
mod keybindings;
mod login;
mod media;
mod message;
mod runtime;
mod selection;
mod state;
mod terminal;
mod text;
mod text_cursor;
mod text_input;
mod theme;
mod ui;

use tokio::sync::{mpsc, watch};

use crate::{
    AppError, Result,
    config::{KeymapOptions, ThemeOptions},
    discord::{
        AppCommand, DiscordAuthSession, DiscordClient, SequencedAppEvent, SnapshotRevision,
        load_client_fingerprint_and_http,
    },
};

pub use runtime::DashboardExit;

pub fn validate_keymap_options(keymap_options: &KeymapOptions) -> Result<()> {
    keybindings::KeyBindings::try_from_options(keymap_options)
        .map(|_| ())
        .map_err(AppError::InvalidKeymapConfig)
}

/// Resolves `theme_options` against the built-in defaults and returns any
/// per-field warnings, without applying the result. Theme values never fail
/// startup outright (an unparseable color just falls back), so this is a
/// report, not a pass/fail check like [`validate_keymap_options`].
pub fn theme_options_warnings(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    theme::Theme::from_options(theme_options, &mut warnings);
    warnings
}

pub fn initialize_theme(theme_options: &ThemeOptions) -> Vec<String> {
    let mut warnings = Vec::new();
    let resolved = theme::Theme::from_options(theme_options, &mut warnings);
    theme::init(resolved);
    warnings
}

pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    let (fingerprint, http) = load_client_fingerprint_and_http().await;
    let auth_session = DiscordAuthSession::with_http(fingerprint, http);
    login::prompt_login(notice, auth_session).await
}

pub(crate) async fn prompt_login_with_auth_session(
    notice: Option<String>,
    auth_session: DiscordAuthSession,
) -> Result<String> {
    login::prompt_login(notice, auth_session).await
}

pub async fn run(
    mut effects: mpsc::Receiver<SequencedAppEvent>,
    mut snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
    config_warnings: Vec<String>,
) -> Result<DashboardExit> {
    let mut terminal = ratatui::init();
    let _restore_guard = match terminal::TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };

    runtime::run_dashboard(
        &mut terminal,
        &mut effects,
        &mut snapshots,
        commands,
        client,
        config_warnings,
    )
    .await
}
