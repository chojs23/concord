use std::time::Duration;

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::{SinkExt, StreamExt};
use rand::rngs::OsRng;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey, pkcs8::EncodePublicKey};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::Sha256;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

use super::auth_http::{
    DISCORD_LOGIN_REFERER, DISCORD_ORIGIN, DISCORD_WEB_USER_AGENT, discord_web_client,
};

const REMOTE_AUTH_URL: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
const TICKET_EXCHANGE_URL: &str = "https://discord.com/api/v10/users/@me/remote-auth/login";
const QR_QUIET_ZONE_MODULES: usize = 4;

#[derive(Clone, Debug)]
pub enum QrEvent {
    Status(String),
    QrBitmap(Vec<Vec<bool>>),
    UserPending {
        username: String,
        discriminator: String,
    },
    Token(String),
    Cancelled,
    Failed(String),
}

pub fn spawn(events_tx: mpsc::Sender<QrEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        match run(&events_tx).await {
            Ok(Some(token)) => {
                let _ = events_tx.send(QrEvent::Token(token)).await;
            }
            Ok(None) => {
                let _ = events_tx.send(QrEvent::Cancelled).await;
            }
            Err(message) => {
                let _ = events_tx.send(QrEvent::Failed(message)).await;
            }
        }
    })
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

async fn run(tx: &mpsc::Sender<QrEvent>) -> Result<Option<String>, String> {
    let _ = tx
        .send(QrEvent::Status(
            "Connecting to Discord remote auth gateway...".into(),
        ))
        .await;

    let mut request = REMOTE_AUTH_URL.into_client_request().map_err(err)?;
    {
        let headers = request.headers_mut();
        headers.insert("Origin", HeaderValue::from_static(DISCORD_ORIGIN));
        headers.insert(
            "User-Agent",
            HeaderValue::from_static(DISCORD_WEB_USER_AGENT),
        );
    }

    let (ws, _) = connect_async(request).await.map_err(err)?;
    let (mut writer, mut reader) = ws.split();

    let _ = tx
        .send(QrEvent::Status("Generating RSA-2048 key pair...".into()))
        .await;
    let key_task = tokio::task::spawn_blocking(|| RsaPrivateKey::new(&mut OsRng, 2048));

    let hello_text = read_text(&mut reader).await?;
    let hello: Value = serde_json::from_str(&hello_text).map_err(err)?;
    if hello.get("op").and_then(Value::as_str) != Some("hello") {
        return Err(format!("expected hello op, got: {hello_text}"));
    }
    let heartbeat_ms = hello
        .get("heartbeat_interval")
        .and_then(Value::as_u64)
        .unwrap_or(40_000);
    let heartbeat_interval = Duration::from_millis(heartbeat_ms);

    let private_key = key_task.await.map_err(err)?.map_err(err)?;
    let public_key = RsaPublicKey::from(&private_key);
    let spki = public_key.to_public_key_der().map_err(err)?;
    let encoded_public = STANDARD.encode(spki.as_bytes());

    send_op(
        &mut writer,
        &json!({
            "op": "init",
            "encoded_public_key": encoded_public,
        }),
    )
    .await?;

    let _ = tx
        .send(QrEvent::Status("Waiting for handshake...".into()))
        .await;

    let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);
    heartbeat_timer.tick().await;

    let mut fingerprint: Option<String> = None;

    loop {
        tokio::select! {
            _ = heartbeat_timer.tick() => {
                send_op(&mut writer, &json!({"op": "heartbeat"})).await?;
            }
            msg = reader.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    Some(Ok(Message::Binary(b))) => String::from_utf8(b.to_vec()).map_err(err)?,
                    Some(Ok(Message::Close(_))) | None => return Err("connection closed".into()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(err(e)),
                };
                let value: Value = serde_json::from_str(&text).map_err(err)?;
                let op = value.get("op").and_then(Value::as_str).unwrap_or("");
                match op {
                    "nonce_proof" => {
                        let encrypted_b64 = value
                            .get("encrypted_nonce")
                            .and_then(Value::as_str)
                            .ok_or("missing encrypted_nonce")?;
                        let encrypted = STANDARD.decode(encrypted_b64).map_err(err)?;
                        let decrypted = private_key
                            .decrypt(Oaep::new::<Sha256>(), &encrypted)
                            .map_err(err)?;
                        let proof = URL_SAFE_NO_PAD.encode(&decrypted);
                        send_op(
                            &mut writer,
                            &json!({"op": "nonce_proof", "nonce": proof}),
                        )
                        .await?;
                    }
                    "pending_remote_init" => {
                        let fp = value
                            .get("fingerprint")
                            .and_then(Value::as_str)
                            .ok_or("missing fingerprint")?
                            .to_string();
                        let bitmap = build_qr_bitmap(&format!("https://discord.com/ra/{fp}"))?;
                        let _ = tx.send(QrEvent::QrBitmap(bitmap)).await;
                        let _ = tx
                            .send(QrEvent::Status(
                                "Scan this QR code in the Discord mobile app to log in.".into(),
                            ))
                            .await;
                        fingerprint = Some(fp);
                    }
                    "pending_ticket" => {
                        let payload_b64 = value
                            .get("encrypted_user_payload")
                            .and_then(Value::as_str)
                            .ok_or("missing encrypted_user_payload")?;
                        let encrypted = STANDARD.decode(payload_b64).map_err(err)?;
                        let decrypted = private_key
                            .decrypt(Oaep::new::<Sha256>(), &encrypted)
                            .map_err(err)?;
                        let payload = String::from_utf8(decrypted).map_err(err)?;
                        let parts: Vec<&str> = payload.splitn(4, ':').collect();
                        if parts.len() == 4 {
                            let _ = tx
                                .send(QrEvent::UserPending {
                                    username: parts[3].to_string(),
                                    discriminator: parts[1].to_string(),
                                })
                                .await;
                            let _ = tx
                                .send(QrEvent::Status(
                                    "Confirm the login in the Discord mobile app.".into(),
                                ))
                                .await;
                        }
                    }
                    "pending_login" => {
                        let ticket = value
                            .get("ticket")
                            .and_then(Value::as_str)
                            .ok_or("missing ticket")?
                            .to_string();
                        let _ = tx
                            .send(QrEvent::Status("Authenticating with Discord...".into()))
                            .await;
                        let _ = writer.close().await;
                        let token = exchange_ticket(&ticket, &private_key, fingerprint.as_deref()).await?;
                        return Ok(Some(token));
                    }
                    "cancel" => {
                        return Ok(None);
                    }
                    "heartbeat_ack" => {}
                    _ => {}
                }
            }
        }
    }
}

