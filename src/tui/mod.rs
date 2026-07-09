mod clipboard;
mod commands;
mod fuzzy;
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
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::discord::{AppEvent, DiscordClient, SequencedAppEvent};

    use super::{
        media::{AvatarImageCache, EmojiImageCache, ImagePreviewCache},
        runtime::effects,
    };
    use crate::tui::state::DashboardState;

    #[test]
    fn effect_waits_until_snapshot_revision_catches_up() {
        let mut state = DashboardState::new();
        let mut image_previews = ImagePreviewCache::new();
        let mut avatar_images = AvatarImageCache::new();
        let mut emoji_images = EmojiImageCache::new();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let (media_decode_tx, _media_decode_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut deferred_effects = VecDeque::new();

        {
            let mut ctx = effects::EffectContext {
                state: &mut state,
                client: &client,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                media_decode_tx: &media_decode_tx,
            };
            effects::process_sequenced_effect(
                SequencedAppEvent {
                    revision: 2,
                    event: AppEvent::Ready {
                        user: "tester".to_owned(),
                        user_id: None,
                    },
                },
                1,
                &mut deferred_effects,
                &mut ctx,
            );
        }

        assert_eq!(deferred_effects.len(), 1);
        assert_eq!(state.current_user(), None);

        {
            let mut ctx = effects::EffectContext {
                state: &mut state,
                client: &client,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                media_decode_tx: &media_decode_tx,
            };
            effects::process_deferred_effects(2, &mut deferred_effects, &mut ctx);
        }

        assert!(deferred_effects.is_empty());
        assert_eq!(state.current_user(), Some("tester"));
    }
}
