use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::discord::{
    ids::{Id, marker::ChannelMarker},
    json::extra_fields,
};
use crate::{AppError, Result};

use reqwest::{
    RequestBuilder, Response, StatusCode,
    header::{AUTHORIZATION, HeaderMap, RETRY_AFTER},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

mod application;
mod application_commands;
mod connection;
mod forum;
mod guilds;
mod messages;
mod notification_settings;
mod polls;
mod presence;
mod profile;
mod reactions;
mod read_state;
mod search;
mod user_settings;

pub use forum::{CreatedForumPost, ForumPostPage};
pub(in crate::discord) use messages::MessageEditRequest;
pub use reactions::ReactionUsersPage;

#[derive(Clone, Debug)]
pub struct DiscordRest {
    raw_http: reqwest::Client,
    headers: HeaderMap,
    token: String,
    request_safety: Arc<RequestSafety>,
    message_sends: Arc<MessageSendCoordinator>,
}

const FORBIDDEN_CIRCUIT_THRESHOLD: u8 = 3;
const FORBIDDEN_CIRCUIT_COOLDOWN: Duration = Duration::from_secs(5 * 60);

impl DiscordRest {
    pub fn new(token: String, raw_http: reqwest::Client, headers: HeaderMap) -> Self {
        Self {
            raw_http,
            headers,
            token,
            request_safety: Arc::new(RequestSafety::default()),
            message_sends: Arc::new(MessageSendCoordinator::default()),
        }
    }

    async fn execute_authenticated(
        &self,
        request: RequestBuilder,
        label: &str,
    ) -> Result<Response> {
        let request = request
            .headers(self.headers.clone())
            .header(AUTHORIZATION, &self.token)
            .build()
            .map_err(|error| {
                AppError::DiscordRequest(format!("{label} request build failed: {error}"))
            })?;
        let route = RequestRoute::from_request(&request);
        self.request_safety.preflight(&route)?;

        let response = self.raw_http.execute(request).await.map_err(|error| {
            AppError::DiscordRequest(format!("{label} request failed: {error}"))
        })?;
        self.request_safety
            .record_response(&route, response.status());
        Ok(response)
    }

    async fn send_unit(&self, request: RequestBuilder, label: &str) -> Result<()> {
        let response = self.execute_authenticated(request, label).await?;
        if let Err(error) = response.error_for_status_ref() {
            return Err(request_error(error, response, label).await);
        }
        Ok(())
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        label: &str,
    ) -> Result<T> {
        let response = self.execute_authenticated(request, label).await?;
        if let Err(error) = response.error_for_status_ref() {
            return Err(request_error(error, response, label).await);
        }
        response
            .json()
            .await
            .map_err(|error| AppError::DiscordRequest(format!("{label} decode failed: {error}")))
    }
}

#[derive(Debug, Default)]
struct RequestSafety {
    authentication_stopped: AtomicBool,
    forbidden_circuits: Mutex<HashMap<RequestRoute, ForbiddenCircuit>>,
}

impl RequestSafety {
    fn preflight(&self, route: &RequestRoute) -> Result<()> {
        if self.authentication_stopped.load(Ordering::Acquire) {
            return Err(AppError::DiscordAuthenticationStopped);
        }

        let now = Instant::now();
        let mut circuits = self
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned");
        let Some(circuit) = circuits.get_mut(route) else {
            return Ok(());
        };
        let Some(open_until) = circuit.open_until else {
            return Ok(());
        };
        if open_until <= now {
            circuits.remove(route);
            return Ok(());
        }

        Err(AppError::DiscordRequestCircuitOpen {
            method: route.method.clone(),
            path: route.path.clone(),
            retry_after_millis: duration_millis_ceil(open_until.duration_since(now)),
        })
    }

    fn record_response(&self, route: &RequestRoute, status: StatusCode) {
        if status == StatusCode::UNAUTHORIZED {
            self.authentication_stopped.store(true, Ordering::Release);
        }

        let mut circuits = self
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned");
        if status != StatusCode::FORBIDDEN {
            circuits.remove(route);
            return;
        }

        let circuit = circuits.entry(route.clone()).or_default();
        circuit.consecutive_forbidden = circuit.consecutive_forbidden.saturating_add(1);
        if circuit.consecutive_forbidden >= FORBIDDEN_CIRCUIT_THRESHOLD {
            circuit.open_until = Some(Instant::now() + FORBIDDEN_CIRCUIT_COOLDOWN);
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RequestRoute {
    method: String,
    path: String,
}

impl RequestRoute {
    fn from_request(request: &reqwest::Request) -> Self {
        Self {
            method: request.method().as_str().to_owned(),
            path: request.url().path().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ForbiddenCircuit {
    consecutive_forbidden: u8,
    open_until: Option<Instant>,
}

#[derive(Debug, Default)]
struct MessageSendCoordinator {
    channel_locks: AsyncMutex<HashMap<Id<ChannelMarker>, Arc<AsyncMutex<()>>>>,
    cooldowns: Mutex<HashMap<Id<ChannelMarker>, Instant>>,
}

impl MessageSendCoordinator {
    async fn acquire(&self, channel_id: Id<ChannelMarker>) -> OwnedMutexGuard<()> {
        let channel_lock = {
            let mut channel_locks = self.channel_locks.lock().await;
            channel_locks
                .entry(channel_id)
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        channel_lock.lock_owned().await
    }

    fn ensure_cooldown_elapsed(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let now = Instant::now();
        let mut cooldowns = self
            .cooldowns
            .lock()
            .expect("message cooldown mutex is not poisoned");
        let Some(deadline) = cooldowns.get(&channel_id).copied() else {
            return Ok(());
        };
        if deadline <= now {
            cooldowns.remove(&channel_id);
            return Ok(());
        }
        Err(AppError::MessageSlowModeActive {
            retry_after_millis: duration_millis_ceil(deadline.duration_since(now)),
        })
    }

    fn record_cooldown(&self, channel_id: Id<ChannelMarker>, duration: Duration) {
        if duration.is_zero() {
            return;
        }
        self.cooldowns
            .lock()
            .expect("message cooldown mutex is not poisoned")
            .insert(channel_id, Instant::now() + duration);
    }
}

/// Turns a non-2xx Discord response into an `AppError`, reading the body once.
///
/// A captcha challenge becomes `CaptchaRequired` so callers stop instead of
/// retrying. Retrying an unsolved captcha is what escalates to a temporary
/// account block (issue #218).
async fn request_error(
    error: reqwest::Error,
    response: reqwest::Response,
    label: &str,
) -> AppError {
    let status = response.status();
    let retry_after_header = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<f64>().ok());
    let body = response.text().await.ok();
    if status == StatusCode::UNAUTHORIZED {
        return AppError::DiscordAuthenticationStopped;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = retry_after_header
            .or_else(|| {
                body.as_deref()
                    .and_then(|body| serde_json::from_str::<Value>(body).ok())
                    .and_then(|body| body.get("retry_after").and_then(Value::as_f64))
            })
            .unwrap_or(1.0)
            .max(0.0);
        return AppError::DiscordRateLimited {
            action: label.to_owned(),
            retry_after_millis: seconds_to_millis_ceil(retry_after),
        };
    }
    if let Some(body) = body.as_deref()
        && super::captcha::parse_captcha_challenge(status, body).is_some()
    {
        return AppError::CaptchaRequired {
            action: label.to_owned(),
        };
    }
    let detail = body
        .map(discord_error_detail)
        .filter(|detail| !detail.trim().is_empty());
    match detail {
        Some(detail) => AppError::DiscordRequest(format!("{label} failed: {error}: {detail}")),
        None => AppError::DiscordRequest(format!("{label} failed: {error}")),
    }
}

fn discord_error_detail(body: String) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(&body)
        && let Some(message) = value.get("message").and_then(Value::as_str)
        && !message.trim().is_empty()
    {
        let message = truncate_error_body(message.to_owned());
        let code = value.get("code").and_then(|code| {
            code.as_u64()
                .map(|code| code.to_string())
                .or_else(|| code.as_str().map(str::to_owned))
        });
        return match code {
            Some(code) => format!("{message} (Discord code {code})"),
            None => message,
        };
    }

    truncate_error_body(body)
}

fn seconds_to_millis_ceil(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1_000.0).ceil().min(u64::MAX as f64) as u64
}

fn duration_millis_ceil(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(u64::from(!duration.is_zero()))
}

fn truncate_error_body(body: String) -> String {
    const MAX_ERROR_BODY_CHARS: usize = 500;
    let mut chars = body.chars();
    let truncated: String = chars.by_ref().take(MAX_ERROR_BODY_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn clone_array(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|values| values.to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
