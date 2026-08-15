use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{Duration, Instant as TokioInstant, sleep, timeout};

use crate::{
    DiscordClient,
    discord::{AppEvent, AttachmentDownloadId, DownloadAttachmentSource, MediaPlaybackRequestId},
    logging,
    support::media_player::MediaPlayerIpcEndpoint,
    url_policy::normalize_openable_url,
};

pub(super) const ATTACHMENT_PREVIEW_TIMEOUT: Duration = Duration::from_secs(30);

const MAX_ATTACHMENT_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const MEDIA_PLAYER_READY_TIMEOUT: Duration = Duration::from_secs(300);
const MEDIA_PLAYER_IPC_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MEDIA_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// One pooled client for every media fetch.
///
/// `reqwest::get` builds a whole client per call, so a screen of attachments,
/// avatars, and emoji opened as many TCP connections and TLS handshakes as it
/// had images. Discord's CDN starts refusing those, which surfaced as
/// "error sending request" and preview timeouts. Reusing one client keeps the
/// connection pool alive across requests. Deliberately unauthenticated: these
/// URLs include proxies for third-party content, which must never see the
/// session cookies the Discord API client carries.
fn media_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(MEDIA_CONNECT_TIMEOUT)
            // Pooling is the point, but the default keeps unlimited idle
            // connections per host and each one holds TLS buffers.
            .pool_max_idle_per_host(6)
            .build()
            .unwrap_or_default()
    })
}

pub(super) async fn fetch_attachment_preview(url: &str) -> std::result::Result<Vec<u8>, String> {
    fetch_limited_bytes(
        url,
        MAX_ATTACHMENT_PREVIEW_BYTES,
        "image preview",
        "download image preview failed",
        "read image preview failed",
    )
    .await
}

pub(super) async fn download_attachment(
    client: &DiscordClient,
    id: AttachmentDownloadId,
    url: &str,
    filename: &str,
    source: DownloadAttachmentSource,
) -> std::result::Result<PathBuf, String> {
    let mut response = timeout(
        ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT,
        media_client().get(url).send(),
    )
    .await
    .map_err(|_| "download attachment timed out".to_owned())?
    .map_err(|error| format!("download attachment failed: {error}"))?
    .error_for_status()
    .map_err(|error| format!("download attachment failed: {error}"))?;
    let total_bytes = response.content_length();
    let filename = sanitize_filename(filename);
    let directory = downloads_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("create download directory failed: {error}"))?;
    let (mut file, temp_path) = create_download_temp_file(&directory)?;

    client
        .publish_event(AppEvent::AttachmentDownloadStarted {
            id,
            filename: filename.clone(),
            total_bytes,
            source,
        })
        .await;

    let mut downloaded_bytes = 0u64;
    let mut last_reported_bytes = 0u64;
    let mut next_progress_at = TokioInstant::now() + ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL;
    while let Some(chunk) = timeout(ATTACHMENT_DOWNLOAD_IDLE_TIMEOUT, response.chunk())
        .await
        .map_err(|_| "read attachment timed out".to_owned())?
        .map_err(|error| format!("read attachment failed: {error}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("write attachment failed: {error}"))?;
        downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
        let now = TokioInstant::now();
        if now >= next_progress_at {
            client
                .publish_event(AppEvent::AttachmentDownloadProgress {
                    id,
                    downloaded_bytes,
                    total_bytes,
                })
                .await;
            last_reported_bytes = downloaded_bytes;
            next_progress_at = now + ATTACHMENT_DOWNLOAD_PROGRESS_INTERVAL;
        }
    }

    file.flush()
        .await
        .map_err(|error| format!("write attachment failed: {error}"))?;
    drop(file.into_std().await);
    if downloaded_bytes != last_reported_bytes {
        client
            .publish_event(AppEvent::AttachmentDownloadProgress {
                id,
                downloaded_bytes,
                total_bytes,
            })
            .await;
    }
    persist_unique_download_file(&directory, &filename, temp_path)
}

