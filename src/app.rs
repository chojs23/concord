use tokio::sync::mpsc;

use crate::{
    Config, DiscordClient, Result,
    discord::{AppCommand, AppEvent},
    token_store, tui,
};

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
        let client = DiscordClient::new(token);
        let events = client.subscribe();
        let (commands_tx, commands_rx) = mpsc::channel(64);
        let gateway_task = client.start_gateway(self.config.enable_message_content);
        let command_task = start_command_loop(client.clone(), commands_rx);

        let result = async {
            for warning in token_warnings {
                client.publish_event(AppEvent::GatewayError { message: warning });
            }

            if let (Some(channel_id), Some(message)) = (
                self.config.default_channel_id,
                self.config.boot_message.as_deref(),
            ) && let Err(error) = client.send_message(channel_id, message).await
            {
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
                AppCommand::SendMessage {
                    channel_id,
                    content,
                } => match client.send_message(channel_id, &content).await {
                    Ok(message) => client.publish_event(AppEvent::from_message(message)),
                    Err(error) => client.publish_event(AppEvent::GatewayError {
                        message: format!("send message failed: {error}"),
                    }),
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

    let token = tui::prompt_token(login_notice).await?;
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
        eprintln!("gateway task ended unexpectedly: {error}");
    }
}
