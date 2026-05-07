use std::process::Command;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT,
};
use serde::Serialize;
use uuid::Uuid;

use super::auth_http::DISCORD_ORIGIN;

pub(super) const CLIENT_BUILD_NUMBER: u64 = 536_121;
pub(super) const CLIENT_VERSION: &str = "0.0.397";

const CHROME_VERSION: &str = "142.0.7444.175";
const ELECTRON_VERSION: &str = "38.2.0";
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
    client_version: &'static str,
    os_version: String,
    os_arch: &'static str,
    system_locale: &'static str,
    has_client_mods: bool,
    browser_user_agent: String,
    browser_version: &'static str,
    client_build_number: u64,
    native_build_number: Option<u64>,
    client_event_source: Option<String>,
    client_app_state: &'static str,
    launch_signature: String,
    client_launch_id: String,
    client_heartbeat_session_id: String,
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
        HeaderValue::from_str(&identity.user_agent).expect("desktop user agent is valid"),
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
        browser: "Discord Client",
        release_channel: "stable",
        client_version: CLIENT_VERSION,
        os_version: identity.os_version.clone(),
        os_arch: identity.os_arch,
        system_locale: SYSTEM_LOCALE,
        has_client_mods: false,
        browser_user_agent: identity.user_agent.clone(),
        browser_version: "",
        client_build_number: CLIENT_BUILD_NUMBER,
        native_build_number: None,
        client_event_source: None,
        client_app_state: "focused",
        launch_signature: generate_launch_signature(),
        client_launch_id: Uuid::new_v4().to_string(),
        client_heartbeat_session_id: Uuid::new_v4().to_string(),
        referrer: "",
        referrer_current: "",
        referring_domain: "",
        referring_domain_current: "",
    };
    let raw = serde_json::to_vec(&properties).expect("super properties serialize");
    STANDARD.encode(raw)
}

fn client_identity() -> ClientIdentity {
    let os = operating_system();
    let os_version = operating_system_version();
    let os_arch = operating_system_arch();
    let user_agent = desktop_user_agent(os, &os_version, os_arch);
    ClientIdentity {
        os,
        os_version,
        os_arch,
        user_agent,
    }
}