async fn fetch_limited_bytes(
    url: &str,
    max_bytes: usize,
    size_label: &str,
    download_error: &str,
    read_error: &str,
) -> std::result::Result<Vec<u8>, String> {
    let response = media_client()
        .get(url)
        .send()
        .await
        .map_err(|error| format!("{download_error}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("{download_error}: {error}"))?;

    if let Some(length) = response.content_length()
        && length > max_bytes as u64
    {
        return Err(format!(
            "{size_label} is too large: {length} bytes (max {max_bytes})"
        ));
    }

    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{read_error}: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "{size_label} is too large: {} bytes (max {max_bytes})",
                bytes.len().saturating_add(chunk.len())
            ));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}

fn downloads_directory() -> std::result::Result<PathBuf, String> {
    crate::paths::download_dir()
        .ok_or_else(|| "could not resolve user download directory".to_owned())
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

fn create_download_temp_file(
    directory: &Path,
) -> std::result::Result<(tokio::fs::File, tempfile::TempPath), String> {
    let temp = tempfile::Builder::new()
        .prefix(".concord-download-")
        .tempfile_in(directory)
        .map_err(|error| format!("create temporary download file failed: {error}"))?;
    let (file, path) = temp.into_parts();
    Ok((tokio::fs::File::from_std(file), path))
}

fn persist_unique_download_file(
    directory: &Path,
    filename: &str,
    mut temp_path: tempfile::TempPath,
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

        match temp_path.persist_noclobber(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                temp_path = error.path;
            }
            Err(error) => return Err(format!("persist attachment failed: {}", error.error)),
        }
    }

    unreachable!("unbounded search returns a path before exhausting usize")
}

pub(super) fn open_url(url: &str) -> io::Result<()> {
    let url = normalize_openable_url(url)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsupported URL scheme"))?;
    let status = open_url_command(&url).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "open command exited with status {status}"
        )))
    }
}

fn open_url_command(url: &str) -> Command {
    let spec = current_open_url_command_spec(url);
    let mut command = Command::new(spec.program);
    command.args(spec.args);
    command
}

pub(super) async fn play_media(
    client: DiscordClient,
    request_id: MediaPlaybackRequestId,
    url: &str,
    label: &str,
) -> io::Result<()> {
    let ipc_endpoint = MediaPlayerIpcEndpoint::unique();
    ipc_endpoint.prepare()?;
    let spec = media_player_command_spec_for_url_with_ipc(url, Some(ipc_endpoint.server_arg()))?;
    let mut command = TokioCommand::new(spec.program);
    command
        .args(spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().map_err(media_player_spawn_error)?;
    let url = url.to_owned();
    let label = media_playback_label(label).to_owned();
    let _player_monitor_task = tokio::spawn(async move {
        monitor_media_player_window(child, ipc_endpoint, client, request_id, url, label).await;
    });
    Ok(())
}

async fn monitor_media_player_window(
    mut child: tokio::process::Child,
    ipc_endpoint: MediaPlayerIpcEndpoint,
    client: DiscordClient,
    request_id: MediaPlaybackRequestId,
    url: String,
    label: String,
) {
    let ready_timeout = sleep(MEDIA_PLAYER_READY_TIMEOUT);
    tokio::pin!(ready_timeout);
    let ready_result = wait_for_media_player_ready(&ipc_endpoint);
    tokio::pin!(ready_result);

    let outcome = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(status) => {
                    let message = if status.success() {
                        format!("play {label} failed: media player exited before opening a window")
                    } else {
                        format!("play {label} failed: media player exited with status {status}")
                    };
                    logging::error("media", &message);
                    client.publish_event(AppEvent::GatewayError { message }).await;
                }
                Err(error) => {
                    logging::error("media", format!("media player wait failed: {error}"));
                    client
                        .publish_event(AppEvent::GatewayError {
                            message: format!("play {label} failed: media player wait failed: {error}"),
                        })
                        .await;
                }
            }
            MediaPlayerWindowMonitorOutcome::ChildExited
        }
        result = &mut ready_result => {
            match result {
                Ok(()) => MediaPlayerWindowMonitorOutcome::Ready,
                Err(error) => {
                    MediaPlayerWindowMonitorOutcome::ReadinessFailed(format!(
                        "play {label} failed: media player readiness check failed: {error}"
                    ))
                }
            }
        }
        () = &mut ready_timeout => {
            MediaPlayerWindowMonitorOutcome::ReadinessFailed(
                format!(
                    "play {label} failed: media player did not configure video output within {} seconds",
                    MEDIA_PLAYER_READY_TIMEOUT.as_secs()
                ),
            )
        }
    };

    match outcome {
        MediaPlayerWindowMonitorOutcome::Ready => {
            client
                .publish_event(AppEvent::MediaPlaybackWindowReady { request_id, url })
                .await;
        }
        MediaPlayerWindowMonitorOutcome::ReadinessFailed(message) => {
            logging::error("media", &message);
            client
                .publish_event(AppEvent::GatewayError { message })
                .await;
        }
        MediaPlayerWindowMonitorOutcome::ChildExited => return,
    }

    if let Err(error) = child.wait().await {
        logging::error("media", format!("media player wait failed: {error}"));
    }
}

