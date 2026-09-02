use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chrono::{TimeZone, Utc};
use reqwest::header::{AUTHORIZATION, ORIGIN};

use crate::discord::ids::{
    Id,
    marker::{ApplicationMarker, EmojiMarker, GuildMarker, UserMarker},
};

use crate::{
    AppError,
    discord::{
        ApplicationCommandInfo, ApplicationCommandInteraction, ApplicationCommandInteractionOption,
        BASE_ATTACHMENT_LIMIT_BYTES, BASE_MESSAGE_CHARACTER_LIMIT, GuildFolder,
        MessageAttachmentUpload, MessageSearchAuthorType, MessageSearchHas, MessageSearchQuery,
        MessageSendLimits, NITRO_MESSAGE_CHARACTER_LIMIT, ReactionEmoji, ReplyReference,
        application_commands::{
            ApplicationCommandAutocompleteInteraction, ApplicationCommandAutocompleteOption,
        },
        fingerprint::{CLIENT_BUILD_NUMBER, ClientFingerprint},
        validate_message_content, validate_message_payload,
    },
};

const BASE_MESSAGE_LIMITS: MessageSendLimits = MessageSendLimits {
    max_content_chars: BASE_MESSAGE_CHARACTER_LIMIT,
    max_attachment_bytes: BASE_ATTACHMENT_LIMIT_BYTES,
};

use super::{
    DiscordRest, FORBIDDEN_FAILURE_WINDOW, MAX_FORBIDDEN_CIRCUITS, MessageSendCoordinator,
    REST_MESSAGE_SEND_MIN_INTERVAL, REST_MUTATION_MIN_INTERVAL,
    REST_UNKNOWN_MUTATION_ROUTE_INTERVAL, RequestRoute, RequestSafety, RestRateLimitBody,
    RestRateLimitDecision, RestRateLimitResponse, RestRateLimitRoute, RestRateLimiter,
    RestRequestPacer, RestRequestPacingKind,
    application_commands::{
        application_command_autocomplete_body, application_command_interaction_body,
        application_command_option_body, parse_application_command_index,
    },
    apply_authenticated_method_headers,
    forum::{
        create_forum_post_request_body, parse_create_forum_post_response,
        parse_forum_post_data_response, parse_public_archived_threads_response,
    },
    messages::{
        MessageEditRequest, edit_message_request_body, message_multipart_form,
        message_request_body, message_request_body_with_tts, parse_pinned_messages_response,
        upload_content_type,
    },
    notification_settings::mute_request_body,
    polls::poll_vote_request_body,
    profile::parse_user_profile_response,
    reactions::{
        next_reaction_users_after, parse_reaction_users_response, reaction_route_component,
    },
    search::{
        MessageSearchResponse, message_search_date_snowflake_bounds, message_search_has_more,
        message_search_query_params, message_search_retry_delay,
    },
    user_settings::settings_proto_request_body,
};

#[test]
fn rest_rate_limit_routes_normalize_ids_but_keep_major_scope() {
    let first = test_request(
        "https://discord.com/api/v9/channels/123/messages/456/reactions/%F0%9F%91%8D/@me",
    );
    let second = test_request(
        "https://discord.com/api/v9/channels/999/messages/777/reactions/%F0%9F%8E%89/@me",
    );

    let first = RestRateLimitRoute::from_request(&first);
    let second = RestRateLimitRoute::from_request(&second);

    assert_eq!(first.family, second.family);
    assert_eq!(
        first.family.template,
        "/api/v9/channels/:major/messages/:id/reactions/:reaction/@me"
    );
    assert_eq!(first.major_parameter, "123");
    assert_eq!(second.major_parameter, "999");

    let safety_first = RequestRoute::from_request(&test_request(
        "https://discord.com/api/v9/channels/123/messages/456/reactions/%F0%9F%91%8D/@me",
    ));
    let safety_second = RequestRoute::from_request(&test_request(
        "https://discord.com/api/v9/channels/999/messages/777/reactions/%F0%9F%8E%89/@me",
    ));
    assert_ne!(safety_first, safety_second);
    assert_eq!(
        safety_first.path,
        "/api/v9/channels/:major/messages/:id/reactions/:reaction/@me"
    );
    assert_eq!(safety_first.major_parameter, "123");
    assert_eq!(safety_second.major_parameter, "999");
}

#[test]
fn stream_preview_routes_keep_each_stream_as_a_major_scope() {
    let first = RestRateLimitRoute::from_request(&test_request_with_method(
        reqwest::Method::POST,
        "https://discord.com/api/v9/streams/guild:123:456:789/preview",
    ));
    let second = RestRateLimitRoute::from_request(&test_request_with_method(
        reqwest::Method::POST,
        "https://discord.com/api/v9/streams/guild:123:456:999/preview",
    ));

    assert_eq!(first.family, second.family);
    assert_eq!(first.family.template, "/api/v9/streams/:major/preview");
    assert_eq!(first.major_parameter, "guild:123:456:789");
    assert_eq!(second.major_parameter, "guild:123:456:999");
}

#[test]
fn rest_request_pacing_separates_message_sends_from_other_requests() {
    let cases = [
        (
            reqwest::Method::POST,
            "https://discord.com/api/v9/channels/123/messages",
            RestRequestPacingKind::MessageSend,
        ),
        (
            reqwest::Method::POST,
            "https://discord.com/api/v9/channels/123/typing",
            RestRequestPacingKind::Mutation,
        ),
        (
            reqwest::Method::PATCH,
            "https://discord.com/api/v9/channels/123/messages/456",
            RestRequestPacingKind::Mutation,
        ),
        (
            reqwest::Method::GET,
            "https://discord.com/api/v9/channels/123/messages",
            RestRequestPacingKind::None,
        ),
    ];

    for (method, url, expected) in cases {
        let route = RequestRoute::from_request(&test_request_with_method(method, url));
        assert_eq!(route.pacing_kind(), expected, "{url}");
    }
}