fn desktop_user_agent(os: &str, os_version: &str, os_arch: &str) -> String {
    let platform = match os {
        "Windows" => "Windows NT 10.0; Win64; x64".to_owned(),
        "Mac OS X" => format!("Macintosh; Intel Mac OS X {}", os_version.replace('.', "_")),
        _ if os_arch == "arm64" => "X11; Linux aarch64".to_owned(),
        _ => "X11; Linux x86_64".to_owned(),
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) \
         discord/{CLIENT_VERSION} Chrome/{CHROME_VERSION} Electron/{ELECTRON_VERSION} \
         Safari/537.36"
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
mod tests {
    use super::*;
    use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, ORIGIN, REFERER, USER_AGENT};
    use serde_json::Value;
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    #[test]
    fn rest_headers_match_desktop_fingerprint_plan() {
        let headers = discord_rest_headers();
        let identity = client_identity();

        assert_eq!(
            headers
                .get(USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some(identity.user_agent.as_str())
        );
        assert_eq!(
            headers.get(ACCEPT).and_then(|value| value.to_str().ok()),
            Some("*/*")
        );
        assert_eq!(
            headers
                .get(ACCEPT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip, deflate, br, zstd")
        );
        assert_eq!(
            headers
                .get(ACCEPT_LANGUAGE)
                .and_then(|value| value.to_str().ok()),
            Some(ACCEPT_LANGUAGE_VALUE)
        );
        assert_eq!(
            headers.get(ORIGIN).and_then(|value| value.to_str().ok()),
            Some(DISCORD_ORIGIN)
        );
        assert_eq!(
            headers.get(REFERER).and_then(|value| value.to_str().ok()),
            Some(DISCORD_CHANNELS_REFERER)
        );
        assert_eq!(
            headers
                .get("Priority")
                .and_then(|value| value.to_str().ok()),
            Some("u=1, i")
        );
        assert_eq!(
            headers
                .get("Sec-Fetch-Dest")
                .and_then(|value| value.to_str().ok()),
            Some("empty")
        );
        assert_eq!(
            headers
                .get("Sec-Fetch-Mode")
                .and_then(|value| value.to_str().ok()),
            Some("cors")
        );
        assert_eq!(
            headers
                .get("Sec-Fetch-Site")
                .and_then(|value| value.to_str().ok()),
            Some("same-origin")
        );
        assert_eq!(
            headers
                .get("X-Discord-Locale")
                .and_then(|value| value.to_str().ok()),
            Some(DISCORD_LOCALE)
        );
        assert_eq!(
            headers
                .get("X-Discord-Timezone")
                .and_then(|value| value.to_str().ok()),
            Some(DISCORD_TIMEZONE)
        );
        assert_eq!(
            headers
                .get("X-Debug-Options")
                .and_then(|value| value.to_str().ok()),
            Some("bugReporterEnabled")
        );
        assert!(headers.get("X-Super-Properties").is_some());
    }

    #[test]
    fn super_properties_are_base64_encoded_desktop_fields() {
        let identity = client_identity();
        let encoded = build_super_properties(&identity);
        let decoded = STANDARD
            .decode(encoded)
            .expect("super properties should decode from base64");
        let value: Value =
            serde_json::from_slice(&decoded).expect("super properties should decode as json");

        assert_eq!(value["os"], identity.os);
        assert_eq!(value["device"], "");
        assert_eq!(value["browser"], "Discord Client");
        assert_eq!(value["release_channel"], "stable");
        assert_eq!(value["client_version"], CLIENT_VERSION);
        assert_eq!(value["os_arch"], identity.os_arch);
        assert_eq!(value["system_locale"], SYSTEM_LOCALE);
        assert_eq!(value["has_client_mods"], false);
        assert_eq!(value["browser_user_agent"], identity.user_agent);
        assert_eq!(value["browser_version"], "");
        assert_eq!(value["client_build_number"], CLIENT_BUILD_NUMBER);
        assert!(value["native_build_number"].is_null());
        assert!(value["client_event_source"].is_null());
        assert_eq!(value["client_app_state"], "focused");
        assert_uuid_field(&value, "launch_signature");
        assert_uuid_field(&value, "client_launch_id");
        assert_uuid_field(&value, "client_heartbeat_session_id");
        assert_eq!(value["referrer"], "");
        assert_eq!(value["referrer_current"], "");
        assert_eq!(value["referring_domain"], "");
        assert_eq!(value["referring_domain_current"], "");
    }

    #[test]
    fn rest_client_sends_default_headers_and_replays_cookies() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener
            .local_addr()
            .expect("test server should have an address");
        let server = thread::spawn(move || {
            let first_request = accept_request(&listener);
            let (first_request, _headers) = read_headers(first_request);
            respond(
                first_request,
                "HTTP/1.1 200 OK\r\nSet-Cookie: __dcfduid=test-cookie; Path=/\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok",
            );

            let second_request = accept_request(&listener);
            let (second_request, headers) = read_headers(second_request);
            assert!(
                headers
                    .iter()
                    .any(|line| line
                        .eq_ignore_ascii_case("Accept-Encoding: gzip, deflate, br, zstd")),
                "default Accept-Encoding header should be sent"
            );
            assert!(
                headers.iter().any(|line| line
                    .to_ascii_lowercase()
                    .starts_with("user-agent: mozilla/5.0")),
                "desktop user agent should be sent"
            );
            assert!(
                headers.iter().any(|line| {
                    line.to_ascii_lowercase().starts_with("cookie:")
                        && line.contains("__dcfduid=test-cookie")
                }),
                "cookie jar should replay the first response cookie"
            );
            respond(
                second_request,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok",
            );
        });

        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
        runtime.block_on(async {
            let client = discord_rest_client();
            client
                .get(format!("http://{address}/first"))
                .send()
                .await
                .expect("first local request should succeed")
                .error_for_status()
                .expect("first local response should be successful");
            client
                .get(format!("http://{address}/second"))
                .send()
                .await
                .expect("second local request should succeed")
                .error_for_status()
                .expect("second local response should be successful");
        });
        server.join().expect("test server should finish");
    }

    #[test]
    fn launch_signature_applies() {
        let signature = generate_launch_signature();
        let uuid = Uuid::parse_str(&signature).expect("launch signature should be a UUID");
        let bytes = uuid.as_bytes();

        assert_eq!(bytes[1] & 0b1000_0000, 0);
        assert_eq!(bytes[2] & 0b0001_0000, 0);
        assert_eq!(bytes[3] & 0b0001_0000, 0);
        assert_eq!(bytes[4] & 0b0000_1000, 0);
        assert_eq!(bytes[5] & 0b0001_0000, 0);
        assert_eq!(bytes[6] & 0b0000_1000, 0);
        assert_eq!(bytes[8] & 0b0010_0000, 0);
        assert_eq!(bytes[9] & 0b1000_0000, 0);
        assert_eq!(bytes[9] & 0b0000_0001, 0);
        assert_eq!(bytes[11] & 0b0100_0000, 0);
        assert_eq!(bytes[12] & 0b0000_0001, 0);
        assert_eq!(bytes[14] & 0b0000_1000, 0);
    }

    fn assert_uuid_field(value: &Value, field: &str) {
        let raw = value[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} should be a string"));
        Uuid::parse_str(raw).unwrap_or_else(|_| panic!("{field} should be a UUID"));
    }

    fn accept_request(listener: &TcpListener) -> TcpStream {
        listener
            .accept()
            .expect("test server should accept a request")
            .0
    }

    fn read_headers(stream: TcpStream) -> (TcpStream, Vec<String>) {
        let mut reader = BufReader::new(stream);
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("test server should read request headers");
            let line = line.trim_end_matches(['\r', '\n']).to_owned();
            if line.is_empty() {
                break;
            }
            lines.push(line);
        }
        (reader.into_inner(), lines)
    }

    fn respond(mut stream: TcpStream, response: &str) {
        stream
            .write_all(response.as_bytes())
            .expect("test server should write response");
    }
}