enum MediaPlayerWindowMonitorOutcome {
    Ready,
    ReadinessFailed(String),
    ChildExited,
}

fn media_playback_label(label: &str) -> &str {
    if label.is_empty() { "media" } else { label }
}

fn media_player_spawn_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        return io::Error::new(
            io::ErrorKind::NotFound,
            "mpv is required for media playback; install mpv and make sure it is on PATH",
        );
    }

    error
}

#[cfg(test)]
fn media_player_command_spec_for_url(url: &str) -> io::Result<MediaPlayerCommandSpec> {
    media_player_command_spec_for_url_with_ipc(url, None)
}

fn media_player_command_spec_for_url_with_ipc(
    url: &str,
    ipc_server: Option<&str>,
) -> io::Result<MediaPlayerCommandSpec> {
    let url = normalize_openable_url(url).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "unsupported media URL scheme")
    })?;
    let mut args = vec!["--no-terminal".to_owned(), "--force-window".to_owned()];
    if let Some(ipc_server) = ipc_server {
        args.push(format!("--input-ipc-server={ipc_server}"));
    }
    args.extend(["--".to_owned(), url]);
    Ok(MediaPlayerCommandSpec {
        program: "mpv",
        args,
    })
}

async fn wait_for_media_player_ready(endpoint: &MediaPlayerIpcEndpoint) -> io::Result<()> {
    #[cfg(unix)]
    {
        let stream = endpoint.connect().await?;
        wait_for_mpv_video_output_ready(stream).await
    }

    #[cfg(windows)]
    {
        let stream = endpoint.connect().await?;
        wait_for_mpv_video_output_ready(stream).await
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = endpoint;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "media player IPC is not supported on this platform",
        ))
    }
}

async fn wait_for_mpv_video_output_ready<S>(stream: S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stream);
    let mut request_id = 1_u64;
    let mut line = Vec::new();

    loop {
        let request =
            format!(r#"{{"command":["get_property","vo-configured"],"request_id":{request_id}}}"#);
        reader.get_mut().write_all(request.as_bytes()).await?;
        reader.get_mut().write_all(b"\n").await?;
        reader.get_mut().flush().await?;

        loop {
            line.clear();
            let bytes_read = reader.read_until(b'\n', &mut line).await?;
            if bytes_read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "media player IPC closed before configuring video output",
                ));
            }
            if let Some(video_output_ready) = mpv_video_output_response_readiness(&line, request_id)
            {
                if video_output_ready {
                    return Ok(());
                }
                break;
            }
        }

        request_id = request_id.saturating_add(1);
        sleep(MEDIA_PLAYER_IPC_READY_POLL_INTERVAL).await;
    }
}

fn mpv_video_output_response_readiness(line: &[u8], request_id: u64) -> Option<bool> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
        return None;
    };
    if value.get("request_id").and_then(serde_json::Value::as_u64) != Some(request_id) {
        return None;
    }
    let success = value.get("error").and_then(serde_json::Value::as_str) == Some("success");
    if !success {
        return Some(false);
    }

    value.get("data").and_then(serde_json::Value::as_bool)
}

