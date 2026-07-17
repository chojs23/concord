use super::*;
use crate::discord::auth_http::DiscordAuthSession;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, ORIGIN, PRAGMA, REFERER, USER_AGENT,
};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::Duration,
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
fn rest_headers_and_persisted_identifiers_match_web_fingerprint_plan() {
    let mut fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
    let mut identifiers = SessionIdentifiers {
        fingerprint: Some("anonymous-fingerprint".to_owned()),
        installation: Some("installation-id".to_owned()),
    };
    fingerprint.apply_session_identifiers(identifiers.clone());
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
    assert!(headers.get(ORIGIN).is_none());
    assert_eq!(
        headers.get(REFERER).and_then(|value| value.to_str().ok()),
        Some(DISCORD_CHANNELS_REFERER)
    );
    assert_eq!(
        discord_experiments_headers(&fingerprint)
            .get(REFERER)
            .and_then(|value| value.to_str().ok()),
        Some(DISCORD_ROOT_REFERER)
    );
    assert_eq!(
        discord_experiments_headers(&fingerprint)
            .get("X-Fingerprint")
            .and_then(|value| value.to_str().ok()),
        Some("anonymous-fingerprint")
    );
    assert_eq!(
        discord_experiments_headers(&fingerprint)
            .get("X-Installation-ID")
            .and_then(|value| value.to_str().ok()),
        Some("installation-id")
    );
    assert_eq!(
        headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers.get(PRAGMA).and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers
            .get("Priority")
            .and_then(|value| value.to_str().ok()),
        Some("u=1, i")
    );
    assert_eq!(
        headers
            .get("Sec-CH-UA")
            .and_then(|value| value.to_str().ok()),
        Some("\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"150\", \"Google Chrome\";v=\"150\"")
    );
    assert_eq!(
        headers
            .get("Sec-CH-UA-Mobile")
            .and_then(|value| value.to_str().ok()),
        Some("?0")
    );
    assert_eq!(
        headers
            .get("Sec-CH-UA-Platform")
            .and_then(|value| value.to_str().ok()),
        Some(if cfg!(target_os = "macos") {
            "\"macOS\""
        } else if cfg!(target_os = "windows") {
            "\"Windows\""
        } else {
            "\"Linux\""
        })
    );
    assert!(headers.get("Sec-GPC").is_none());
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
        Some(fingerprint.timezone.as_str())
    );
    assert_eq!(
        headers
            .get("X-Debug-Options")
            .and_then(|value| value.to_str().ok()),
        Some("bugReporterEnabled")
    );
    assert!(headers.get("X-Super-Properties").is_some());
    assert!(headers.get("X-Fingerprint").is_none());
    assert_eq!(
        headers
            .get("X-Installation-ID")
            .and_then(|value| value.to_str().ok()),
        Some("installation-id")
    );

    assert_eq!(
        discord_channel_referer(Some(Id::new(1)), Id::new(2)),
        "https://discord.com/channels/1/2"
    );
    assert_eq!(
        discord_channel_referer(None, Id::new(2)),
        "https://discord.com/channels/@me/2"
    );

    let state_dir = tempfile::tempdir().expect("temporary state directory should be created");
    let state_path = state_dir.path().join("discord-browser.toml");
    save_session_identifiers_to_path(&state_path, &identifiers)
        .expect("browser identifiers should be persisted");
    assert_eq!(
        load_session_identifiers_from_path(&state_path)
            .expect("browser identifiers should be loaded"),
        Some(identifiers.clone())
    );

    let cookies = discord_cookies(
        Some(HeaderValue::from_static("__dcfduid=test; locale=ko-KR")),
        &Url::parse("https://discord.com/api/v9/channels/1").expect("Discord URL is valid"),
    )
    .expect("Discord locale cookie should be present");
    assert_eq!(
        cookies.to_str().expect("cookies are valid text"),
        "__dcfduid=test; locale=en-US"
    );
    let cookie_names = cookie_names_for_log(Some(&cookies));
    assert_eq!(cookie_names, "__dcfduid, locale");
    assert!(!cookie_names.contains("test"));

    identifiers.merge_fetched(SessionIdentifiers {
        fingerprint: None,
        installation: Some("replacement-installation-id".to_owned()),
    });
    assert_eq!(
        identifiers,
        SessionIdentifiers {
            fingerprint: Some("anonymous-fingerprint".to_owned()),
            installation: Some("replacement-installation-id".to_owned()),
        }
    );
    fingerprint.set_installation_id_for_test("ready-installation-id");
    assert_eq!(
        discord_rest_headers(&fingerprint)
            .get("X-Installation-ID")
            .and_then(|value| value.to_str().ok()),
        Some("ready-installation-id")
    );
    assert_eq!(
        DISCORD_EXPERIMENTS_URL,
        "https://discord.com/api/v9/experiments?with_guild_experiments=true"
    );
    assert_eq!(
        DISCORD_APEX_EXPERIMENTS_URL,
        "https://discord.com/api/v9/apex/experiments?surface=2"
    );
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
    assert_eq!(value["system_locale"], fingerprint.system_locale);
    assert_eq!(value["has_client_mods"], false);
    assert_eq!(value["browser_user_agent"], fingerprint.user_agent);
    assert_eq!(value["browser_version"], CLIENT_BROWSER_VERSION);
    assert_eq!(value["os_version"], fingerprint.os_version);
    assert_eq!(value["client_build_number"], CLIENT_BUILD_NUMBER);
    assert!(value["client_event_source"].is_null());
    assert_uuid_field(&value, "launch_signature");
    assert_uuid_field(&value, "client_launch_id");
    assert_eq!(value["client_app_state"], "focused");
    assert_eq!(value["referrer"], "");
    assert_eq!(value["referring_domain"], "");
    assert_eq!(value["referrer_current"], DISCORD_REFERRER_CURRENT);
    assert_eq!(
        value["referring_domain_current"],
        DISCORD_REFERRING_DOMAIN_CURRENT
    );
    assert!(value.get("os_arch").is_none());
    assert_uuid_field(&value, "client_heartbeat_session_id");
    assert!(value.get("client_version").is_none());
    assert!(value.get("native_build_number").is_none());

    assert_eq!(chrome_os_version("Mac OS X", "26.5.2"), "10.15.7");
    assert_eq!(chrome_os_version("Windows", "10.0.26100"), "10");
    assert_eq!(chrome_os_version("Linux", "6.12.0"), "6.12.0");
    assert_eq!(
        web_user_agent("Mac OS X", "10.15.7", "arm64"),
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36"
    );
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

