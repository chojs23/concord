use std::{io, time::Duration};

#[cfg(unix)]
use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    time::sleep,
};

#[cfg(unix)]
use crate::logging;

const MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const MPV_COMMAND_REQUEST_ID: u64 = 1;

#[derive(Debug)]
pub(crate) struct MediaPlayerIpcEndpoint {
    server_arg: String,
    #[cfg(unix)]
    socket_path: PathBuf,
}

impl MediaPlayerIpcEndpoint {
    pub(crate) fn unique() -> Self {
        let id = uuid::Uuid::new_v4();

        #[cfg(unix)]
        {
            let socket_path = std::env::temp_dir().join(format!("concord-mpv-{id}.sock"));
            Self {
                server_arg: socket_path.display().to_string(),
                socket_path,
            }
        }

        #[cfg(windows)]
        {
            Self {
                server_arg: format!(r"\\.\pipe\concord-mpv-{id}"),
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            Self {
                server_arg: std::env::temp_dir()
                    .join(format!("concord-mpv-{id}.sock"))
                    .display()
                    .to_string(),
            }
        }
    }

    pub(crate) fn server_arg(&self) -> &str {
        &self.server_arg
    }

    pub(crate) fn prepare(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            match fs::remove_file(&self.socket_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }

        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    #[cfg(unix)]
    pub(crate) async fn connect(&self) -> io::Result<tokio::net::UnixStream> {
        loop {
            match tokio::net::UnixStream::connect(&self.socket_path).await {
                Ok(stream) => return Ok(stream),
                Err(error) if ipc_connect_error_is_retryable(&error) => {
                    sleep(MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[cfg(windows)]
    pub(crate) async fn connect(
        &self,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(self.server_arg()) {
                Ok(stream) => return Ok(stream),
                Err(error) if ipc_connect_error_is_retryable(&error) => {
                    sleep(MEDIA_PLAYER_IPC_CONNECT_RETRY_INTERVAL).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) async fn set_property(&self, name: &str, value: &str) -> io::Result<()> {
        #[cfg(any(unix, windows))]
        {
            let stream = self.connect().await?;
            send_mpv_set_property(stream, name, value).await
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = (name, value);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "media player IPC is not supported on this platform",
            ))
        }
    }
}

impl Drop for MediaPlayerIpcEndpoint {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Err(error) = fs::remove_file(&self.socket_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            logging::error("media", format!("media player IPC cleanup failed: {error}"));
        }
    }
}

fn ipc_connect_error_is_retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused | io::ErrorKind::WouldBlock
    )
}

async fn send_mpv_set_property<S>(stream: S, name: &str, value: &str) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut stream = BufReader::new(stream);
    let request = json!({
        "command": ["set_property", name, value],
        "request_id": MPV_COMMAND_REQUEST_ID,
    });
    let request = serde_json::to_vec(&request).expect("mpv property command is valid JSON");
    stream.get_mut().write_all(&request).await?;
    stream.get_mut().write_all(b"\n").await?;
    stream.get_mut().flush().await?;

    let mut line = Vec::new();
    loop {
        line.clear();
        if stream.read_until(b'\n', &mut line).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "media player IPC closed before acknowledging command",
            ));
        }
        if let Some(result) = mpv_command_response(&line, MPV_COMMAND_REQUEST_ID) {
            return result;
        }
    }
}

fn mpv_command_response(line: &[u8], request_id: u64) -> Option<io::Result<()>> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    (value.get("request_id").and_then(Value::as_u64) == Some(request_id)).then(|| {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        if error == "success" {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "media player command failed: {error}"
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn media_player_ipc_sets_a_property_and_waits_for_success() {
        let (client, server) = tokio::io::duplex(512);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            let mut line = Vec::new();
            server
                .read_until(b'\n', &mut line)
                .await
                .expect("test server should read the command");
            let request: Value =
                serde_json::from_slice(&line).expect("command should be valid JSON");
            assert_eq!(request["command"], json!(["set_property", "aid", "auto"]));
            assert_eq!(request["request_id"], MPV_COMMAND_REQUEST_ID);
            server
                .get_mut()
                .write_all(b"{\"error\":\"success\",\"request_id\":1}\n")
                .await
                .expect("test server should acknowledge the command");
        });

        send_mpv_set_property(client, "aid", "auto")
            .await
            .expect("successful mpv response should complete the command");
        server.await.expect("test server should finish");
    }

    #[test]
    fn media_player_ipc_matches_responses_to_the_request() {
        assert!(mpv_command_response(br#"{"error":"success","request_id":6}"#, 7).is_none());
        assert!(
            mpv_command_response(br#"{"error":"success","request_id":7}"#, 7)
                .expect("matching response should be recognized")
                .is_ok()
        );
        assert!(
            mpv_command_response(br#"{"error":"property unavailable","request_id":7}"#, 7)
                .expect("matching error response should be recognized")
                .is_err()
        );
    }
}
