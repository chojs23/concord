use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};

use super::auth_http::{discord_login_headers, discord_web_client};

const LOGIN_URL: &str = "https://discord.com/api/v9/auth/login";
const MFA_VERIFY_URL: &str = "https://discord.com/api/v9/auth/mfa";
const MFA_SMS_SEND_URL: &str = "https://discord.com/api/v9/auth/mfa/sms/send";

#[derive(Clone, Debug)]
pub enum PasswordAuthEvent {
    Status(String),
    MfaRequired(MfaChallenge),
    SmsSent { phone: Option<String> },
    Token(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MfaMethod {
    Totp,
    Sms,
}

impl MfaMethod {
    fn endpoint_name(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Sms => "sms",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MfaChallenge {
    pub ticket: String,
    pub login_instance_id: String,
    pub methods: Vec<MfaMethod>,
}

pub fn spawn_login(
    login: String,
    password: String,
    events_tx: mpsc::Sender<PasswordAuthEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = events_tx
            .send(PasswordAuthEvent::Status(
                "Authenticating with Discord...".to_string(),
            ))
            .await;

        match login_with_password(&login, &password).await {
            Ok(LoginOutcome::Token(token)) => {
                let _ = events_tx.send(PasswordAuthEvent::Token(token)).await;
            }
            Ok(LoginOutcome::MfaRequired(challenge)) => {
                let _ = events_tx
                    .send(PasswordAuthEvent::MfaRequired(challenge))
                    .await;
            }
            Err(error) => {
                let _ = events_tx.send(PasswordAuthEvent::Failed(error)).await;
            }
        }
    })
}

pub fn spawn_mfa_verify(
    method: MfaMethod,
    code: String,
    ticket: String,
    login_instance_id: String,
    events_tx: mpsc::Sender<PasswordAuthEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = events_tx
            .send(PasswordAuthEvent::Status(
                "Verifying multi-factor authentication...".to_string(),
            ))
            .await;

        match verify_mfa(method, &code, &ticket, &login_instance_id).await {
            Ok(token) => {
                let _ = events_tx.send(PasswordAuthEvent::Token(token)).await;
            }
            Err(error) => {
                let _ = events_tx.send(PasswordAuthEvent::Failed(error)).await;
            }
        }
    })
}

pub fn spawn_sms_send(
    ticket: String,
    events_tx: mpsc::Sender<PasswordAuthEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let _ = events_tx
            .send(PasswordAuthEvent::Status(
                "Requesting Discord SMS code...".to_string(),
            ))
            .await;

        match send_mfa_sms(&ticket).await {
            Ok(phone) => {
                let _ = events_tx.send(PasswordAuthEvent::SmsSent { phone }).await;
            }
            Err(error) => {
                let _ = events_tx.send(PasswordAuthEvent::Failed(error)).await;
            }
        }
    })
}

enum LoginOutcome {
    Token(String),
    MfaRequired(MfaChallenge),
}

