mod command_dispatch;
mod command_loop;
mod credentials;
mod gateway_commands;
mod history_commands;
mod inbox_commands;
mod media_adapters;
mod media_commands;
mod message_commands;
mod notification_commands;
mod read_state_commands;
mod session_commands;
mod shutdown;
mod user_commands;
mod voice_commands;

use tokio::sync::mpsc;

use crate::{
    DiscordClient, Result, config,
    discord::{AppEvent, DiscordAuthSession},
    logging, tui, version_check,
};

use self::{
    command_loop::start_command_loop,
    credentials::resolve_token,
    shutdown::{leave_current_voice_channel_on_shutdown, shutdown_gateway},
};

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let theme_warnings = match config::load_theme_options_with_warnings() {
            Ok((theme_options, mut warnings)) => {
                warnings.extend(tui::initialize_theme(&theme_options));
                warnings
            }
            Err(error) => {
                logging::error("config", format!("failed to load theme config: {error}"));
                let mut warnings = vec![format!("theme.toml could not be loaded: {error}")];
                warnings.extend(tui::initialize_theme(&config::ThemeOptions::default()));
                warnings
            }
        };

        loop {
            let (fingerprint, http) = crate::discord::load_client_fingerprint_and_http().await;
            let auth_session = DiscordAuthSession::with_http(fingerprint, http);
            let resolved_token = resolve_token(auth_session.clone()).await?;
            let token = resolved_token.token;
            let token_warnings = resolved_token.warnings;

            let client = DiscordClient::new_with_auth_session(token, auth_session)?;
            let effects = client.take_effects();
            let snapshots = client.subscribe_snapshots();
            let (commands_tx, commands_rx) = mpsc::channel(64);
            let serve_rich_presence = config::load_options()
                .map(|options| options.presence.share_rich_presence)
                .unwrap_or(true);
            let gateway_task = client.start_gateway(serve_rich_presence);
            let command_task = start_command_loop(client.clone(), commands_rx);

            let version_client = client.clone();
            tokio::spawn(async move {
                match version_check::check_latest_version().await {
                    Ok(Some(latest_version)) => {
                        version_client
                            .publish_event(AppEvent::UpdateAvailable { latest_version })
                            .await;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        logging::debug("version", format!("latest version check failed: {error}"))
                    }
                }
            });

            let result = async {
                for warning in token_warnings {
                    logging::error("app", &warning);
                    client
                        .publish_event(AppEvent::GatewayError { message: warning })
                        .await;
                }

                tui::run(
                    effects,
                    snapshots,
                    commands_tx,
                    client.clone(),
                    theme_warnings.clone(),
                )
                .await
            }
            .await;

            command_task.abort();
            leave_current_voice_channel_on_shutdown(&client);
            shutdown_gateway(&client, gateway_task).await;
            match result? {
                tui::DashboardExit::Quit => return Ok(()),
                // Sign-out of an env-token session quits: re-resolving would
                // read the same CONCORD_TOKEN and log straight back in.
                tui::DashboardExit::SignOut => {
                    if crate::token_store::env_token().is_some() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