#[test]
fn rest_request_pacers_reserve_configured_intervals_independently() {
    let message_send_pacer = RestRequestPacer::new(REST_MESSAGE_SEND_MIN_INTERVAL);
    let mutation_pacer = RestRequestPacer::new(REST_MUTATION_MIN_INTERVAL);
    let started_at = std::time::Instant::now();

    assert_eq!(message_send_pacer.reserve_at(started_at), None);
    assert_eq!(
        message_send_pacer.reserve_at(started_at),
        Some(Duration::from_millis(100))
    );
    assert_eq!(mutation_pacer.reserve_at(started_at), None);
    assert_eq!(
        mutation_pacer.reserve_at(started_at),
        Some(Duration::from_millis(500))
    );
}

#[tokio::test]
async fn rest_rate_limiter_serializes_a_route_until_headers_are_learned() {
    let limiter = Arc::new(RestRateLimiter::default());
    let request = test_request("https://discord.com/api/v9/channels/123/messages");
    let route = RestRateLimitRoute::from_request(&request);
    let first = limiter.acquire(route.clone()).await;
    let second_acquired = Arc::new(AtomicBool::new(false));
    let waiter = {
        let limiter = Arc::clone(&limiter);
        let second_acquired = Arc::clone(&second_acquired);
        tokio::spawn(async move {
            let _permit = limiter.acquire(route).await;
            second_acquired.store(true, Ordering::Release);
        })
    };

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!second_acquired.load(Ordering::Acquire));
    drop(first);
    tokio::time::timeout(Duration::from_millis(50), waiter)
        .await
        .expect("the next request should continue when the probe ends")
        .expect("the probe waiter should finish");
    assert!(second_acquired.load(Ordering::Acquire));
}

#[test]
fn rest_rate_limiter_applies_learned_bucket_per_major_scope() {
    let limiter = RestRateLimiter::default();
    let first_request = test_request("https://discord.com/api/v9/channels/123/messages");
    let other_channel_request = test_request("https://discord.com/api/v9/channels/999/messages");
    let first_route = RestRateLimitRoute::from_request(&first_request);
    let other_channel_route = RestRateLimitRoute::from_request(&other_channel_request);
    let now = std::time::Instant::now();
    let (admitted_key, probe) = match limiter.reserve_at(&first_route, now) {
        RestRateLimitDecision::Admit { key, probe } => (key, probe),
        _ => panic!("the first request should be admitted"),
    };
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-ratelimit-bucket",
        "messages".parse().expect("valid header"),
    );
    headers.insert("x-ratelimit-remaining", "0".parse().expect("valid header"));
    headers.insert(
        "x-ratelimit-reset-after",
        "2".parse().expect("valid header"),
    );

    limiter.finish(
        &first_route,
        &admitted_key,
        probe,
        RestRateLimitResponse {
            headers: &headers,
            status: reqwest::StatusCode::OK,
            body: None,
            now,
            wall_clock: SystemTime::now(),
        },
    );

    assert!(matches!(
        limiter.reserve_at(&first_route, now + Duration::from_secs(1)),
        RestRateLimitDecision::Delay(delay) if delay == Duration::from_secs(1)
    ));
    assert!(matches!(
        limiter.reserve_at(&other_channel_route, now + Duration::from_secs(1)),
        RestRateLimitDecision::Admit { probe: true, .. }
    ));

    let fallback_limiter = RestRateLimiter::default();
    let mutation_request = test_request_with_method(
        reqwest::Method::POST,
        "https://discord.com/api/v9/channels/123/messages",
    );
    let mutation_route = RestRateLimitRoute::from_request(&mutation_request);
    let (mutation_key, mutation_probe) = match fallback_limiter.reserve_at(&mutation_route, now) {
        RestRateLimitDecision::Admit { key, probe } => (key, probe),
        _ => panic!("the first mutation should be admitted"),
    };
    fallback_limiter.finish(
        &mutation_route,
        &mutation_key,
        mutation_probe,
        RestRateLimitResponse {
            headers: &reqwest::header::HeaderMap::new(),
            status: reqwest::StatusCode::OK,
            body: None,
            now,
            wall_clock: SystemTime::now(),
        },
    );
    assert!(matches!(
        fallback_limiter.reserve_at(&mutation_route, now),
        RestRateLimitDecision::Delay(delay)
            if delay == REST_UNKNOWN_MUTATION_ROUTE_INTERVAL
    ));
}

#[test]
fn rest_rate_limiter_applies_global_retry_after_to_other_routes() {
    let limiter = RestRateLimiter::default();
    let first_request = test_request("https://discord.com/api/v9/users/@me");
    let other_request = test_request("https://discord.com/api/v9/guilds/123/channels");
    let first_route = RestRateLimitRoute::from_request(&first_request);
    let other_route = RestRateLimitRoute::from_request(&other_request);
    let now = std::time::Instant::now();
    let (admitted_key, probe) = match limiter.reserve_at(&first_route, now) {
        RestRateLimitDecision::Admit { key, probe } => (key, probe),
        _ => panic!("the first request should be admitted"),
    };
    let headers = reqwest::header::HeaderMap::new();

    limiter.finish(
        &first_route,
        &admitted_key,
        probe,
        RestRateLimitResponse {
            headers: &headers,
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            body: Some(RestRateLimitBody {
                retry_after: Some(Duration::from_secs(3)),
                global: true,
            }),
            now,
            wall_clock: SystemTime::now(),
        },
    );

    assert!(matches!(
        limiter.reserve_at(&other_route, now + Duration::from_secs(1)),
        RestRateLimitDecision::Delay(delay) if delay == Duration::from_secs(2)
    ));
}

