use std::{process::Command, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue, ORIGIN,
    PRAGMA, REFERER, USER_AGENT,
};
use serde::Serialize;
use uuid::Uuid;

use super::auth_http::DISCORD_ORIGIN;

/// Fallback used only when the live build number cannot be fetched at startup.
pub(super) const CLIENT_BUILD_NUMBER: u64 = 573_410;
pub(super) const CLIENT_BROWSER: &str = "Chrome";
pub(super) const CLIENT_BROWSER_VERSION: &str = "143.0.0.0";

const DISCORD_CHANNELS_REFERER: &str = "https://discord.com/channels/@me";

#[derive(Clone, Debug)]
pub(crate) struct ClientFingerprint {
    pub(super) os: &'static str,
    pub(super) os_version: String,
    pub(super) os_arch: &'static str,
    pub(super) system_locale: String,
    pub(super) timezone: String,
    pub(super) user_agent: String,
    pub(super) client_build_number: u64,
    launch_signature: String,
    client_launch_id: String,
    client_heartbeat_session_id: String,
}

impl ClientFingerprint {
    pub(super) fn new(client_build_number: u64) -> Self {
        let os = operating_system();
        let os_version = operating_system_version();
        let os_arch = operating_system_arch();
        Self {
            os,
            user_agent: web_user_agent(os, &os_version, os_arch),
            os_version,
            os_arch,
            system_locale: system_locale(),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_owned()),
            client_build_number,
            launch_signature: generate_launch_signature(),
            client_launch_id: Uuid::new_v4().to_string(),
            client_heartbeat_session_id: Uuid::new_v4().to_string(),
        }
    }
}

#[derive(Serialize)]
struct SuperProperties<'a> {
    os: &'a str,
    device: &'static str,
    browser: &'static str,
    release_channel: &'static str,
    os_version: &'a str,
    os_arch: &'a str,
    system_locale: &'a str,
    has_client_mods: bool,
    browser_user_agent: &'a str,
    browser_version: &'static str,
    client_build_number: u64,
    client_event_source: Option<String>,
    launch_signature: &'a str,
    client_launch_id: &'a str,
    client_heartbeat_session_id: &'a str,
    client_app_state: &'static str,
    referrer: &'static str,
    referrer_current: &'static str,
    referring_domain: &'static str,
    referring_domain_current: &'static str,
}

/// Creates the login-session fingerprint after reading Discord's current web
/// build. REST and Gateway keep this same value for the full login session.
pub(crate) async fn load_client_fingerprint() -> Arc<ClientFingerprint> {
    let bootstrap = Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER));
    let client = discord_http_client(&bootstrap);
    let client_build_number = match fetch_client_build_number(&client).await {
        Some(build) => build,
        None => {
            crate::logging::debug(
                "fingerprint",
                "could not fetch Discord build number; using compiled fallback",
            );
            CLIENT_BUILD_NUMBER
        }
    };
    Arc::new(ClientFingerprint::new(client_build_number))
}

pub(super) fn discord_http_client(fingerprint: &ClientFingerprint) -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .default_headers(discord_browser_headers(fingerprint))
        .build()
        .expect("static Discord REST client configuration is valid")
}

fn discord_browser_headers(fingerprint: &ClientFingerprint) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&fingerprint.user_agent).expect("web user agent is valid"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_str(&accept_language(&fingerprint.system_locale))
            .expect("system locale is a valid header value"),
    );
    headers
}

pub(super) fn discord_rest_headers(fingerprint: &ClientFingerprint) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&fingerprint.user_agent).expect("web user agent is valid"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(
        ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_str(&accept_language(&fingerprint.system_locale))
            .expect("system locale is a valid header value"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static(DISCORD_ORIGIN));
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_CHANNELS_REFERER));
    headers.insert("Priority", HeaderValue::from_static("u=1, i"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert(
        "X-Discord-Locale",
        HeaderValue::from_str(&fingerprint.system_locale)
            .expect("system locale is a valid header value"),
    );
    headers.insert(
        "X-Discord-Timezone",
        HeaderValue::from_str(&fingerprint.timezone).expect("timezone is a valid header value"),
    );
    headers.insert(
        "X-Debug-Options",
        HeaderValue::from_static("bugReporterEnabled"),
    );
    headers.insert(
        "X-Super-Properties",
        HeaderValue::from_str(&build_super_properties(fingerprint))
            .expect("base64 super properties are a valid header value"),
    );
    headers
}

pub(super) fn discord_gateway_headers(fingerprint: &ClientFingerprint) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&fingerprint.user_agent).expect("web user agent is valid"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_str(&accept_language(&fingerprint.system_locale))
            .expect("system locale is a valid header value"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static(DISCORD_ORIGIN));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers
}

fn build_super_properties(fingerprint: &ClientFingerprint) -> String {
    let properties = SuperProperties {
        os: fingerprint.os,
        device: "",
        browser: CLIENT_BROWSER,
        release_channel: "stable",
        os_version: &fingerprint.os_version,
        os_arch: fingerprint.os_arch,
        system_locale: &fingerprint.system_locale,
        has_client_mods: false,
        browser_user_agent: &fingerprint.user_agent,
        browser_version: CLIENT_BROWSER_VERSION,
        client_build_number: fingerprint.client_build_number,
        client_event_source: None,
        launch_signature: &fingerprint.launch_signature,
        client_launch_id: &fingerprint.client_launch_id,
        client_heartbeat_session_id: &fingerprint.client_heartbeat_session_id,
        client_app_state: "unfocused",
        referrer: "",
        referrer_current: "",
        referring_domain: "",
        referring_domain_current: "",
    };
    let raw = serde_json::to_vec(&properties).expect("super properties serialize");
    STANDARD.encode(raw)
}

async fn fetch_client_build_number(client: &reqwest::Client) -> Option<u64> {
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

fn system_locale() -> String {
    sys_locale::get_locale()
        .map(|locale| {
            locale
                .split(['.', '@'])
                .next()
                .unwrap_or("en-US")
                .replace('_', "-")
        })
        .filter(|locale| !locale.is_empty() && HeaderValue::from_str(locale).is_ok())
        .unwrap_or_else(|| "en-US".to_owned())
}

pub(super) fn accept_language(locale: &str) -> String {
    let language = locale.split('-').next().unwrap_or(locale);
    if locale == language {
        format!("{locale},en;q=0.9")
    } else {
        format!("{locale},{language};q=0.9,en;q=0.8")
    }
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
