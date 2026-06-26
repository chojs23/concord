use tokio::time::{Duration, sleep};

use crate::{DiscordClient, logging};

pub(super) fn leave_current_voice_channel_on_shutdown(client: &DiscordClient) {
    let Some(voice) = client.requested_voice_connection() else {
        return;
    };
    if let Err(message) =
        client.update_voice_state(voice.scope, None, voice.self_mute, voice.self_deaf)
    {
        logging::error("app", format!("voice shutdown leave failed: {message}"));
    }
}

pub(super) async fn shutdown_gateway(
    client: &DiscordClient,
    mut gateway_task: tokio::task::JoinHandle<()>,
) {
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