#[test]
fn forbidden_circuit_resets_stale_failures_and_bounds_storage() {
    let safety = RequestSafety::default();
    let route = RequestRoute {
        method: "POST".to_owned(),
        path: "/channels/1/messages".to_owned(),
        major_parameter: "none".to_owned(),
    };
    let start = std::time::Instant::now();
    safety.record_response_at(&route, reqwest::StatusCode::FORBIDDEN, start);
    safety.record_response_at(
        &route,
        reqwest::StatusCode::FORBIDDEN,
        start + Duration::from_secs(1),
    );
    let outside_window = start + FORBIDDEN_FAILURE_WINDOW + Duration::from_secs(1);

    safety.record_response_at(&route, reqwest::StatusCode::FORBIDDEN, outside_window);

    safety
        .preflight_at(&route, outside_window)
        .expect("old forbidden failures should not open the circuit");
    let circuits = safety
        .forbidden_circuits
        .lock()
        .expect("request circuit mutex is not poisoned");
    assert_eq!(
        circuits
            .get(&route)
            .expect("the recent failure should remain")
            .consecutive_forbidden,
        1
    );
    drop(circuits);

    let safety = RequestSafety::default();
    for index in 0..(MAX_FORBIDDEN_CIRCUITS + 10) {
        safety.record_response_at(
            &RequestRoute {
                method: "GET".to_owned(),
                path: format!("/messages/{index}"),
                major_parameter: "none".to_owned(),
            },
            reqwest::StatusCode::FORBIDDEN,
            start + Duration::from_millis(index as u64),
        );
    }
    assert_eq!(
        safety
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned")
            .len(),
        MAX_FORBIDDEN_CIRCUITS
    );

    let cleanup_time = start
        + FORBIDDEN_FAILURE_WINDOW
        + Duration::from_millis((MAX_FORBIDDEN_CIRCUITS + 10) as u64)
        + Duration::from_secs(1);
    safety
        .preflight_at(
            &RequestRoute {
                method: "GET".to_owned(),
                path: "/untracked".to_owned(),
                major_parameter: "none".to_owned(),
            },
            cleanup_time,
        )
        .expect("stale route cleanup should not block a new route");
    assert!(
        safety
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned")
            .is_empty()
    );
}

#[tokio::test]
async fn authenticated_requests_stop_after_unauthorized_response() {
    let rest = test_rest();
    let mut get_request = rest
        .raw_http
        .get("https://discord.com/api/v9/users/@me")
        .header(ORIGIN, "https://wrong.example")
        .build()
        .expect("GET request should build");
    apply_authenticated_method_headers(&mut get_request);
    assert!(get_request.headers().get(ORIGIN).is_none());

    let mut post_request = rest
        .raw_http
        .post("https://discord.com/api/v9/channels/1/messages")
        .build()
        .expect("POST request should build");
    apply_authenticated_method_headers(&mut post_request);
    assert_eq!(
        post_request
            .headers()
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("https://discord.com")
    );

    let mut log_headers = reqwest::header::HeaderMap::new();
    log_headers.insert(AUTHORIZATION, "secret-token".parse().expect("valid header"));
    log_headers.insert(
        "x-super-properties",
        "secret-properties".parse().expect("valid header"),
    );
    log_headers.insert("user-agent", "safe-agent".parse().expect("valid header"));
    let logged = super::request_headers_for_log(&log_headers);
    assert!(!logged.contains("authorization"));
    assert!(!logged.contains("x-super-properties"));
    assert!(logged.contains("user-agent=\"safe-agent\""));
    assert!(!logged.contains("secret-token"));
    assert!(!logged.contains("secret-properties"));

    let (base_url, server) = status_server(vec![
        "HTTP/1.1 401 Unauthorized\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
    ]);

    let first = rest
        .send_unit(rest.raw_http.get(format!("{base_url}/users/@me")), "first")
        .await
        .expect_err("401 must stop authenticated requests");
    assert!(matches!(first, AppError::DiscordAuthenticationStopped));

    let second = rest
        .send_unit(
            rest.raw_http.get(format!("{base_url}/channels/1")),
            "second",
        )
        .await
        .expect_err("later request must stop before network I/O");
    assert!(matches!(second, AppError::DiscordAuthenticationStopped));
    server.join().expect("test server should finish");
}

#[tokio::test]
async fn repeated_forbidden_route_is_scoped_to_major_resource() {
    let forbidden = "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
    let success = "HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
    let (base_url, server) = status_server(vec![forbidden, forbidden, forbidden, success]);
    let rest = test_rest();

    for attempt in 0..3 {
        let error = rest
            .send_unit(
                rest.raw_http
                    .get(format!("{base_url}/channels/1/messages?attempt={attempt}")),
                "forbidden",
            )
            .await
            .expect_err("server should return 403");
        assert!(matches!(error, AppError::DiscordRequest(_)));
    }

    rest.send_unit(
        rest.raw_http
            .get(format!("{base_url}/channels/99/messages?attempt=4")),
        "other channel",
    )
    .await
    .expect("a different channel must not share the forbidden circuit");

    let blocked = rest
        .send_unit(
            rest.raw_http
                .get(format!("{base_url}/channels/1/messages?attempt=5")),
            "forbidden",
        )
        .await
        .expect_err("the repeated channel route should be blocked locally");
    assert!(matches!(
        blocked,
        AppError::DiscordRequestCircuitOpen { ref path, .. }
            if path == "/channels/:major/messages"
    ));
    server.join().expect("test server should finish");
}

#[tokio::test]
async fn rate_limit_returns_retry_delay_without_retrying() {
    let (base_url, server) = status_server(vec![
        "HTTP/1.1 429 Too Many Requests\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"retry_after\":1.5,\"global\":false}",
    ]);
    let rest = test_rest();

    let error = rest
        .send_unit(rest.raw_http.get(format!("{base_url}/messages")), "send")
        .await
        .expect_err("429 should be returned to the caller");
    assert!(matches!(
        error,
        AppError::DiscordRateLimited {
            retry_after_millis: 1_500,
            ..
        }
    ));
    server
        .join()
        .expect("test server should receive only one request");
}

