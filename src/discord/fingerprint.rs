use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT,
};
use serde::Serialize;
use uuid::Uuid;

use super::auth_http::DISCORD_ORIGIN;

/// Fallback used only when the live build number cannot be fetched at startup.
pub(super) const CLIENT_BUILD_NUMBER: u64 = 573_410;
static CLIENT_BUILD_NUMBER_CACHE: OnceLock<u64> = OnceLock::new();

pub(super) const CLIENT_BROWSER: &str = "Chrome";
pub(super) const CLIENT_BROWSER_VERSION: &str = "143.0.0.0";

const DISCORD_CHANNELS_REFERER: &str = "https://discord.com/channels/@me";
const ACCEPT_LANGUAGE_VALUE: &str = "en-US,en;q=0.9";
const DISCORD_LOCALE: &str = "en-US";
const SYSTEM_LOCALE: &str = "en-US";
const DISCORD_TIMEZONE: &str = "America/New_York";

#[derive(Serialize)]
struct SuperProperties {
    os: &'static str,
    device: &'static str,
    browser: &'static str,
    release_channel: &'static str,
    os_version: String,
    os_arch: &'static str,
    system_locale: &'static str,
    has_client_mods: bool,
    browser_user_agent: String,
    browser_version: &'static str,
    client_build_number: u64,
    client_event_source: Option<String>,
    launch_signature: String,
    client_launch_id: String,
    client_heartbeat_session_id: String,
    client_app_state: &'static str,
    referrer: &'static str,
    referrer_current: &'static str,
    referring_domain: &'static str,
    referring_domain_current: &'static str,
}

struct ClientIdentity {
    os: &'static str,
    os_version: String,
    os_arch: &'static str,
    user_agent: String,
}

pub(super) fn discord_rest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .default_headers(discord_rest_headers())
        .build()
        .expect("static Discord REST client configuration is valid")
}

