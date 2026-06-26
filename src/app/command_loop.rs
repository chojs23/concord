use tokio::sync::mpsc;

use crate::{
    DiscordClient,
    discord::{AppCommand, AppEvent},
    error::AppError,
    logging,
};

use super::command_dispatch::CommandDispatcher;

pub(super) fn start_command_loop(
    client: DiscordClient,
    mut commands: mpsc::Receiver<AppCommand>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let dispatcher = CommandDispatcher::new(client);
        while let Some(command) = commands.recv().await {
            dispatcher.dispatch(command).await;
        }
    })
}

pub(super) fn log_app_error(context: &str, error: &AppError) {
    logging::error(
        "app",
        format!("{context}: {}; detail={}", error, error.log_detail()),
    );
}

pub(super) async fn publish_app_error(client: &DiscordClient, context: &str, error: &AppError) {
    log_app_error(context, error);
    // A captcha gate is not a connection failure, so it gets its own event
    // instead of the persistent gateway-error banner.
    let event = match error {
        AppError::CaptchaRequired { action } => AppEvent::CaptchaRequired {
            action: action.clone(),
        },
        _ => AppEvent::GatewayError {
            message: format!("{context}: {error}"),
        },
    };
    client.publish_event(event).await;
}