#[tokio::test]
async fn discord_json_error_decodes_message_and_code() {
    let body = r#"{"message":"\uad8c\ud55c \uc5c6\uc74c","code":50013}"#;
    let response = format!(
        "HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (base_url, server) = status_server(vec![&response]);
    let rest = test_rest();

    let error = rest
        .send_unit(rest.raw_http.get(format!("{base_url}/messages")), "send")
        .await
        .expect_err("Discord should return the decoded API error");

    assert!(matches!(
        error,
        AppError::DiscordRequest(message)
            if message.contains("권한 없음")
                && message.contains("Discord code 50013")
                && !message.contains(r"\uad8c")
    ));
    server.join().expect("test server should finish");
}

#[tokio::test]
async fn message_send_coordinator_serializes_only_matching_channels() {
    let coordinator = Arc::new(MessageSendCoordinator::default());
    let first_guard = coordinator.acquire(Id::new(10)).await;
    let same_channel_acquired = Arc::new(AtomicBool::new(false));
    let waiter = {
        let coordinator = Arc::clone(&coordinator);
        let same_channel_acquired = Arc::clone(&same_channel_acquired);
        tokio::spawn(async move {
            let _guard = coordinator.acquire(Id::new(10)).await;
            same_channel_acquired.store(true, Ordering::Release);
        })
    };

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!same_channel_acquired.load(Ordering::Acquire));
    let other_channel =
        tokio::time::timeout(Duration::from_millis(50), coordinator.acquire(Id::new(11)))
            .await
            .expect("different channel should not wait");
    drop(other_channel);

    drop(first_guard);
    waiter.await.expect("same-channel waiter should finish");
    assert!(same_channel_acquired.load(Ordering::Acquire));
}

#[test]
fn message_send_coordinator_tracks_cooldowns_per_channel() {
    let coordinator = MessageSendCoordinator::default();
    coordinator.record_cooldown(Id::new(10), Duration::from_secs(1));

    assert!(matches!(
        coordinator.ensure_cooldown_elapsed(Id::new(10)),
        Err(AppError::MessageSlowModeActive {
            retry_after_millis: 1..=1_000
        })
    ));
    coordinator
        .ensure_cooldown_elapsed(Id::new(11))
        .expect("another channel has no cooldown");
}

fn test_rest() -> DiscordRest {
    let _ = rustls::crypto::ring::default_provider().install_default();
    DiscordRest::new(
        "test-token".to_owned(),
        reqwest::Client::new(),
        Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER)),
    )
}

fn test_request(url: &str) -> reqwest::Request {
    test_request_with_method(reqwest::Method::GET, url)
}

fn test_request_with_method(method: reqwest::Method, url: &str) -> reqwest::Request {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
        .request(method, url)
        .build()
        .expect("test request should build")
}

fn status_server(responses: Vec<&str>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let address = listener
        .local_addr()
        .expect("test server should have an address");
    let responses: Vec<String> = responses.into_iter().map(str::to_owned).collect();
    let server = thread::spawn(move || {
        for response in responses {
            let stream = listener
                .accept()
                .expect("test server should accept a request")
                .0;
            let mut stream = read_request_headers(stream);
            stream
                .write_all(response.as_bytes())
                .expect("test server should write a response");
        }
    });
    (format!("http://{address}"), server)
}

fn read_request_headers(stream: TcpStream) -> TcpStream {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("test server should read request headers");
        if line == "\r\n" || line.is_empty() {
            break;
        }
    }
    reader.into_inner()
}

#[test]
fn rejects_invalid_message_content() {
    let error = validate_message_content("   ", BASE_MESSAGE_CHARACTER_LIMIT)
        .expect_err("blank messages must fail");
    assert!(matches!(error, AppError::EmptyMessageContent));

    for (limit, len, allowed) in [
        (BASE_MESSAGE_CHARACTER_LIMIT, 2_000, true),
        (BASE_MESSAGE_CHARACTER_LIMIT, 2_001, false),
        (NITRO_MESSAGE_CHARACTER_LIMIT, 4_000, true),
        (NITRO_MESSAGE_CHARACTER_LIMIT, 4_001, false),
    ] {
        let result = validate_message_content(&"x".repeat(len), limit);
        if allowed {
            result.expect("content at the account limit should pass");
        } else {
            assert!(matches!(
                result,
                Err(AppError::MessageTooLong {
                    len: actual_len,
                    limit: actual_limit,
                }) if actual_len == len && actual_limit == limit
            ));
        }
    }
}

#[test]
fn validates_attachment_only_message_payload() {
    let attachments = vec![MessageAttachmentUpload::from_path(
        "/tmp/cat.png".into(),
        "cat.png".to_owned(),
        2_048,
    )];

    validate_message_payload("   ", &attachments, BASE_MESSAGE_LIMITS)
        .expect("file-only messages should be valid");

    let body = message_request_body(
        "",
        Id::new(99),
        Some(ReplyReference {
            message_id: Id::new(44),
            mention_author: true,
        }),
        &attachments,
    );
    assert_eq!(body["content"], "");
    assert_eq!(body["message_reference"]["message_id"], "44");
    assert!(body.get("allowed_mentions").is_none());
    assert_eq!(body["attachments"][0]["id"], 0);
    assert_eq!(body["attachments"][0]["filename"], "cat.png");
}

#[test]
fn message_request_body_matches_web_defaults_and_carries_snowflake_nonce() {
    let body = message_request_body("hi", Id::new(99), None, &[]);
    let nonce = body["nonce"].as_str().expect("nonce is a string");
    assert_eq!(nonce, "99");
    assert_eq!(body["enforce_nonce"], true);
    assert_eq!(body["mobile_network_type"], "unknown");
    assert_eq!(body["tts"], false);
    assert_eq!(body["flags"], 0);

    let tts = message_request_body_with_tts("hi", Id::new(99), None, &[], true);
    assert_eq!(tts["tts"], true);
}