async fn login_with_password(login: &str, password: &str) -> Result<LoginOutcome, String> {
    let response = http_client()
        .post(LOGIN_URL)
        .headers(discord_login_headers())
        .json(&json!({
            "login": normalize_login_identifier(login),
            "password": password,
        }))
        .send()
        .await
        .map_err(|error| format!("Discord password login request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("read Discord password login response failed: {error}"))?;

    if status.is_success() {
        parse_login_success(&body)
    } else {
        Err(format_login_error(status, &body))
    }
}

async fn send_mfa_sms(ticket: &str) -> Result<Option<String>, String> {
    #[derive(Deserialize)]
    struct SmsResponse {
        phone: Option<String>,
    }

    let response = http_client()
        .post(MFA_SMS_SEND_URL)
        .headers(discord_login_headers())
        .json(&json!({ "ticket": ticket }))
        .send()
        .await
        .map_err(|error| format!("Discord SMS request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("read Discord SMS response failed: {error}"))?;

    if status.is_success() {
        let response: SmsResponse = serde_json::from_str(&body)
            .map_err(|error| format!("decode Discord SMS response failed: {error}"))?;
        Ok(response.phone)
    } else {
        Err(format_login_error(status, &body))
    }
}

async fn verify_mfa(
    method: MfaMethod,
    code: &str,
    ticket: &str,
    login_instance_id: &str,
) -> Result<String, String> {
    let url = format!("{MFA_VERIFY_URL}/{}", method.endpoint_name());
    let response = http_client()
        .post(url)
        .headers(discord_login_headers())
        .json(&json!({
            "code": code.trim(),
            "login_instance_id": login_instance_id,
            "ticket": ticket,
        }))
        .send()
        .await
        .map_err(|error| format!("Discord MFA request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("read Discord MFA response failed: {error}"))?;

    if status.is_success() {
        token_from_body(&body).ok_or_else(|| "Discord MFA response did not include a token".into())
    } else {
        Err(format_login_error(status, &body))
    }
}

fn http_client() -> reqwest::Client {
    discord_web_client().expect("reqwest client builder with static user agent cannot fail")
}

fn parse_login_success(body: &str) -> Result<LoginOutcome, String> {
    #[derive(Deserialize)]
    struct LoginResponse {
        token: Option<String>,
        mfa: Option<bool>,
        sms: Option<bool>,
        totp: Option<bool>,
        ticket: Option<String>,
        login_instance_id: Option<String>,
        suspended_user_token: Option<String>,
    }

    let response: LoginResponse = serde_json::from_str(body)
        .map_err(|error| format!("decode Discord password login response failed: {error}"))?;
    if let Some(token) = response.token {
        return Ok(LoginOutcome::Token(token));
    }
    if response.mfa == Some(true) {
        let ticket = response
            .ticket
            .ok_or("Discord MFA response did not include a ticket")?;
        let login_instance_id = response
            .login_instance_id
            .ok_or("Discord MFA response did not include a login instance id")?;
        let mut methods = Vec::new();
        if response.totp == Some(true) {
            methods.push(MfaMethod::Totp);
        }
        if response.sms == Some(true) {
            methods.push(MfaMethod::Sms);
        }
        if methods.is_empty() {
            return Err(
                "Discord requires MFA, but this terminal supports only TOTP and SMS".into(),
            );
        }
        return Ok(LoginOutcome::MfaRequired(MfaChallenge {
            ticket,
            login_instance_id,
            methods,
        }));
    }
    if response.suspended_user_token.is_some() {
        return Err("Discord account is suspended".into());
    }
    Err("Discord password login response did not include a token".into())
}

fn token_from_body(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("token")?
        .as_str()
        .map(str::to_string)
}

fn format_login_error(status: reqwest::StatusCode, body: &str) -> String {
    #[derive(Deserialize)]
    struct DiscordError {
        code: Option<i64>,
        message: Option<String>,
        captcha_key: Option<Value>,
    }

    let Ok(error) = serde_json::from_str::<DiscordError>(body) else {
        return format!("Discord login failed with HTTP {status}");
    };
    if error.captcha_key.is_some() {
        return "Discord requires captcha verification, so email/password login cannot continue in this terminal. Log in with a token instead.".to_string();
    }
    match error.code {
        Some(50035) => "Discord rejected the email/phone number or password".to_string(),
        Some(20013) | Some(20011) => {
            "Discord account is disabled or marked for deletion".to_string()
        }
        Some(70007) => "Discord requires phone verification before login can continue".to_string(),
        Some(70009) => "Discord requires email verification before login can continue".to_string(),
        _ => error
            .message
            .unwrap_or_else(|| format!("Discord login failed with HTTP {status}")),
    }
}

fn normalize_login_identifier(login: &str) -> String {
    let trimmed = login.trim();
    if trimmed.starts_with('+') {
        trimmed.replace([' ', '-'], "")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LOGIN_URL, LoginOutcome, MFA_SMS_SEND_URL, MfaMethod, format_login_error,
        normalize_login_identifier, parse_login_success,
    };
    use reqwest::StatusCode;

    #[test]
    fn parse_login_success_returns_token() {
        let outcome = parse_login_success(r#"{"token":"abc"}"#).expect("token should parse");

        assert!(matches!(outcome, LoginOutcome::Token(token) if token == "abc"));
    }

    #[test]
    fn parse_login_success_returns_supported_mfa_methods() {
        let outcome = parse_login_success(
            r#"{"mfa":true,"sms":true,"totp":true,"ticket":"ticket","login_instance_id":"login"}"#,
        )
        .expect("mfa should parse");

        let LoginOutcome::MfaRequired(challenge) = outcome else {
            panic!("expected MFA challenge");
        };
        assert_eq!(challenge.ticket, "ticket");
        assert_eq!(challenge.login_instance_id, "login");
        assert_eq!(challenge.methods, vec![MfaMethod::Totp, MfaMethod::Sms]);
    }

    #[test]
    fn captcha_error_is_clear_and_does_not_include_raw_body() {
        let message = format_login_error(
            StatusCode::BAD_REQUEST,
            r#"{"captcha_key":["captcha-required"],"captcha_rqtoken":"secret"}"#,
        );

        assert!(message.contains("captcha"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn phone_login_identifier_removes_common_separators() {
        assert_eq!(normalize_login_identifier("+1 234-567"), "+1234567");
    }

    #[test]
    fn generic_error_keeps_status_without_raw_body() {
        let message = format_login_error(StatusCode::TOO_MANY_REQUESTS, "not json secret");

        assert_eq!(
            message,
            "Discord login failed with HTTP 429 Too Many Requests"
        );
        assert!(!message.contains("secret"));
    }

    #[test]
    fn sms_send_uses_api_endpoint() {
        assert_eq!(LOGIN_URL, "https://discord.com/api/v9/auth/login");
        assert_eq!(
            MFA_SMS_SEND_URL,
            "https://discord.com/api/v9/auth/mfa/sms/send"
        );
    }
}
