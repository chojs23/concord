use std::{env, fs, io, path::PathBuf, process::Command, time::Instant};

use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::{
    Config, DiscordClient, Result,
    discord::{AppCommand, AppEvent, MessageInfo},
    logging, token_store, tui,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;
const MAX_ATTACHMENT_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTACHMENT_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
const ATTACHMENT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);

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
                            let message = format!("load message history failed: {error}");
                            logging::timing(
                                "history",
                                format!("channel={} messages=0", channel_id.get()),
                                started.elapsed(),
                            );
                            logging::error("history", &message);
                            client.publish_event(AppEvent::MessageHistoryLoadFailed {
                                channel_id,
                                message,
                            });
                        }
                    }
                }
                AppCommand::LoadAttachmentPreview { url } => {
                    match timeout(ATTACHMENT_PREVIEW_TIMEOUT, fetch_attachment_preview(&url)).await
                    {
                        Err(_) => {
                            let message = "download image preview timed out".to_owned();
                            logging::error("preview", &message);
                            client.publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                url,
                                message,
                            });
                        }
                        Ok(bytes) => match bytes {
                            Ok(bytes) => client
                                .publish_event(AppEvent::AttachmentPreviewLoaded { url, bytes }),
                            Err(message) => {
                                logging::error("preview", &message);
                                client.publish_event(AppEvent::AttachmentPreviewLoadFailed {
                                    url,
                                    message,
                                });
                            }
                        },
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
                AppCommand::OpenUrl { url } => {
                    if let Err(error) = open_url(&url) {
                        logging::error("app", format!("open attachment failed: {error}"));
                        client.publish_event(AppEvent::GatewayError {
                            message: format!("open attachment failed: {error}"),
                        });
                    }
                }
                AppCommand::DownloadAttachment { url, filename } => {
                    match timeout(
                        ATTACHMENT_PREVIEW_TIMEOUT,
                        download_attachment(&url, &filename),
                    )
                    .await
                    {
                        Err(_) => {
                            let message = "download attachment timed out".to_owned();
                            logging::error("attachment", &message);
                            client.publish_event(AppEvent::GatewayError { message });
                        }
                        Ok(Ok(path)) => client.publish_event(AppEvent::StatusMessage {
                            message: format!("downloaded attachment to {}", path.display()),
                        }),
                        Ok(Err(message)) => {
                            logging::error("attachment", &message);
                            client.publish_event(AppEvent::GatewayError { message });
                        }
                    }
                }
                AppCommand::AddReaction {
                    channel_id,
                    message_id,
                    emoji,
                } => match client.add_reaction(channel_id, message_id, &emoji).await {
                    Ok(()) => client.publish_event(AppEvent::StatusMessage {
                        message: format!("added {emoji} reaction"),
                    }),
                    Err(error) => {
                        logging::error("app", format!("add reaction failed: {error}"));
                        client.publish_event(AppEvent::GatewayError {
                            message: format!("add reaction failed: {error}"),
                        });
                    }
                },
            }
        }
    })
}

async fn fetch_attachment_preview(url: &str) -> std::result::Result<Vec<u8>, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|error| format!("download image preview failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download image preview failed: {error}"))?;

    if let Some(length) = response.content_length()
        && length > MAX_ATTACHMENT_PREVIEW_BYTES as u64
    {
        return Err(format!(
            "image preview is too large: {length} bytes (max {MAX_ATTACHMENT_PREVIEW_BYTES})"
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("read image preview failed: {error}"))?;

    if bytes.len() > MAX_ATTACHMENT_PREVIEW_BYTES {
        return Err(format!(
            "image preview is too large: {} bytes (max {MAX_ATTACHMENT_PREVIEW_BYTES})",
            bytes.len()
        ));
    }

    Ok(bytes.to_vec())
}

async fn download_attachment(url: &str, filename: &str) -> std::result::Result<PathBuf, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|error| format!("download attachment failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download attachment failed: {error}"))?;

    if let Some(length) = response.content_length()
        && length > MAX_ATTACHMENT_DOWNLOAD_BYTES as u64
    {
        return Err(format!(
            "attachment is too large: {length} bytes (max {MAX_ATTACHMENT_DOWNLOAD_BYTES})"
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("read attachment failed: {error}"))?;
    if bytes.len() > MAX_ATTACHMENT_DOWNLOAD_BYTES {
        return Err(format!(
            "attachment is too large: {} bytes (max {MAX_ATTACHMENT_DOWNLOAD_BYTES})",
            bytes.len()
        ));
    }

    let directory = downloads_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create download directory failed: {error}"))?;
    let path = unique_download_path(&directory, &sanitize_filename(filename));
    fs::write(&path, bytes).map_err(|error| format!("write attachment failed: {error}"))?;
    Ok(path)
}

fn downloads_directory() -> std::result::Result<PathBuf, String> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home).join("Downloads"))
}

fn sanitize_filename(filename: &str) -> String {
    let sanitized: String = filename
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches([' ', '.']);
    if sanitized.is_empty() {
        "attachment".to_owned()
    } else {
        sanitized.to_owned()
    }
}

fn unique_download_path(directory: &std::path::Path, filename: &str) -> PathBuf {
    let path = directory.join(filename);
    if !path.exists() {
        return path;
    }

    let original = std::path::Path::new(filename);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = original.extension().and_then(|value| value.to_str());
    for index in 1.. {
        let candidate = match extension {
            Some(extension) => directory.join(format!("{stem} ({index}).{extension}")),
            None => directory.join(format!("{stem} ({index})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded search returns a path before exhausting usize")
}

fn open_url(url: &str) -> io::Result<()> {
    let status = open_url_command(url).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "open command exited with status {status}"
        )))
    }
}

fn open_url_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
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