#[test]
fn message_request_body_suppresses_reply_ping_when_disabled() {
    let body = message_request_body(
        "hi",
        Id::new(99),
        Some(ReplyReference {
            message_id: Id::new(44),
            mention_author: false,
        }),
        &[],
    );
    assert_eq!(body["message_reference"]["message_id"], "44");
    assert_eq!(body["allowed_mentions"]["replied_user"], false);
    assert_eq!(body["allowed_mentions"]["parse"][0], "users");
}

#[test]
fn forum_post_request_body_nests_message_and_tags() {
    let body = create_forum_post_request_body(
        "  Need help  ",
        "The client crashes",
        &[Id::new(101), Id::new(102)],
        &[],
        BASE_MESSAGE_LIMITS,
    )
    .expect("forum post body should build");

    assert_eq!(body["name"], "Need help");
    assert_eq!(body["message"]["content"], "The client crashes");
    assert_eq!(body["applied_tags"], serde_json::json!(["101", "102"]));
}

#[test]
fn forum_post_request_body_validates_title_and_message() {
    let error = create_forum_post_request_body(" ", "body", &[], &[], BASE_MESSAGE_LIMITS)
        .expect_err("empty title must fail");
    assert!(matches!(error, AppError::DiscordRequest(_)));

    let error = create_forum_post_request_body("title", " ", &[], &[], BASE_MESSAGE_LIMITS)
        .expect_err("empty body must fail");
    assert!(matches!(error, AppError::EmptyMessageContent));
}

#[test]
fn forum_post_create_response_parses_thread_and_nested_first_message() {
    let response = parse_create_forum_post_response(
        &serde_json::json!({
            "id": "30",
            "guild_id": "1",
            "type": 11,
            "name": "Need help",
            "thread_metadata": {
                "archived": false,
                "locked": false
            },
            "member": {
                "id": "30",
                "user_id": "10",
                "flags": 4,
                "muted": false
            },
            "message": {
                "id": "30",
                "channel_id": "30",
                "author": {
                    "id": "10",
                    "username": "neo"
                },
                "type": 0,
                "content": "Body",
                "timestamp": "2026-01-01T00:00:00.000000+00:00",
                "edited_timestamp": null,
                "pinned": false,
                "mention_everyone": false,
                "mentions": [],
                "mention_roles": [],
                "attachments": [],
                "embeds": []
            }
        }),
        Some(Id::new(20)),
    )
    .expect("create response should parse");

    assert_eq!(response.thread.channel_id, Id::new(30));
    assert_eq!(response.thread.parent_id, Some(Id::new(20)));
    assert_eq!(
        response
            .current_user_member
            .as_ref()
            .and_then(|member| member.thread_id),
        Some(Id::new(30))
    );
    assert_eq!(
        response.first_message.map(|message| message.message_id),
        Some(Id::new(30))
    );
}

#[test]
fn public_archived_thread_parser_preserves_order_members_and_cursor() {
    let raw = serde_json::json!({
        "threads": [
            {
                "id": "31",
                "guild_id": "1",
                "parent_id": "20",
                "type": 11,
                "name": "newer archive",
                "thread_metadata": {
                    "archived": true,
                    "archive_timestamp": "2026-08-14T02:00:00.000000+00:00",
                    "locked": false
                }
            },
            {
                "id": "30",
                "type": 11,
                "name": "older archive",
                "thread_metadata": {
                    "archived": true,
                    "archive_timestamp": "2026-08-14T01:00:00.000000+00:00",
                    "locked": true
                }
            },
            {
                "id": "29",
                "type": 11,
                "name": "not archived",
                "thread_metadata": {
                    "archived": false,
                    "archive_timestamp": "2026-08-14T00:00:00.000000+00:00",
                    "locked": false
                }
            }
        ],
        "members": [
            {
                "id": "30",
                "user_id": "9",
                "flags": 4,
                "muted": true,
                "future_member_field": "kept"
            },
            {
                "id": "99",
                "user_id": "9",
                "flags": 8
            }
        ],
        "has_more": true,
        "future_page_field": 7
    });

    let page = parse_public_archived_threads_response(&raw, Id::new(1), Id::new(20));

    assert_eq!(
        page.threads
            .iter()
            .map(|thread| thread.channel_id)
            .collect::<Vec<_>>(),
        vec![Id::new(31), Id::new(30)]
    );
    assert_eq!(page.threads[1].parent_id, Some(Id::new(20)));
    assert_eq!(page.members.len(), 1);
    assert_eq!(page.members[0].thread_id, Some(Id::new(30)));
    assert_eq!(
        page.members[0].extra_fields.get("future_member_field"),
        Some(&serde_json::json!("kept"))
    );
    assert!(page.has_more);
    assert_eq!(
        page.next_before.as_deref(),
        Some("2026-08-14T00:00:00.000000+00:00")
    );
    assert_eq!(
        page.extra_fields.get("future_page_field"),
        Some(&serde_json::json!(7))
    );
}

