use crate::{
    DiscordClient, Result, config,
    discord::{AppCommand, AppEvent},
    error::AppError,
    logging, token_store,
};

use super::command_loop::publish_app_error;

pub(super) async fn handle(client: DiscordClient, command: AppCommand) {
    match command {
        AppCommand::SignOut => match delete_saved_credentials().await {
            Ok(()) => client.publish_event(AppEvent::SignedOut).await,
            Err(error) => publish_app_error(&client, "sign out failed", &error).await,
        },
        _ => unreachable!("non-session command routed to session handler"),
    }
}

async fn delete_saved_credentials() -> Result<()> {
    // An env-token session has nothing stored to delete
    if token_store::env_token().is_some() {
        return Ok(());
    }

    let credential_store = match config::load_options() {
        Ok(options) => options.credentials.store,
        Err(error) => {
            logging::error(
                "app",
                format!(
                    "config could not be loaded for sign-out credential settings: {error}; using auto credential storage"
                ),
            );
            config::CredentialStoreMode::default()
        }
    };

    tokio::task::spawn_blocking(move || token_store::delete_token(credential_store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
}