#[test]
fn system_locale_normalization_accepts_language_tags_and_rejects_process_locales() {
    let cases = [
        ("ko_KR.UTF-8", Some("ko-KR")),
        ("en_US@calendar", Some("en-US")),
        ("zh_Hans_CN.UTF-8", Some("zh-Hans-CN")),
        ("C.UTF-8", None),
        ("POSIX", None),
        ("invalid locale", None),
    ];

    for (raw, expected) in cases {
        assert_eq!(normalize_system_locale(raw).as_deref(), expected);
    }

    assert_eq!(accept_language("en"), "en");
    assert_eq!(accept_language("en-US"), "en-US,en;q=0.9");
    assert_eq!(
        accept_language("ko-KR"),
        "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7"
    );
}

#[test]
fn windows_version_extracts_the_numeric_os_version() {
    assert_eq!(
        windows_version("Microsoft Windows [Version 10.0.26100.4652]").as_deref(),
        Some("10.0.26100.4652")
    );
    assert_eq!(windows_version("Microsoft Windows"), None);
}

fn assert_uuid_field(value: &Value, field: &str) {
    let raw = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} should be a string"));
    Uuid::parse_str(raw).unwrap_or_else(|_| panic!("{field} should be a UUID"));
}

#[test]
fn shared_auth_session_sends_web_headers_and_persists_server_cookies() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let fingerprint = Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER));
    let state_dir = tempfile::tempdir().expect("temporary state directory should be created");
    let cookie_path = state_dir.path().join("discord-cookies.json");
    let http = discord_http_client_with_cookie_path(&fingerprint, Some(cookie_path.clone()));
    let auth_session = DiscordAuthSession::with_http(Arc::clone(&fingerprint), http);
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

        let third_request = accept_request(&listener);
        let (third_request, headers) = read_headers(third_request);
        assert!(
            headers.iter().any(|line| {
                line.to_ascii_lowercase().starts_with("cookie:")
                    && line.contains("__dcfduid=test-cookie")
            }),
            "a new HTTP client should restore the persisted server cookie"
        );
        respond(
            third_request,
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok",
        );
    });

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime should start");
    runtime.block_on(async {
        auth_session
            .http()
            .get(format!("http://{address}/first"))
            .headers(discord_rest_headers(&fingerprint))
            .send()
            .await
            .expect("first local request should succeed")
            .error_for_status()
            .expect("first local response should be successful");
        auth_session
            .clone()
            .http()
            .get(format!("http://{address}/second"))
            .headers(discord_rest_headers(&fingerprint))
            .send()
            .await
            .expect("second local request should succeed")
            .error_for_status()
            .expect("second local response should be successful");

        tokio::time::timeout(Duration::from_secs(2), async {
            while !cookie_path.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("server cookies should be persisted after the debounce");
        let restored_http =
            discord_http_client_with_cookie_path(&fingerprint, Some(cookie_path.clone()));
        restored_http
            .get(format!("http://{address}/third"))
            .headers(discord_rest_headers(&fingerprint))
            .send()
            .await
            .expect("restored local request should succeed")
            .error_for_status()
            .expect("restored local response should be successful");
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