#[test]
fn forum_post_data_parser_hydrates_only_requested_threads() {
    let guild_id = Id::<GuildMarker>::new(1);
    let requested = vec![Id::new(31), Id::new(30), Id::new(99)];
    let message = |channel_id: &str, author_id: &str| {
        serde_json::json!({
            "id": channel_id,
            "channel_id": channel_id,
            "author": { "id": author_id, "username": "neo" },
            "type": 0,
            "content": "Body",
            "timestamp": "2026-01-01T00:00:00.000000+00:00",
            "edited_timestamp": null,
            "pinned": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": []
        })
    };
    let raw = serde_json::json!({
        "threads": {
            "30": {
                "owner": {
                    "user": { "id": "10", "username": "neo" },
                    "roles": []
                },
                "first_message": message("30", "10"),
                "future_field": true
            },
            "31": {
                "owner": null,
                "first_message": message("999", "11")
            },
            "40": {
                "owner": null,
                "first_message": message("40", "12")
            }
        }
    });

    let posts = parse_forum_post_data_response(&raw, guild_id, &requested);

    assert_eq!(
        posts.iter().map(|post| post.thread_id).collect::<Vec<_>>(),
        vec![Id::new(31), Id::new(30)]
    );
    assert_eq!(posts[0].first_message, None);
    assert_eq!(
        posts[1].owner.as_ref().map(|owner| owner.user_id),
        Some(Id::new(10))
    );
    assert_eq!(
        posts[1]
            .first_message
            .as_ref()
            .map(|message| message.channel_id),
        Some(Id::new(30))
    );
    assert_eq!(
        posts[1].extra_fields.get("future_field"),
        Some(&serde_json::Value::Bool(true))
    );
}

#[test]
fn guild_folder_settings_proto_includes_name_and_color() {
    let body = settings_proto_request_body(&[GuildFolder {
        id: Some(42),
        name: Some("work".to_owned()),
        color: Some(0x00aaff),
        guild_ids: vec![Id::new(1), Id::new(2)],
    }]);
    let settings = body["settings"]
        .as_str()
        .expect("settings body should be base64");
    let decoded = BASE64_STANDARD
        .decode(settings)
        .expect("settings body should decode");

    assert!(decoded.windows(b"work".len()).any(|bytes| bytes == b"work"));
    assert!(
        decoded
            .windows(4)
            .any(|bytes| bytes == [0x08, 0xff, 0xd5, 0x02])
    );
}

#[test]
fn edit_message_request_body_sets_only_requested_fields() {
    let (content_body, content_action) = edit_message_request_body(
        MessageEditRequest::Content("hello"),
        BASE_MESSAGE_CHARACTER_LIMIT,
    )
    .expect("content edit body should build");
    let (flags_body, flags_action) = edit_message_request_body(
        MessageEditRequest::Flags(4_100),
        BASE_MESSAGE_CHARACTER_LIMIT,
    )
    .expect("flags edit body should build");

    assert_eq!(content_body, serde_json::json!({ "content": "hello" }));
    assert_eq!(content_action, "edit message");
    assert_eq!(flags_body, serde_json::json!({ "flags": 4_100 }));
    assert_eq!(flags_action, "update message flags");
}

#[test]
fn application_command_interaction_body_nests_subcommand_options_for_guild_command() {
    let interaction = ApplicationCommandInteraction {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        nonce: "interaction-nonce".to_owned(),
        command: ApplicationCommandInfo {
            application_id: Id::<ApplicationMarker>::new(200),
            version: "1".to_owned(),
            application_name: Some("ModBot".to_owned()),
            description: "moderation".to_owned(),
            raw: serde_json::json!({ "name": "mod", "guild_id": "1" }),
            ..ApplicationCommandInfo::test(Id::<ApplicationMarker>::new(100), "mod")
        },
        options: vec![ApplicationCommandInteractionOption {
            kind: 2,
            name: "admin".to_owned(),
            value: None,
            options: vec![ApplicationCommandInteractionOption {
                kind: 1,
                name: "ban".to_owned(),
                value: None,
                options: vec![ApplicationCommandInteractionOption {
                    kind: 6,
                    name: "user".to_owned(),
                    value: Some(serde_json::json!("123")),
                    options: Vec::new(),
                }],
            }],
        }],
    };

    let body = application_command_interaction_body(&interaction, "session");

    assert_eq!(
        body["data"]["options"],
        serde_json::json!([
            {
                "type": 2,
                "name": "admin",
                "options": [
                    {
                        "type": 1,
                        "name": "ban",
                        "options": [
                            { "type": 6, "name": "user", "value": "123" }
                        ]
                    }
                ]
            }
        ])
    );
    assert_eq!(body["data"]["guild_id"], "1");
    assert_eq!(body["nonce"], "interaction-nonce");
    assert!(body["data"]["options"][0].get("value").is_none());
    assert!(
        body["data"]["options"][0]["options"][0]
            .get("value")
            .is_none()
    );
}

#[test]
fn application_command_interaction_body_omits_data_guild_id_for_global_command() {
    let interaction = ApplicationCommandInteraction {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        nonce: "interaction-nonce".to_owned(),
        command: ApplicationCommandInfo {
            application_id: Id::<ApplicationMarker>::new(200),
            version: "1".to_owned(),
            application_name: Some("MusicBot".to_owned()),
            description: "search music".to_owned(),
            raw: serde_json::json!({
                "id": "100",
                "application_id": "200",
                "name": "search",
                "version": "1",
                "integration_types": [0],
            }),
            ..ApplicationCommandInfo::test(Id::<ApplicationMarker>::new(100), "search")
        },
        options: Vec::new(),
    };

    let body = application_command_interaction_body(&interaction, "session");

    assert_eq!(body["guild_id"], "1");
    assert!(body["data"].get("guild_id").is_none());
}

#[test]
fn application_command_autocomplete_body_marks_only_the_focused_option() {
    let interaction = ApplicationCommandAutocompleteInteraction {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        nonce: "autocomplete-nonce".to_owned(),
        command: ApplicationCommandInfo {
            application_id: Id::<ApplicationMarker>::new(200),
            version: "1".to_owned(),
            raw: serde_json::json!({
                "id": "100",
                "application_id": "200",
                "name": "search",
                "version": "1",
                "guild_id": "1",
            }),
            ..ApplicationCommandInfo::test(Id::<ApplicationMarker>::new(100), "search")
        },
        options: vec![ApplicationCommandAutocompleteOption {
            kind: 3,
            name: "query".to_owned(),
            value: Some(serde_json::json!("ne")),
            options: Vec::new(),
            focused: true,
        }],
    };

    let body = application_command_autocomplete_body(&interaction, "session");

    assert_eq!(body["type"], 4);
    assert_eq!(body["nonce"], "autocomplete-nonce");
    assert_eq!(
        body["data"]["options"],
        serde_json::json!([
            {
                "type": 3,
                "name": "query",
                "value": "ne",
                "focused": true
            }
        ])
    );
}

