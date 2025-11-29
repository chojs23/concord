use std::time::Instant;

use tokio::sync::mpsc;

use crate::{
    Config, DiscordClient, Result,
    discord::{AppCommand, AppEvent, MessageInfo},
    logging, token_store, tui,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;

pub struct App {
    config: Config,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let resolved_token = resolve_token().await?;
        let token = resolved_token.token;
        let token_warnings = resolved_token.warnings;
        let client = DiscordClient::new(token)?;
        let events = client.subscribe();
        let (commands_tx, commands_rx) = mpsc::channel(64);
        let gateway_task = client.start_gateway(self.config.enable_message_content);
        let command_task = start_command_loop(client.clone(), commands_rx);

        let result = async {
            for warning in token_warnings {
                logging::error("app", &warning);
                client.publish_event(AppEvent::GatewayError { message: warning });
            }

            if let (Some(channel_id), Some(message)) = (
                self.config.default_channel_id,
                self.config.boot_message.as_deref(),
            ) && let Err(error) = client.send_message(channel_id, message).await
            {
                logging::error("app", format!("startup message failed: {error}"));
                client.publish_event(AppEvent::GatewayError {
                    message: format!("startup message failed: {error}"),
                });
            }

            tui::run(events, commands_tx).await
        }
        .await;

        command_task.abort();
        shutdown_gateway(gateway_task).await;
        result
    }
}

fn start_command_loop(
    client: DiscordClient,
    mut commands: mpsc::Receiver<AppCommand>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            match command {
                AppCommand::LoadMessageHistory { channel_id } => {
                    let started = Instant::now();
                    match client
                        .load_message_history(channel_id, MESSAGE_HISTORY_LIMIT)
                        .await
                    {
                        Ok(messages) => {
                            logging::timing(
                                "history",
                                format!("channel={} messages={}", channel_id.get(), messages.len()),
                                started.elapsed(),
                            );
                            client.publish_event(AppEvent::MessageHistoryLoaded {
                                channel_id,
                                messages: messages
                                    .into_iter()
                                    .map(MessageInfo::from_message)
                                    .collect(),
                            });
                        }
                        Err(error) => {
                            logging::timing(
                                "history",
                                format!("channel={} messages=0", channel_id.get()),
                                started.elapsed(),
                            );
                            logging::error(
                                "history",
                                format!("load message history failed: {error}"),
                            );
                            client.publish_event(AppEvent::GatewayError {
                                message: format!("load message history failed: {error}"),
                            });
                        }
                    }
                }
                AppCommand::SendMessage {
                    channel_id,
                    content,
                } => match client.send_message(channel_id, &content).await {
                    Ok(message) => client.publish_event(AppEvent::from_message(message)),
                    Err(error) => {
                        logging::error("app", format!("send message failed: {error}"));
                        client.publish_event(AppEvent::GatewayError {
                            message: format!("send message failed: {error}"),
                        });
                    }
                },
            }
        }
    })
}

struct ResolvedToken {
    token: String,
    warnings: Vec<String>,
}

async fn resolve_token() -> Result<ResolvedToken> {
    let mut warnings = Vec::new();

    match token_store::load_token() {
        Ok(Some(token)) => {
            return Ok(ResolvedToken { token, warnings });
        }
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "credential store unavailable: {error}; enter a token to continue for this session"
        )),
    }

    let login_notice = if warnings.is_empty() {
        None
    } else {
        Some("Credential storage is unavailable; token may not be saved.".to_owned())
    };

    let token = tui::prompt_login(login_notice).await?;
    if let Err(error) = token_store::save_token(&token) {
        warnings.push(format!("token was not saved: {error}"));
    }

    Ok(ResolvedToken { token, warnings })
}

async fn shutdown_gateway(gateway_task: tokio::task::JoinHandle<()>) {
    gateway_task.abort();

    if let Err(error) = gateway_task.await
        && !error.is_cancelled()
    {
        logging::error("app", format!("gateway task ended unexpectedly: {error}"));
    }
}
