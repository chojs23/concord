use super::*;
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    thread,
};

#[test]
fn parses_build_number_from_sentry_asset() {
    let js = r#"e.exports={buildNumber","573410",version:"1.0.0"}"#;
    assert_eq!(parse_build_number(js), Some(573_410));
    assert_eq!(parse_build_number("no build number here"), None);
}

#[test]
fn finds_sentry_asset_path_in_app_html() {
    let html = r#"<script src="/assets/sentry.a1b2c3d4.js" crossorigin defer></script>"#;
    assert_eq!(
        find_sentry_asset_path(html).as_deref(),
        Some("/assets/sentry.a1b2c3d4.js")
    );
    assert_eq!(find_sentry_asset_path("no sentry asset here"), None);
}

#[test]
fn rest_headers_match_web_fingerprint_plan() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let headers = discord_rest_headers(&fingerprint);

    assert_eq!(
        headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok()),
        Some(fingerprint.user_agent.as_str())
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
        Some(accept_language(&fingerprint.system_locale).as_str())
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
        Some(fingerprint.system_locale.as_str())
    );
    assert_eq!(
        headers
            .get("X-Discord-Timezone")
            .and_then(|value| value.to_str().ok()),
        Some(fingerprint.timezone.as_str())
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
fn super_properties_are_base64_encoded_web_fields() {
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let encoded = build_super_properties(&fingerprint);
    let decoded = STANDARD
        .decode(encoded)
        .expect("super properties should decode from base64");
    let value: Value =
        serde_json::from_slice(&decoded).expect("super properties should decode as json");

    assert_eq!(value["os"], fingerprint.os);
    assert_eq!(value["device"], "");
    assert_eq!(value["browser"], CLIENT_BROWSER);
    assert_eq!(value["release_channel"], "stable");
    assert_eq!(value["os_arch"], fingerprint.os_arch);
    assert_eq!(value["system_locale"], fingerprint.system_locale);
    assert_eq!(value["has_client_mods"], false);
    assert_eq!(value["browser_user_agent"], fingerprint.user_agent);
    assert_eq!(value["browser_version"], CLIENT_BROWSER_VERSION);
    assert_eq!(value["client_build_number"], CLIENT_BUILD_NUMBER);
    assert!(value["client_event_source"].is_null());
    assert_uuid_field(&value, "launch_signature");
    assert_uuid_field(&value, "client_launch_id");
    assert_uuid_field(&value, "client_heartbeat_session_id");
    assert_eq!(value["client_app_state"], "unfocused");
    assert_eq!(value["referrer"], "");
    assert_eq!(value["referrer_current"], "");
    assert_eq!(value["referring_domain"], "");
    assert_eq!(value["referring_domain_current"], "");
    assert!(value.get("client_version").is_none());
    assert!(value.get("native_build_number").is_none());
}

#[test]
fn launch_signature_applies_discord_mask() {
    let signature = generate_launch_signature();
    let uuid = Uuid::parse_str(&signature).expect("launch signature should be a UUID");
    let bytes = uuid.as_bytes();

    for (index, mask) in [
        (1, 0b1000_0000),
        (2, 0b0001_0000),
        (3, 0b0001_0000),
        (4, 0b0000_1000),
        (5, 0b0001_0000),
        (6, 0b0000_1000),
        (8, 0b0010_0000),
        (9, 0b1000_0001),
        (11, 0b0100_0000),
        (12, 0b0000_0001),
        (14, 0b0000_1000),
    ] {
        assert_eq!(bytes[index] & mask, 0);
    }
}

fn assert_uuid_field(value: &Value, field: &str) {
    let raw = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} should be a string"));
    Uuid::parse_str(raw).unwrap_or_else(|_| panic!("{field} should be a UUID"));
}

#[test]
fn rest_client_sends_default_headers_and_replays_cookies() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
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
                .any(|line| line.eq_ignore_ascii_case("Accept-Encoding: gzip, deflate, br, zstd")),
            "default Accept-Encoding header should be sent"
        );
        assert!(
            headers.iter().any(|line| line
                .to_ascii_lowercase()
                .starts_with("user-agent: mozilla/5.0")),
            "web user agent should be sent"
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
        let client = discord_http_client(&fingerprint);
        client
            .get(format!("http://{address}/first"))
            .headers(discord_rest_headers(&fingerprint))
            .send()
            .await
            .expect("first local request should succeed")
            .error_for_status()
            .expect("first local response should be successful");
        client
            .get(format!("http://{address}/second"))
            .headers(discord_rest_headers(&fingerprint))
            .send()
            .await
            .expect("second local request should succeed")
            .error_for_status()
            .expect("second local response should be successful");
    });
    server.join().expect("test server should finish");
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