#[test]
fn application_command_index_joins_application_names() {
    let commands = parse_application_command_index(&serde_json::json!({
        "applications": [
            { "id": "200", "name": "PollBot" }
        ],
        "application_commands": [
            {
                "id": "100",
                "application_id": "200",
                "version": "1",
                "name": "poll",
                "description": "Create a poll",
                "options": []
            }
        ]
    }));

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].application_name.as_deref(), Some("PollBot"));
}

#[test]
fn application_command_option_body_keeps_value_and_options_exclusive() {
    let option = ApplicationCommandInteractionOption {
        kind: 3,
        name: "text".to_owned(),
        value: Some(serde_json::json!("hello")),
        options: vec![ApplicationCommandInteractionOption {
            kind: 3,
            name: "nested".to_owned(),
            value: Some(serde_json::json!("ignored")),
            options: Vec::new(),
        }],
    };

    let body = application_command_option_body(&option);

    assert_eq!(body["value"], serde_json::json!("hello"));
    assert!(body.get("options").is_none());
}

#[test]
fn enforces_per_file_upload_limit() {
    let too_large_file = vec![MessageAttachmentUpload::from_path(
        "/tmp/large.bin".into(),
        "large.bin".to_owned(),
        BASE_ATTACHMENT_LIMIT_BYTES + 1,
    )];
    let error = validate_message_payload("", &too_large_file, BASE_MESSAGE_LIMITS)
        .expect_err("oversized attachment must fail");
    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));

    let many_sub_limit_files = ["a.bin", "b.bin", "c.bin"]
        .into_iter()
        .map(|name| {
            MessageAttachmentUpload::from_path(
                format!("/tmp/{name}").into(),
                name.to_owned(),
                BASE_ATTACHMENT_LIMIT_BYTES - 1,
            )
        })
        .collect::<Vec<_>>();
    validate_message_payload("", &many_sub_limit_files, BASE_MESSAGE_LIMITS)
        .expect("files each under the per-file limit are accepted even if their sum exceeds it");
}