#[derive(Debug, Eq, PartialEq)]
struct MediaPlayerCommandSpec {
    program: &'static str,
    args: Vec<String>,
}

struct UrlOpenCommandSpec {
    program: &'static str,
    args: Vec<String>,
}

fn current_open_url_command_spec(url: &str) -> UrlOpenCommandSpec {
    #[cfg(target_os = "macos")]
    {
        UrlOpenCommandSpec {
            program: "open",
            args: vec![url.to_owned()],
        }
    }

    #[cfg(target_os = "windows")]
    {
        windows_open_url_command_spec(url)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        UrlOpenCommandSpec {
            program: "xdg-open",
            args: vec![url.to_owned()],
        }
    }
}

#[cfg(any(test, target_os = "windows"))]
fn windows_open_url_command_spec(url: &str) -> UrlOpenCommandSpec {
    UrlOpenCommandSpec {
        program: "rundll32",
        args: vec!["url.dll,FileProtocolHandler".to_owned(), url.to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, process};

    use super::{
        media_player_command_spec_for_url, media_player_command_spec_for_url_with_ipc,
        media_player_spawn_error, mpv_video_output_response_readiness, open_url,
        persist_unique_download_file, sanitize_filename, wait_for_mpv_video_output_ready,
        windows_open_url_command_spec,
    };
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn unix_timestamp_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    }

    #[test]
    fn persist_unique_download_file_uses_next_available_name() {
        let directory = std::env::temp_dir().join(format!(
            "concord-download-test-{}-{}",
            process::id(),
            unix_timestamp_nanos()
        ));
        fs::create_dir_all(&directory).expect("test directory should be created");
        let existing = directory.join("cat.png");
        fs::write(&existing, b"old").expect("existing file should be written");
        let mut temp = tempfile::Builder::new()
            .tempfile_in(&directory)
            .expect("temporary file should be created");
        temp.write_all(b"new")
            .expect("temporary file should be written");
        let temp_path = temp.into_temp_path();

        let path = persist_unique_download_file(&directory, "cat.png", temp_path)
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

    #[test]
    fn open_url_rejects_non_web_schemes_before_spawning_opener() {
        let error = open_url("file:///etc/passwd").expect_err("file URLs should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn media_player_rejects_non_web_schemes_before_spawning_player() {
        let error = media_player_command_spec_for_url("file:///etc/passwd")
            .expect_err("file URLs should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn media_player_uses_mpv_without_shell_parsing() {
        let spec = media_player_command_spec_for_url("https://example.com/video.mp4?x=1&y=2")
            .expect("https media URLs should be accepted");

        assert_eq!(spec.program, "mpv");
        assert_eq!(
            spec.args,
            vec![
                "--no-terminal".to_owned(),
                "--force-window".to_owned(),
                "--".to_owned(),
                "https://example.com/video.mp4?x=1&y=2".to_owned(),
            ]
        );
    }

    #[test]
    fn media_player_command_can_enable_json_ipc() {
        let spec = media_player_command_spec_for_url_with_ipc(
            "https://example.com/video.mp4",
            Some("/tmp/concord-mpv.sock"),
        )
        .expect("https media URLs should be accepted");

        assert_eq!(spec.program, "mpv");
        assert_eq!(
            spec.args,
            vec![
                "--no-terminal".to_owned(),
                "--force-window".to_owned(),
                "--input-ipc-server=/tmp/concord-mpv.sock".to_owned(),
                "--".to_owned(),
                "https://example.com/video.mp4".to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn mpv_video_output_readiness_polls_until_configured() {
        let (client, server) = tokio::io::duplex(512);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut line = Vec::new();

            server
                .read_until(b'\n', &mut line)
                .await
                .expect("test server should read the first readiness request");
            let request: Value =
                serde_json::from_slice(&line).expect("readiness request should be valid JSON");
            assert_eq!(request["command"], json!(["get_property", "vo-configured"]));
            assert_eq!(request["request_id"], 1);
            server
                .get_mut()
                .write_all(b"{\"data\":false,\"error\":\"success\",\"request_id\":1}\n")
                .await
                .expect("test server should report video output not configured");

            line.clear();
            server
                .read_until(b'\n', &mut line)
                .await
                .expect("test server should read the next readiness request");
            let request: Value =
                serde_json::from_slice(&line).expect("readiness request should be valid JSON");
            assert_eq!(request["command"], json!(["get_property", "vo-configured"]));
            assert_eq!(request["request_id"], 2);
            server
                .get_mut()
                .write_all(b"{\"data\":true,\"error\":\"success\",\"request_id\":2}\n")
                .await
                .expect("test server should report video output configured");
        });

        wait_for_mpv_video_output_ready(client)
            .await
            .expect("configured video output should complete readiness polling");
        server.await.expect("test server should finish");
    }

    #[tokio::test]
    async fn mpv_video_output_readiness_ignores_unrelated_messages() {
        let (client, server) = tokio::io::duplex(512);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut line = Vec::new();
            server
                .read_until(b'\n', &mut line)
                .await
                .expect("test server should read the readiness request");
            server
                .get_mut()
                .write_all(
                    b"{\"event\":\"file-loaded\"}\nnot json\n{\"data\":true,\"error\":\"success\",\"request_id\":99}\n{\"data\":true,\"error\":\"success\",\"request_id\":1}\n",
                )
                .await
                .expect("test server should send readiness messages");
        });

        wait_for_mpv_video_output_ready(client)
            .await
            .expect("matching configured response should complete readiness polling");
        server.await.expect("test server should finish");
    }

    #[tokio::test]
    async fn mpv_video_output_readiness_errors_when_ipc_closes() {
        let (client, server) = tokio::io::duplex(512);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut line = Vec::new();
            server
                .read_until(b'\n', &mut line)
                .await
                .expect("test server should read the readiness request");
        });

        let error = wait_for_mpv_video_output_ready(client)
            .await
            .expect_err("closed IPC should fail before readiness");

        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
        server.await.expect("test server should finish");
    }

    #[test]
    fn mpv_video_output_response_requires_boolean_readiness() {
        assert_eq!(
            mpv_video_output_response_readiness(
                br#"{"data":true,"error":"success","request_id":7}"#,
                7,
            ),
            Some(true)
        );
        assert_eq!(
            mpv_video_output_response_readiness(
                br#"{"data":false,"error":"success","request_id":7}"#,
                7,
            ),
            Some(false)
        );
        assert_eq!(
            mpv_video_output_response_readiness(
                br#"{"data":true,"error":"success","request_id":6}"#,
                7,
            ),
            None
        );
        for invalid_data in [
            br#"{"error":"success","request_id":7}"#.as_slice(),
            br#"{"data":null,"error":"success","request_id":7}"#.as_slice(),
            br#"{"data":"true","error":"success","request_id":7}"#.as_slice(),
            br#"{"data":1,"error":"success","request_id":7}"#.as_slice(),
        ] {
            assert_eq!(mpv_video_output_response_readiness(invalid_data, 7), None);
        }
        assert_eq!(
            mpv_video_output_response_readiness(
                br#"{"data":true,"error":"property unavailable","request_id":7}"#,
                7,
            ),
            Some(false)
        );
    }

    #[test]
    fn media_player_missing_binary_error_mentions_mpv_requirement() {
        let error = media_player_spawn_error(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file or directory",
        ));

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            error.to_string(),
            "mpv is required for media playback; install mpv and make sure it is on PATH"
        );
    }

    #[test]
    fn windows_url_opener_avoids_cmd_shell_parsing() {
        let spec = windows_open_url_command_spec("https://example.com/?a=1&b=2");

        assert_eq!(spec.program, "rundll32");
        assert_eq!(
            spec.args,
            vec![
                "url.dll,FileProtocolHandler".to_owned(),
                "https://example.com/?a=1&b=2".to_owned(),
            ]
        );
    }
}
