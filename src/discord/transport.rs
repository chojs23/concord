use std::sync::LazyLock;

use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        handshake::client::Request,
        protocol::{Role, WebSocketConfig},
    },
};
use wreq::{
    ClientBuilder, IntoEmulation,
    header::{ACCEPT_ENCODING, HeaderMap, HeaderValue, OrigHeaderMap, USER_AGENT},
};
use wreq_util::{Emulation, Platform, Profile};

// Keep the wire settings and browser metadata in one fixed profile. Endpoint
// headers remain explicit so navigation defaults do not leak into API or WS calls.
static CHROME: LazyLock<wreq::Emulation> = LazyLock::new(|| {
    let platform = if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOS
    } else {
        Platform::Linux
    };
    let mut profile = Emulation::builder()
        .profile(Profile::Chrome149)
        .platform(platform)
        .build()
        .into_emulation();
    let mut headers = HeaderMap::new();
    for name in [
        "user-agent",
        "sec-ch-ua",
        "sec-ch-ua-mobile",
        "sec-ch-ua-platform",
    ] {
        if let Some(value) = profile.headers.get(name) {
            headers.insert(name, value.clone());
        }
    }
    profile.headers = headers;
    profile
});

pub(super) fn browser_headers() -> HeaderMap {
    CHROME.headers.clone()
}

pub(super) fn user_agent() -> &'static str {
    CHROME.headers[USER_AGENT]
        .to_str()
        .expect("Chrome profile has a text user agent")
}

pub(super) fn browser_version() -> &'static str {
    user_agent()
        .split_once("Chrome/")
        .and_then(|(_, suffix)| suffix.split_whitespace().next())
        .expect("Chrome profile user agent has a version")
}

pub(super) fn client_builder() -> Result<ClientBuilder, String> {
    builder(CHROME.clone())
}

fn builder(profile: wreq::Emulation) -> Result<ClientBuilder, String> {
    Ok(crate::support::tls::http_client_builder()?.emulation(profile))
}

pub(super) type DiscordWebSocket = WebSocketStream<wreq::Upgraded>;

pub(super) async fn connect_websocket(
    mut request: Request,
    config: WebSocketConfig,
) -> Result<DiscordWebSocket, String> {
    let mut profile = CHROME.clone();
    profile.headers.clear();
    // Do not advertise permessage-deflate because the existing frame parser does
    // not implement it. Gateway payload compression still uses zlib-stream.
    // HTTP/1 upgrades have their own header layout, unlike HTTP/2 API calls.
    profile.orig_headers = OrigHeaderMap::new();
    for name in [
        "Host",
        "Connection",
        "Pragma",
        "Cache-Control",
        "User-Agent",
        "Upgrade",
        "Origin",
        "Sec-WebSocket-Version",
        "Accept-Encoding",
        "Accept-Language",
        "Sec-WebSocket-Key",
    ] {
        profile.orig_headers.insert(name);
    }
    let client = builder(profile)?
        .http1_only()
        .redirect(wreq::redirect::Policy::none())
        .build()
        .map_err(|error| format!("build Discord WebSocket client: {error}"))?;
    request.headers_mut().insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    let response = client
        .websocket(request.uri().to_string())
        .headers(request.headers().clone())
        .send()
        .await
        .map_err(|error| format!("connect Discord WebSocket: {error}"))?;
    let upgraded = response
        .into_websocket()
        .await
        .map_err(|error| format!("upgrade Discord WebSocket: {error}"))?
        .into_inner();
    // Retain the existing frame parser and limits after wreq validates the HTTP
    // upgrade. Upgraded owns any bytes read ahead with the handshake response.
    Ok(WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await)
}

#[cfg(test)]
mod tests;
