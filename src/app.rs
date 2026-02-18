use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::{
    DiscordClient, Result,
    discord::{AppCommand, AppEvent, MessageInfo},
    logging, token_store, tui,
};

const MESSAGE_HISTORY_LIMIT: u16 = 50;
const MAX_ATTACHMENT_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTACHMENT_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
const ATTACHMENT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<()> {
        let resolved_token = resolve_token().await?;
        let token = resolved_token.token;
        let token_warnings = resolved_token.warnings;
        let client = DiscordClient::new(token)?;
        let events = client.subscribe();
        let (commands_tx, commands_rx) = mpsc::channel(64);
        let gateway_task = client.start_gateway();
        let command_task = start_command_loop(client.clone(), commands_rx);

        let result = async {
            for warning in token_warnings {
                logging::error("app", &warning);
                client.publish_event(AppEvent::GatewayError { message: warning });
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
                AppCommand::LoadMessageHistory { channel_id, before } => {
                    let started = Instant::now();
                    match client
                        .load_message_history(channel_id, before, MESSAGE_HISTORY_LIMIT)
                        .await
                    {
                        Ok(messages) => {
                            logging::timing(
                                "history",
                                format!(
                                    "channel={} before={} messages={}",
                                    channel_id.get(),
                                    before.map(|id| id.get()).unwrap_or_default(),
                                    messages.len()
                                ),
                                started.elapsed(),
                            );
                            client.publish_event(AppEvent::MessageHistoryLoaded {
                                channel_id,
                                before,
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
                                format!(
                                    "channel={} before={} messages=0",
                                    channel_id.get(),
                                    before.map(|id| id.get()).unwrap_or_default()
                                ),
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
                AppCommand::LoadGuildMembers { guild_id } => {
                    if let Err(message) = client.request_guild_members(guild_id) {
                        logging::error("app", &message);
                        client.publish_event(AppEvent::GatewayError { message });
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
                    reply_to,
                } => match client.send_message(channel_id, &content, reply_to).await {
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
                        message: format!("added {} reaction", emoji.status_label()),
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
    write_unique_download_file(&directory, &sanitize_filename(filename), &bytes)
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

fn write_unique_download_file(
    directory: &Path,
    filename: &str,
    bytes: &[u8],
) -> std::result::Result<PathBuf, String> {
    let original = Path::new(filename);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = original.extension().and_then(|value| value.to_str());

    for index in 0.. {
        let candidate = if index == 0 {
            directory.join(filename)
        } else {
            match extension {
                Some(extension) => directory.join(format!("{stem} ({index}).{extension}")),
                None => directory.join(format!("{stem} ({index})")),
            }
        };

        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(bytes)
                    .map_err(|error| format!("write attachment failed: {error}"))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("write attachment failed: {error}")),
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

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::{sanitize_filename, write_unique_download_file};

    fn unix_timestamp_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    }

    #[test]
    fn write_unique_download_file_uses_next_available_name() {
        let directory = std::env::temp_dir().join(format!(
            "concord-download-test-{}-{}",
            process::id(),
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let existing = directory.join("cat.png");
        fs::write(&existing, b"old").expect("existing file should be written");

        let path = write_unique_download_file(&directory, "cat.png", b"new")
            .expect("download file should be written");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("cat (1).png")
        );
        assert_eq!(
            fs::read(&existing).expect("existing file should remain"),
            b"old"
        );
        assert_eq!(fs::read(&path).expect("new file should be written"), b"new");

        fs::remove_dir_all(&directory).expect("test directory should be removed");
    }

    #[test]
    fn sanitize_filename_replaces_path_separators() {
        assert_eq!(sanitize_filename("../cat\\dog.png"), "_cat_dog.png");
    }
}