async fn read_text<S>(reader: &mut S) -> Result<String, String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match reader.next().await {
            Some(Ok(Message::Text(t))) => return Ok(t.to_string()),
            Some(Ok(Message::Binary(b))) => {
                return String::from_utf8(b.to_vec()).map_err(err);
            }
            Some(Ok(Message::Close(_))) | None => return Err("connection closed".into()),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(err(e)),
        }
    }
}

async fn send_op<S>(writer: &mut S, value: &Value) -> Result<(), String>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let text = serde_json::to_string(value).map_err(err)?;
    writer.send(Message::Text(text.into())).await.map_err(err)
}

fn build_qr_bitmap(content: &str) -> Result<Vec<Vec<bool>>, String> {
    use qrcode::{Color, EcLevel, QrCode};

    let code = QrCode::with_error_correction_level(content, EcLevel::M).map_err(err)?;
    let width = code.width();
    let output_width = width + QR_QUIET_ZONE_MODULES * 2;
    let colors = code.to_colors();
    let mut rows = vec![vec![false; output_width]; output_width];
    for y in 0..width {
        for x in 0..width {
            rows[y + QR_QUIET_ZONE_MODULES][x + QR_QUIET_ZONE_MODULES] =
                colors[y * width + x] == Color::Dark;
        }
    }
    Ok(rows)
}

async fn exchange_ticket(
    ticket: &str,
    private_key: &RsaPrivateKey,
    fingerprint: Option<&str>,
) -> Result<String, String> {
    #[derive(Deserialize)]
    struct ExchangeResponse {
        encrypted_token: String,
    }

    let client = discord_web_client().map_err(err)?;

    let response = send_ticket_exchange(&client, ticket, fingerprint)
        .await
        .map_err(err)?;
    let response = checked_ticket_exchange_response(response).await?;

    let response: ExchangeResponse = response.json().await.map_err(err)?;

    let encrypted = STANDARD.decode(&response.encrypted_token).map_err(err)?;
    let decrypted = private_key
        .decrypt(Oaep::new::<Sha256>(), &encrypted)
        .map_err(err)?;
    String::from_utf8(decrypted).map_err(err)
}

async fn send_ticket_exchange(
    client: &reqwest::Client,
    ticket: &str,
    fingerprint: Option<&str>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut request = client
        .post(TICKET_EXCHANGE_URL)
        .header("Origin", DISCORD_ORIGIN)
        .header("Referer", DISCORD_LOGIN_REFERER)
        .json(&json!({ "ticket": ticket }));
    if let Some(fp) = fingerprint {
        request = request.header("X-Fingerprint", fp);
    }

    request.send().await
}

async fn checked_ticket_exchange_response(
    response: reqwest::Response,
) -> Result<reqwest::Response, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.map_err(err)?;
        return Err(format_ticket_exchange_error(status, &body));
    }

    Ok(response)
}

