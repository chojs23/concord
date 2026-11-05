use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_tungstenite::tungstenite::{
    Message, client::IntoClientRequest, handshake::derive_accept_key,
};

use super::*;
use crate::discord::fingerprint::{
    CLIENT_BUILD_NUMBER, ClientFingerprint, discord_gateway_headers,
};

#[tokio::test]
async fn chrome_websocket_preserves_handshake_headers_and_buffered_frames() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("local listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
        let expected_agent = fingerprint.user_agent.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("client connects");
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.expect("handshake is readable"));
            }
            let request = String::from_utf8(request).expect("handshake is text");
            let fields: Vec<_> = request.lines().filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.to_ascii_lowercase(), value.trim())).collect();
            let header = |name| fields.iter().find(|(key, _)| key == name).map(|(_, value)| *value);
            assert_eq!(header("user-agent"), Some(expected_agent.as_str()));
            assert_eq!(header("origin"), Some("https://discord.com"));
            assert_eq!(header("sec-websocket-version"), Some("13"));
            assert_eq!(header("upgrade"), Some("websocket"));
            assert_eq!(header("connection"), Some("upgrade"));
            assert!(request.contains("\r\nUpgrade: websocket\r\n"));
            assert_eq!(header("accept-encoding"), Some("gzip, deflate, br, zstd"));
            assert_eq!(header("sec-fetch-mode"), None);
            assert_eq!(header("sec-ch-ua"), None);
            let key = header("sec-websocket-key").expect("upgrade has a key");
            let response = format!("HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {}\r\n\r\n", derive_accept_key(key.as_bytes()));
            // Send Hello alongside the upgrade, as a Gateway can do. It must
            // survive handing the upgraded transport to the existing parser.
            let mut response = response.into_bytes();
            response.extend_from_slice(b"\x81\x05hello");
            socket.write_all(&response).await.expect("upgrade and first frame send");
            let mut ws = WebSocketStream::from_raw_socket(socket, Role::Server, None).await;
            assert_eq!(ws.next().await.expect("client sends a frame").expect("frame parses"), Message::text("identify"));
            ws.send(Message::text("ready")).await.expect("reply sends");
        });
        let mut request = format!("ws://{address}/?v=9&encoding=json").into_client_request().expect("WS request builds");
        request.headers_mut().extend(discord_gateway_headers(&fingerprint));
        let config = WebSocketConfig::default().max_message_size(Some(64 * 1024 * 1024));
        let mut ws = connect_websocket(request, config).await.expect("Chrome upgrade succeeds");
        assert_eq!(ws.get_config().max_message_size, Some(64 * 1024 * 1024));
        assert_eq!(ws.next().await.expect("Hello arrives").expect("Hello parses"), Message::text("hello"));
        ws.send(Message::text("identify")).await.expect("client frame sends");
        assert_eq!(ws.next().await.expect("Ready arrives").expect("Ready parses"), Message::text("ready"));
        server.await.expect("server finishes");
    }).await.expect("local WS exchange finishes");
}
