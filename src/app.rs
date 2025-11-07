use tokio::sync::broadcast;

use crate::{Config, DiscordClient, Result, discord::AppEvent};

pub struct App {
    config: Config,
    client: DiscordClient,
}

impl App {
    pub fn new(config: Config) -> Self {
        let client = DiscordClient::new(&config);
        Self { config, client }
    }

    pub async fn run(self) -> Result<()> {
        let mut events = self.client.subscribe();
        let gateway_task = self
            .client
            .start_gateway(self.config.enable_message_content);

        let result = async {
            if let (Some(channel_id), Some(message)) = (
                self.config.default_channel_id,
                self.config.boot_message.as_deref(),
            ) {
                let sent = self.client.send_message(channel_id, message).await?;
                println!(
                    "sent startup message {} to channel {}",
                    sent.id.get(),
                    channel_id.get()
                );
            }

            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => print_event(event),
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            eprintln!("event stream lagged, skipped {skipped} event(s)");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    signal = tokio::signal::ctrl_c() => {
                        signal?;
                        println!("received ctrl-c, shutting down");
                        break;
                    }
                }
            }

            Ok(())
        }
        .await;

        shutdown_gateway(gateway_task).await;
        result
    }
}

async fn shutdown_gateway(gateway_task: tokio::task::JoinHandle<()>) {
    gateway_task.abort();

    if let Err(error) = gateway_task.await {
        if !error.is_cancelled() {
            eprintln!("gateway task ended unexpectedly: {error}");
        }
    }
}

fn print_event(event: AppEvent) {
    match event {
        AppEvent::Ready { user } => println!("gateway ready as {user}"),
        AppEvent::MessageCreate {
            guild_id,
            channel_id,
            message_id,
            author_id,
            author,
            content,
        } => {
            println!(
                "message create guild={} channel={} message={} author={} ({}) content={}",
                guild_id.map(|id| id.get().to_string()).unwrap_or_else(|| "dm".to_owned()),
                channel_id.get(),
                message_id.get(),
                author,
                author_id.get(),
                content.as_deref().unwrap_or("<unavailable>")
            );
        }
        AppEvent::MessageUpdate {
            guild_id,
            channel_id,
            message_id,
            content,
        } => {
            println!(
                "message update guild={} channel={} message={} content={}",
                guild_id.map(|id| id.get().to_string()).unwrap_or_else(|| "dm".to_owned()),
                channel_id.get(),
                message_id.get(),
                content.as_deref().unwrap_or("<unavailable>")
            );
        }
        AppEvent::GatewayError { message } => eprintln!("gateway error: {message}"),
        AppEvent::GatewayClosed => println!("gateway closed"),
    }
}