#[tokio::test]
async fn multipart_form_rechecks_current_file_size() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("concord-rest-{unique}"));
    std::fs::create_dir_all(&directory).expect("temp upload directory can be created");
    let path = directory.join("changed.bin");
    std::fs::write(&path, [0_u8]).expect("small temp file can be written");
    let attachment = MessageAttachmentUpload::from_path(path.clone(), "changed.bin".to_owned(), 1);
    std::fs::write(
        &path,
        vec![0_u8; (BASE_ATTACHMENT_LIMIT_BYTES + 1) as usize],
    )
    .expect("oversized temp file can be written");

    let result = message_multipart_form(
        message_request_body("", Id::new(99), None, std::slice::from_ref(&attachment)),
        &[attachment],
        BASE_ATTACHMENT_LIMIT_BYTES,
    )
    .await;
    let Err(error) = result else {
        panic!("multipart form must re-check actual file size");
    };

    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn upload_content_type_uses_common_media_types() {
    assert_eq!(upload_content_type("clip.MP4"), "video/mp4");
    assert_eq!(upload_content_type("song.mp3"), "audio/mpeg");
    assert_eq!(
        upload_content_type("sheet.xlsx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        upload_content_type("unknown.concord"),
        "application/octet-stream"
    );
}

#[test]
fn reaction_route_component_formats_unicode_and_custom_reactions() {
    let custom = ReactionEmoji::Custom {
        id: Id::<EmojiMarker>::new(42),
        name: Some("party".to_owned()),
        animated: true,
    };
    let cases = [
        (ReactionEmoji::Unicode("🎉".to_owned()), "%F0%9F%8E%89"),
        (custom, "party%3A42"),
    ];

    for (reaction, expected) in cases {
        assert_eq!(reaction_route_component(&reaction), expected);
    }
}

#[test]
fn reaction_user_pagination_continues_only_after_full_pages() {
    let parsed = parse_reaction_users_response(vec![serde_json::json!({ "id": "1" })]);
    assert_eq!(
        parsed.users,
        vec![crate::discord::ReactionUserInfo::test(
            Id::new(1),
            "unknown"
        )]
    );

    // A full page (100 raw entries) hands back the last entry's id as the cursor
    // for the next page.
    let full: Vec<serde_json::Value> = (1..=100)
        .map(|id| serde_json::json!({ "id": id.to_string() }))
        .collect();
    assert_eq!(
        next_reaction_users_after(&full),
        Id::<UserMarker>::new_checked(100)
    );

    // A short page means there is nothing more to fetch.
    let short: Vec<serde_json::Value> = (1..=99)
        .map(|id| serde_json::json!({ "id": id.to_string() }))
        .collect();
    assert_eq!(next_reaction_users_after(&short), None);

    // A full page whose last entry lacks a parseable id yields no cursor.
    let mut malformed = full.clone();
    malformed[99] = serde_json::json!({ "id": "not-a-number" });
    assert_eq!(next_reaction_users_after(&malformed), None);
}

#[test]
fn search_and_pin_pagination_follow_server_metadata() {
    let response = MessageSearchResponse {
        total_results: Some(1),
        messages: Vec::new(),
        message_groups: vec![serde_json::json!([])],
        extra_fields: Default::default(),
    };
    assert!(!message_search_has_more(&response, 25));
    let full_page = MessageSearchResponse {
        message_groups: vec![serde_json::json!([]); 25],
        ..response
    };
    assert!(
        message_search_has_more(&full_page, 25),
        "an approximate total must not stop a full search page"
    );
    let short_page_with_reported_results_remaining = MessageSearchResponse {
        total_results: Some(50),
        messages: Vec::new(),
        message_groups: vec![serde_json::json!([])],
        extra_fields: Default::default(),
    };
    assert!(
        message_search_has_more(&short_page_with_reported_results_remaining, 25),
        "reported remaining results must keep pagination alive after a short page"
    );
    assert_eq!(
        message_search_retry_delay(&serde_json::json!({ "retry_after": 2.5 })),
        Duration::from_millis(2_500)
    );
    assert_eq!(
        message_search_retry_delay(&serde_json::json!({ "retry_after": 0 })),
        Duration::from_secs(1)
    );

    let pins = parse_pinned_messages_response(&serde_json::json!({
        "items": [
            {
                "pinned_at": "2026-07-24T02:00:00.000Z",
                "message": {
                    "id": "100",
                    "channel_id": "10",
                    "author": { "id": "20", "username": "neo" },
                    "timestamp": "2026-07-24T02:00:00.000Z"
                }
            },
            {
                "pinned_at": "2026-07-24T01:00:00.000Z",
                "message": {
                    "id": "99",
                    "channel_id": "10",
                    "author": { "id": "20", "username": "neo" },
                    "timestamp": "2026-07-24T01:00:00.000Z"
                }
            }
        ],
        "has_more": true
    }))
    .expect("pin page should parse");
    assert_eq!(pins.messages.len(), 2);
    assert!(pins.has_more);
    assert_eq!(
        pins.next_before.as_deref(),
        Some("2026-07-24T01:00:00.000Z")
    );
}

#[test]
fn message_search_date_filters_build_inclusive_snowflake_bounds() {
    let equal =
        message_search_date_snowflake_bounds("equal:2026-05-30").expect("equal date bounds");
    let range = message_search_date_snowflake_bounds("gte:2026-05-01,lte:2026-05-30")
        .expect("range date bounds");
    let lower_only = message_search_date_snowflake_bounds("gte:2026-05-30").expect("lower bound");
    let upper_only = message_search_date_snowflake_bounds("lte:2026-05-30").expect("upper bound");

    assert!(equal.min_id.is_some());
    assert!(equal.max_id.is_some());
    assert!(equal.min_id < equal.max_id);
    assert!(range.min_id < range.max_id);
    assert_eq!(lower_only.max_id, None);
    assert_eq!(upper_only.min_id, None);
    assert_eq!(
        message_search_date_snowflake_bounds("before:2026-05-30"),
        None
    );
}

#[test]
fn message_search_query_params_repeats_multi_value_filters() {
    let query = MessageSearchQuery {
        has: vec![MessageSearchHas::Link, MessageSearchHas::Image],
        author_type: vec![MessageSearchAuthorType::User, MessageSearchAuthorType::Bot],
        ..Default::default()
    };

    let params = message_search_query_params(&query);

    assert!(params.contains(&("has", "link".to_owned())));
    assert!(params.contains(&("has", "image".to_owned())));
    assert!(params.contains(&("author_type", "user".to_owned())));
    assert!(params.contains(&("author_type", "bot".to_owned())));
}

#[test]
fn poll_vote_request_body_uses_numeric_answer_ids() {
    assert_eq!(
        poll_vote_request_body(&[1, 2]),
        serde_json::json!({ "answer_ids": [1, 2] })
    );
    assert_eq!(
        poll_vote_request_body(&[]),
        serde_json::json!({ "answer_ids": [] })
    );
}

#[test]
fn mute_request_body_includes_selected_time_window() {
    let end_time = Utc
        .with_ymd_and_hms(2026, 5, 10, 12, 30, 45)
        .single()
        .expect("valid test timestamp");

    assert_eq!(
        mute_request_body(true, Some(end_time), Some(900)),
        serde_json::json!({
            "muted": true,
            "mute_config": {
                "end_time": "2026-05-10T12:30:45.000Z",
                "selected_time_window": 900,
            },
        })
    );
    assert_eq!(
        mute_request_body(true, None, Some(-1)),
        serde_json::json!({
            "muted": true,
            "mute_config": {
                "end_time": null,
                "selected_time_window": -1,
            },
        })
    );
    assert_eq!(
        mute_request_body(false, None, None),
        serde_json::json!({
            "muted": false,
            "mute_config": null,
        })
    );
}

#[test]
fn user_profile_parser_keeps_roles_and_mutual_friends() {
    let profile = parse_user_profile_response(
        Id::new(10),
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user" },
            "guild_member": { "roles": ["90", "91"] },
            "mutual_friends": [
                { "id": "50", "username": "alice", "global_name": "Alice" },
                { "id": "51", "username": "bob", "global_name": null }
            ],
            "mutual_friends_count": 2
        }),
        None,
    );

    assert_eq!(profile.role_ids, vec![Id::new(90), Id::new(91)]);
    assert!(profile.role_ids_present);
    assert_eq!(profile.mutual_friends_count, 2);
    assert_eq!(profile.mutual_friends.len(), 2);
    assert_eq!(profile.mutual_friends[0].display_name(), "Alice");
    assert_eq!(profile.mutual_friends[1].display_name(), "bob");
}

#[test]
fn user_profile_parser_resolves_avatar_url() {
    let with_avatar = parse_user_profile_response(
        Id::new(10),
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "avatar": "abc123" }
        }),
        None,
    );
    assert_eq!(
        with_avatar.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/10/abc123.png")
    );

    let default_profile = parse_user_profile_response(
        Id::new(10),
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "discriminator": "0" }
        }),
        None,
    );
    assert_eq!(
        default_profile.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/embed/avatars/0.png")
    );

    let guild_avatar = parse_user_profile_response(
        Id::new(10),
        Some(Id::new(77)),
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "avatar": "abc123" },
            "guild_member": { "avatar": "def456" }
        }),
        None,
    );
    assert_eq!(
        guild_avatar.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/guilds/77/users/10/avatars/def456.png")
    );
}