fn format_ticket_exchange_error(status: reqwest::StatusCode, body: &str) -> String {
    if parse_captcha_challenge(status, body).is_some() {
        "Discord requires captcha verification, so QR login cannot continue in this terminal. Log in with a token instead.".into()
    } else {
        format_discord_error_response(status, body)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct CaptchaChallenge {
    captcha_key: Vec<String>,
    captcha_service: Option<String>,
}

fn parse_captcha_challenge(status: reqwest::StatusCode, body: &str) -> Option<CaptchaChallenge> {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return None;
    }

    let challenge = serde_json::from_str::<CaptchaChallenge>(body).ok()?;
    let is_hcaptcha = challenge
        .captcha_service
        .as_deref()
        .is_none_or(|service| service.eq_ignore_ascii_case("hcaptcha"));
    let requires_captcha = challenge
        .captcha_key
        .iter()
        .any(|key| key == "captcha-required");

    if is_hcaptcha && requires_captcha {
        Some(challenge)
    } else {
        None
    }
}

fn format_discord_error_response(status: reqwest::StatusCode, body: &str) -> String {
    let body = sanitize_response_body(body);
    if body.is_empty() {
        format!("Discord ticket exchange failed with status {status}")
    } else {
        format!("Discord ticket exchange failed with status {status}: {body}")
    }
}

fn sanitize_response_body(body: &str) -> String {
    const MAX_BODY_CHARS: usize = 1_200;

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let sanitized = match serde_json::from_str::<Value>(trimmed) {
        Ok(mut value) => {
            redact_sensitive_json(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| trimmed.to_string())
        }
        Err(_) => trimmed.to_string(),
    };

    truncate_chars(&sanitized, MAX_BODY_CHARS)
}

fn redact_sensitive_json(value: &mut Value) {
    match value {
        Value::Object(map) => redact_sensitive_json_object(map),
        Value::Array(values) => {
            for value in values {
                redact_sensitive_json(value);
            }
        }
        _ => {}
    }
}

fn redact_sensitive_json_object(map: &mut Map<String, Value>) {
    for (key, value) in map {
        if is_sensitive_response_key(key) {
            *value = Value::String("[redacted]".to_string());
        } else {
            redact_sensitive_json(value);
        }
    }
}

fn is_sensitive_response_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("ticket")
        || key.contains("rqdata")
        || key == "captcha_session_id"
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...[truncated]")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QR_QUIET_ZONE_MODULES, build_qr_bitmap, format_ticket_exchange_error,
        parse_captcha_challenge, sanitize_response_body,
    };

    #[test]
    fn qr_bitmap_includes_four_module_quiet_zone() {
        let bitmap = build_qr_bitmap("https://discord.com/ra/test-fingerprint")
            .expect("QR bitmap should build");
        let width = bitmap.len();

        assert!(width > QR_QUIET_ZONE_MODULES * 2);
        assert!(bitmap.iter().all(|row| row.len() == width));
        assert!(
            bitmap[..QR_QUIET_ZONE_MODULES]
                .iter()
                .all(|row| row.iter().all(|module| !module))
        );
        assert!(
            bitmap[width - QR_QUIET_ZONE_MODULES..]
                .iter()
                .all(|row| row.iter().all(|module| !module))
        );
        assert!(bitmap.iter().all(|row| {
            row[..QR_QUIET_ZONE_MODULES]
                .iter()
                .chain(&row[width - QR_QUIET_ZONE_MODULES..])
                .all(|module| !module)
        }));
        assert!(
            bitmap[QR_QUIET_ZONE_MODULES..width - QR_QUIET_ZONE_MODULES]
                .iter()
                .any(
                    |row| row[QR_QUIET_ZONE_MODULES..width - QR_QUIET_ZONE_MODULES]
                        .iter()
                        .any(|module| *module)
                )
        );
    }

    #[test]
    fn sanitize_response_body_preserves_useful_error_fields() {
        let body = r#"{"message":"captcha required","captcha_service":"hcaptcha"}"#;

        let sanitized = sanitize_response_body(body);

        assert!(sanitized.contains("captcha required"));
        assert!(sanitized.contains("hcaptcha"));
    }

    #[test]
    fn sanitize_response_body_redacts_sensitive_fields() {
        let body = r#"{"ticket":"abc","captcha_rqtoken":"secret","nested":{"encrypted_token":"token"},"captcha_rqdata":"blob","captcha_session_id":"session"}"#;

        let sanitized = sanitize_response_body(body);

        assert!(!sanitized.contains("abc"));
        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("\":\"token\""));
        assert!(!sanitized.contains("blob"));
        assert!(!sanitized.contains("\":\"session\""));
        assert!(sanitized.contains("[redacted]"));
    }

    #[test]
    fn parse_captcha_challenge_accepts_hcaptcha_required_response() {
        let body = r#"{"captcha_key":["captcha-required"],"captcha_service":"hcaptcha","captcha_sitekey":"site","captcha_rqdata":"data","captcha_rqtoken":"rqtoken","captcha_session_id":"session"}"#;

        assert!(parse_captcha_challenge(reqwest::StatusCode::BAD_REQUEST, body).is_some());
    }

    #[test]
    fn captcha_response_fails_without_local_fallback() {
        let message = format_ticket_exchange_error(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"captcha_key":["captcha-required"],"captcha_service":"hcaptcha"}"#,
        );

        assert!(message.contains("captcha"));
        assert!(message.contains("token"));
    }
}