pub(super) fn discord_rest_headers() -> HeaderMap {
    let identity = client_identity();
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&identity.user_agent).expect("web user agent is valid"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static(ACCEPT_LANGUAGE_VALUE),
    );
    headers.insert(ORIGIN, HeaderValue::from_static(DISCORD_ORIGIN));
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_CHANNELS_REFERER));
    headers.insert("Priority", HeaderValue::from_static("u=1, i"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert("X-Discord-Locale", HeaderValue::from_static(DISCORD_LOCALE));
    headers.insert(
        "X-Discord-Timezone",
        HeaderValue::from_static(DISCORD_TIMEZONE),
    );
    headers.insert(
        "X-Debug-Options",
        HeaderValue::from_static("bugReporterEnabled"),
    );
    headers.insert(
        "X-Super-Properties",
        HeaderValue::from_str(&build_super_properties(&identity))
            .expect("base64 super properties are a valid header value"),
    );
    headers
}

fn build_super_properties(identity: &ClientIdentity) -> String {
    let properties = SuperProperties {
        os: identity.os,
        device: "",
        browser: CLIENT_BROWSER,
        release_channel: "stable",
        os_version: identity.os_version.clone(),
        os_arch: identity.os_arch,
        system_locale: SYSTEM_LOCALE,
        has_client_mods: false,
        browser_user_agent: identity.user_agent.clone(),
        browser_version: CLIENT_BROWSER_VERSION,
        client_build_number: client_build_number(),
        client_event_source: None,
        launch_signature: generate_launch_signature(),
        client_launch_id: Uuid::new_v4().to_string(),
        client_heartbeat_session_id: Uuid::new_v4().to_string(),
        client_app_state: "unfocused",
        referrer: "",
        referrer_current: "",
        referring_domain: "",
        referring_domain_current: "",
    };
    let raw = serde_json::to_vec(&properties).expect("super properties serialize");
    STANDARD.encode(raw)
}

pub(super) fn client_build_number() -> u64 {
    CLIENT_BUILD_NUMBER_CACHE
        .get()
        .copied()
        .unwrap_or(CLIENT_BUILD_NUMBER)
}

/// Aligns our advertised build number with Discord's live value. A stale build
/// number is a self-bot signal that can get accounts flagged. Best-effort. On
/// any failure the compiled fallback stays in place.
pub(crate) async fn refresh_client_build_number() {
    if CLIENT_BUILD_NUMBER_CACHE.get().is_some() {
        return;
    }
    match fetch_client_build_number().await {
        Some(build) => {
            let _ = CLIENT_BUILD_NUMBER_CACHE.set(build);
        }
        None => crate::logging::debug(
            "fingerprint",
            "could not fetch Discord build number; using compiled fallback",
        ),
    }
}

async fn fetch_client_build_number() -> Option<u64> {
    let client = discord_rest_client();
    let app_html = client
        .get(format!("{DISCORD_ORIGIN}/app"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let asset_path = find_sentry_asset_path(&app_html)?;
    let asset = client
        .get(format!("{DISCORD_ORIGIN}{asset_path}"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    parse_build_number(&asset)
}

fn find_sentry_asset_path(html: &str) -> Option<String> {
    const PREFIX: &str = "/assets/sentry";
    const SUFFIX: &str = ".js";
    let start = html.find(PREFIX)?;
    let rest = &html[start..];
    let end = rest.find(SUFFIX)? + SUFFIX.len();
    Some(rest[..end].to_owned())
}

fn parse_build_number(js: &str) -> Option<u64> {
    const MARKER: &str = "buildNumber\",\"";
    let start = js.find(MARKER)? + MARKER.len();
    let digits: String = js[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse::<u64>().ok()
}

fn client_identity() -> ClientIdentity {
    let os = operating_system();
    let os_version = operating_system_version();
    let os_arch = operating_system_arch();
    let user_agent = web_user_agent(os, &os_version, os_arch);
    ClientIdentity {
        os,
        os_version,
        os_arch,
        user_agent,
    }
}

pub(super) fn discord_web_os() -> &'static str {
    operating_system()
}

pub(super) fn discord_web_os_version() -> String {
    operating_system_version()
}

pub(super) fn discord_web_user_agent() -> String {
    client_identity().user_agent
}

fn web_user_agent(os: &str, os_version: &str, os_arch: &str) -> String {
    let platform = match os {
        "Windows" => "Windows NT 10.0; Win64; x64".to_owned(),
        "Mac OS X" => format!("Macintosh; Intel Mac OS X {}", os_version.replace('.', "_")),
        _ if os_arch == "arm64" => "X11; Linux aarch64".to_owned(),
        _ => "X11; Linux x86_64".to_owned(),
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/{CLIENT_BROWSER_VERSION} Safari/537.36"
    )
}

fn generate_launch_signature() -> String {
    let mask = [
        0b1111_1111,
        0b0111_1111,
        0b1110_1111,
        0b1110_1111,
        0b1111_0111,
        0b1110_1111,
        0b1111_0111,
        0b1111_1111,
        0b1101_1111,
        0b0111_1110,
        0b1111_1111,
        0b1011_1111,
        0b1111_1110,
        0b1111_1111,
        0b1111_0111,
        0b1111_1111,
    ];
    let mut bytes = *Uuid::new_v4().as_bytes();
    for (byte, mask) in bytes.iter_mut().zip(mask) {
        *byte &= mask;
    }
    Uuid::from_bytes(bytes).to_string()
}

fn operating_system() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Mac OS X"
    } else {
        "Linux"
    }
}

fn operating_system_version() -> String {
    if cfg!(target_os = "linux") {
        Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|version| version.trim().to_owned())
            .filter(|version| !version.is_empty())
            .unwrap_or_default()
    } else {
        String::new()
    }
}

fn operating_system_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        arch => arch,
    }
}

#[cfg(test)]
mod tests;
