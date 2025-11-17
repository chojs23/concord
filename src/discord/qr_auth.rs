use std::time::Duration;

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::{SinkExt, StreamExt};
use rand::rngs::OsRng;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey, pkcs8::EncodePublicKey};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

const REMOTE_AUTH_URL: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
const TICKET_EXCHANGE_URL: &str = "https://discord.com/api/v10/users/@me/remote-auth/login";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

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
        headers.insert("Origin", HeaderValue::from_static("https://discord.com"));
        headers.insert("User-Agent", HeaderValue::from_static(USER_AGENT));
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
                        let token =
                            exchange_ticket(&ticket, &private_key, fingerprint.as_deref())
                                .await?;
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

    let code =
        QrCode::with_error_correction_level(content, EcLevel::L).map_err(err)?;
    let width = code.width();
    let colors = code.to_colors();
    let mut rows = Vec::with_capacity(width);
    for y in 0..width {
        let mut row = Vec::with_capacity(width);
        for x in 0..width {
            row.push(colors[y * width + x] == Color::Dark);
        }
        rows.push(row);
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

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(err)?;

    let mut request = client
        .post(TICKET_EXCHANGE_URL)
        .header("Origin", "https://discord.com")
        .header("Referer", "https://discord.com/login")
        .json(&json!({ "ticket": ticket }));
    if let Some(fp) = fingerprint {
        request = request.header("X-Fingerprint", fp);
    }

    let response: ExchangeResponse = request
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;

    let encrypted = STANDARD.decode(&response.encrypted_token).map_err(err)?;
    let decrypted = private_key
        .decrypt(Oaep::new::<Sha256>(), &encrypted)
        .map_err(err)?;
    String::from_utf8(decrypted).map_err(err)
}
