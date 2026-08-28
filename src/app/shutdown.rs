use tokio::time::{Duration, sleep};

use crate::{DiscordClient, logging};

pub(super) async fn shutdown_gateway(
    client: &DiscordClient,
    mut gateway_task: tokio::task::JoinHandle<()>,
) {
    if let Err(message) = client.shutdown_voice_runtime().await {
        logging::error("app", format!("voice runtime shutdown failed: {message}"));
    }
    if let Err(message) = client.shutdown_gateway() {
        logging::error("app", format!("gateway shutdown request failed: {message}"));
        gateway_task.abort();
    }

    tokio::select! {
        result = &mut gateway_task => {
            if let Err(error) = result
                && !error.is_cancelled()
            {
                logging::error("app", format!("gateway task ended unexpectedly: {error}"));
            }
        }
        () = sleep(Duration::from_secs(2)) => {
            gateway_task.abort();
            if let Err(error) = gateway_task.await
                && !error.is_cancelled()
            {
                logging::error("app", format!("gateway task ended unexpectedly: {error}"));
            }
        }
    }
}
