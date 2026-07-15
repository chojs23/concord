use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::discord::{
    ids::{Id, marker::ChannelMarker},
    json::extra_fields,
};
use crate::{AppError, Result, logging};

use reqwest::{
    RequestBuilder, Response, StatusCode,
    header::{AUTHORIZATION, HeaderMap, RETRY_AFTER},
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{Mutex as AsyncMutex, Notify, OwnedMutexGuard};

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
    rate_limiter: Arc<RestRateLimiter>,
    message_sends: Arc<MessageSendCoordinator>,
}

const FORBIDDEN_CIRCUIT_THRESHOLD: u8 = 3;
const FORBIDDEN_CIRCUIT_COOLDOWN: Duration = Duration::from_secs(5 * 60);
// A failure older than the cooldown is not evidence that the route is still
// forbidden. Using one window for both rules keeps circuit behavior predictable.
const FORBIDDEN_FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);
// Request routes can include message IDs. A fixed cap prevents one-off 403s on
// many messages from retaining state for the whole application session.
const MAX_FORBIDDEN_CIRCUITS: usize = 512;

impl DiscordRest {
    pub fn new(token: String, raw_http: reqwest::Client, headers: HeaderMap) -> Self {
        Self {
            raw_http,
            headers,
            token,
            request_safety: Arc::new(RequestSafety::default()),
            rate_limiter: Arc::new(RestRateLimiter::default()),
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
        let rate_limit_route = RestRateLimitRoute::from_request(&request);
        let method = request.method().as_str().to_owned();
        if let Err(error) = self.request_safety.preflight(&route) {
            logging::debug(
                "rest",
                format!(
                    "request blocked action={label:?} method={method} reason={}",
                    rest_error_kind(&error)
                ),
            );
            return Err(error);
        }

        let rate_limit_started_at = Instant::now();
        let rate_limit_permit = self.rate_limiter.acquire(rate_limit_route).await;
        let rate_limit_wait = rate_limit_started_at.elapsed();
        if rate_limit_wait >= Duration::from_millis(1) {
            logging::debug(
                "rest",
                format!(
                    "request rate limited action={label:?} method={method} wait_ms={}",
                    duration_millis_ceil(rate_limit_wait)
                ),
            );
        }
        if let Err(error) = self.request_safety.preflight(&route) {
            logging::debug(
                "rest",
                format!(
                    "request blocked action={label:?} method={method} reason={}",
                    rest_error_kind(&error)
                ),
            );
            return Err(error);
        }

        logging::debug(
            "rest",
            format!("request started action={label:?} method={method}"),
        );
        let started_at = Instant::now();
        let response = match self.raw_http.execute(request).await {
            Ok(response) => response,
            Err(error) => {
                logging::debug(
                    "rest",
                    format!(
                        "request transport failed action={label:?} method={method} elapsed_ms={}",
                        duration_millis_ceil(started_at.elapsed())
                    ),
                );
                return Err(AppError::DiscordRequest(format!(
                    "{label} request failed: {error}"
                )));
            }
        };
        rate_limit_permit.record_response(response.headers(), response.status());
        logging::debug(
            "rest",
            format!(
                "request completed action={label:?} method={method} status={} elapsed_ms={}",
                response.status().as_u16(),
                duration_millis_ceil(started_at.elapsed())
            ),
        );
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
        self.preflight_at(route, Instant::now())
    }

    fn preflight_at(&self, route: &RequestRoute, now: Instant) -> Result<()> {
        if self.authentication_stopped.load(Ordering::Acquire) {
            return Err(AppError::DiscordAuthenticationStopped);
        }

        let mut circuits = self
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned");
        prune_forbidden_circuits(&mut circuits, now);
        let Some(circuit) = circuits.get_mut(route) else {
            return Ok(());
        };
        let Some(open_until) = circuit.open_until else {
            return Ok(());
        };
        Err(AppError::DiscordRequestCircuitOpen {
            method: route.method.clone(),
            path: route.path.clone(),
            retry_after_millis: duration_millis_ceil(open_until.duration_since(now)),
        })
    }

    fn record_response(&self, route: &RequestRoute, status: StatusCode) {
        self.record_response_at(route, status, Instant::now());
    }

    fn record_response_at(&self, route: &RequestRoute, status: StatusCode, now: Instant) {
        if status == StatusCode::UNAUTHORIZED {
            self.authentication_stopped.store(true, Ordering::Release);
        }

        let mut circuits = self
            .forbidden_circuits
            .lock()
            .expect("request circuit mutex is not poisoned");
        prune_forbidden_circuits(&mut circuits, now);
        if status != StatusCode::FORBIDDEN {
            circuits.remove(route);
            return;
        }

        if !circuits.contains_key(route) && circuits.len() >= MAX_FORBIDDEN_CIRCUITS {
            let oldest = circuits
                .iter()
                .min_by_key(|(_, circuit)| circuit.last_forbidden)
                .map(|(route, _)| route.clone());
            if let Some(oldest) = oldest {
                circuits.remove(&oldest);
            }
        }

        let circuit = circuits
            .entry(route.clone())
            .or_insert_with(|| ForbiddenCircuit::new(now));
        if now
            .checked_duration_since(circuit.last_forbidden)
            .is_some_and(|elapsed| elapsed >= FORBIDDEN_FAILURE_WINDOW)
        {
            circuit.consecutive_forbidden = 0;
            circuit.open_until = None;
        }
        circuit.consecutive_forbidden = circuit.consecutive_forbidden.saturating_add(1);
        circuit.last_forbidden = now;
        if circuit.consecutive_forbidden >= FORBIDDEN_CIRCUIT_THRESHOLD {
            circuit.open_until = Some(now + FORBIDDEN_CIRCUIT_COOLDOWN);
        }
    }
}

fn prune_forbidden_circuits(circuits: &mut HashMap<RequestRoute, ForbiddenCircuit>, now: Instant) {
    circuits.retain(|_, circuit| match circuit.open_until {
        Some(open_until) => open_until > now,
        None => now
            .checked_duration_since(circuit.last_forbidden)
            .is_none_or(|elapsed| elapsed < FORBIDDEN_FAILURE_WINDOW),
    });
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

// Discord does not publish fixed REST limits. The response headers are the
// source of truth, so this limiter learns each route's bucket after the first
// response and shares that state across every clone of `DiscordRest`.
#[derive(Debug, Default)]
struct RestRateLimiter {
    state: Mutex<RestRateLimitState>,
    changed: Notify,
}

#[derive(Debug, Default)]
struct RestRateLimitState {
    global_until: Option<Instant>,
    route_buckets: HashMap<RestRateLimitRouteFamily, String>,
    windows: HashMap<RestRateLimitKey, RestRateLimitWindow>,
    in_flight_probes: HashSet<RestRateLimitKey>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RestRateLimitRoute {
    family: RestRateLimitRouteFamily,
    major_parameter: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RestRateLimitRouteFamily {
    method: String,
    template: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum RestRateLimitKey {
    Route {
        family: RestRateLimitRouteFamily,
        major_parameter: String,
    },
    Bucket {
        bucket: String,
        major_parameter: String,
    },
}

#[derive(Clone, Copy, Debug, Default)]
struct RestRateLimitWindow {
    remaining: Option<u32>,
    reset_at: Option<Instant>,
}

enum RestRateLimitDecision {
    Admit { key: RestRateLimitKey, probe: bool },
    Delay(Duration),
    WaitForProbe,
}

struct RestRateLimitPermit {
    limiter: Arc<RestRateLimiter>,
    route: RestRateLimitRoute,
    key: RestRateLimitKey,
    probe: bool,
    completed: bool,
}

struct RestRateLimitResponse<'a> {
    headers: &'a HeaderMap,
    status: StatusCode,
    now: Instant,
    wall_clock: SystemTime,
}

impl RestRateLimiter {
    async fn acquire(self: &Arc<Self>, route: RestRateLimitRoute) -> RestRateLimitPermit {
        loop {
            // Register before reading state so a response cannot notify between
            // the state check and the wait.
            let changed = self.changed.notified();
            let decision = self.reserve_at(&route, Instant::now());
            match decision {
                RestRateLimitDecision::Admit { key, probe } => {
                    return RestRateLimitPermit {
                        limiter: Arc::clone(self),
                        route,
                        key,
                        probe,
                        completed: false,
                    };
                }
                RestRateLimitDecision::Delay(delay) => tokio::time::sleep(delay).await,
                RestRateLimitDecision::WaitForProbe => changed.await,
            }
        }
    }

    fn reserve_at(&self, route: &RestRateLimitRoute, now: Instant) -> RestRateLimitDecision {
        let mut state = self
            .state
            .lock()
            .expect("REST rate limit mutex is not poisoned");
        state.prune_expired(now);

        if let Some(global_until) = state.global_until {
            return RestRateLimitDecision::Delay(global_until.duration_since(now));
        }

        let key = state.key_for(route);
        if let Some(window) = state.windows.get(&key) {
            match window.remaining {
                Some(0) => {
                    if let Some(reset_at) = window.reset_at {
                        return RestRateLimitDecision::Delay(reset_at.duration_since(now));
                    }
                }
                Some(remaining) => {
                    state
                        .windows
                        .get_mut(&key)
                        .expect("rate limit window still exists")
                        .remaining = Some(remaining - 1);
                    return RestRateLimitDecision::Admit { key, probe: false };
                }
                None => {}
            }
        }

        // Until a response reports a usable remaining count, allow only one
        // request for the route or learned bucket. This prevents a startup
        // burst from racing past a limit that the client has not learned yet.
        if !state.in_flight_probes.insert(key.clone()) {
            return RestRateLimitDecision::WaitForProbe;
        }
        RestRateLimitDecision::Admit { key, probe: true }
    }

    fn finish(
        &self,
        route: &RestRateLimitRoute,
        admitted_key: &RestRateLimitKey,
        probe: bool,
        response: RestRateLimitResponse<'_>,
    ) {
        let RestRateLimitResponse {
            headers,
            status,
            now,
            wall_clock,
        } = response;
        let mut state = self
            .state
            .lock()
            .expect("REST rate limit mutex is not poisoned");
        state.prune_expired(now);
        if probe {
            state.in_flight_probes.remove(admitted_key);
        }

        let bucket = header_string(headers, "x-ratelimit-bucket");
        let response_key = if let Some(bucket) = bucket {
            state
                .route_buckets
                .insert(route.family.clone(), bucket.clone());
            RestRateLimitKey::Bucket {
                bucket,
                major_parameter: route.major_parameter.clone(),
            }
        } else {
            admitted_key.clone()
        };
        if response_key != *admitted_key {
            state.windows.remove(admitted_key);
        }

        let is_global = header_bool(headers, "x-ratelimit-global")
            || header_string(headers, "x-ratelimit-scope").as_deref() == Some("global");
        let mut reset_at = rate_limit_reset_at(headers, now, wall_clock);
        let mut remaining = header_u32(headers, "x-ratelimit-remaining");
        if status == StatusCode::TOO_MANY_REQUESTS {
            remaining = Some(0);
            if reset_at.is_none() {
                reset_at = Some(now + Duration::from_secs(1));
            }
            if is_global {
                state.global_until = reset_at;
            }
        }

        if remaining.is_none() && reset_at.is_none() {
            state.windows.remove(&response_key);
        } else {
            state.update_window(response_key, remaining, reset_at, now);
        }
        drop(state);
        self.changed.notify_waiters();
    }

    fn release_probe(&self, key: &RestRateLimitKey) {
        let mut state = self
            .state
            .lock()
            .expect("REST rate limit mutex is not poisoned");
        let removed = state.in_flight_probes.remove(key);
        drop(state);
        if removed {
            self.changed.notify_waiters();
        }
    }
}

impl RestRateLimitState {
    fn key_for(&self, route: &RestRateLimitRoute) -> RestRateLimitKey {
        match self.route_buckets.get(&route.family) {
            Some(bucket) => RestRateLimitKey::Bucket {
                bucket: bucket.clone(),
                major_parameter: route.major_parameter.clone(),
            },
            None => RestRateLimitKey::Route {
                family: route.family.clone(),
                major_parameter: route.major_parameter.clone(),
            },
        }
    }

    fn prune_expired(&mut self, now: Instant) {
        if self.global_until.is_some_and(|deadline| deadline <= now) {
            self.global_until = None;
        }
        self.windows
            .retain(|_, window| window.reset_at.is_none_or(|deadline| deadline > now));
    }

    fn update_window(
        &mut self,
        key: RestRateLimitKey,
        remaining: Option<u32>,
        reset_at: Option<Instant>,
        now: Instant,
    ) {
        let window = self.windows.entry(key).or_default();
        if window.reset_at.is_some_and(|deadline| deadline <= now) {
            *window = RestRateLimitWindow {
                remaining,
                reset_at,
            };
            return;
        }

        window.remaining = match (window.remaining, remaining) {
            (Some(current), Some(reported)) => Some(current.min(reported)),
            (None, reported) => reported,
            (current, None) => current,
        };
        window.reset_at = match (window.reset_at, reset_at) {
            (Some(current), Some(reported)) => Some(current.max(reported)),
            (None, reported) => reported,
            (current, None) => current,
        };
    }
}

impl RestRateLimitRoute {
    fn from_request(request: &reqwest::Request) -> Self {
        let segments: Vec<&str> = request
            .url()
            .path()
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let webhook_index = segments.iter().position(|segment| *segment == "webhooks");
        let mut normalized = Vec::with_capacity(segments.len());
        let mut major_parts = Vec::new();

        for (index, segment) in segments.iter().enumerate() {
            let previous = index.checked_sub(1).and_then(|index| segments.get(index));
            let is_major_id = matches!(previous, Some(&"channels" | &"guilds" | &"webhooks"));
            let is_webhook_token = webhook_index.is_some_and(|webhook| index == webhook + 2);
            if is_major_id || is_webhook_token {
                normalized.push(if is_webhook_token {
                    ":major-token"
                } else {
                    ":major"
                });
                major_parts.push((*segment).to_owned());
            } else if previous == Some(&"reactions") {
                normalized.push(":reaction");
            } else if segment.chars().all(|character| character.is_ascii_digit()) {
                normalized.push(":id");
            } else {
                normalized.push(segment);
            }
        }

        Self {
            family: RestRateLimitRouteFamily {
                method: request.method().as_str().to_owned(),
                template: format!("/{}", normalized.join("/")),
            },
            major_parameter: if major_parts.is_empty() {
                "none".to_owned()
            } else {
                major_parts.join("/")
            },
        }
    }
}

impl RestRateLimitPermit {
    fn record_response(mut self, headers: &HeaderMap, status: StatusCode) {
        self.limiter.finish(
            &self.route,
            &self.key,
            self.probe,
            RestRateLimitResponse {
                headers,
                status,
                now: Instant::now(),
                wall_clock: SystemTime::now(),
            },
        );
        self.completed = true;
    }
}

impl Drop for RestRateLimitPermit {
    fn drop(&mut self) {
        if self.probe && !self.completed {
            self.limiter.release_probe(&self.key);
        }
    }
}

fn rate_limit_reset_at(
    headers: &HeaderMap,
    now: Instant,
    wall_clock: SystemTime,
) -> Option<Instant> {
    if let Some(delay) = header_f64(headers, "retry-after").and_then(rate_limit_delay) {
        return Some(now + delay);
    }
    if let Some(delay) = header_f64(headers, "x-ratelimit-reset-after").and_then(rate_limit_delay) {
        return Some(now + delay);
    }

    let reset_epoch = header_f64(headers, "x-ratelimit-reset")?;
    let current_epoch = wall_clock.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    rate_limit_delay((reset_epoch - current_epoch).max(0.0)).map(|delay| now + delay)
}

fn rate_limit_delay(seconds: f64) -> Option<Duration> {
    const MAX_RATE_LIMIT_DELAY: Duration = Duration::from_secs(24 * 60 * 60);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(
        seconds.min(MAX_RATE_LIMIT_DELAY.as_secs_f64()),
    ))
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    header_string(headers, name)?.parse().ok()
}

fn header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
    header_string(headers, name)?.parse().ok()
}

fn header_bool(headers: &HeaderMap, name: &str) -> bool {
    header_string(headers, name).is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

#[derive(Clone, Copy, Debug)]
struct ForbiddenCircuit {
    consecutive_forbidden: u8,
    last_forbidden: Instant,
    open_until: Option<Instant>,
}

impl ForbiddenCircuit {
    fn new(now: Instant) -> Self {
        Self {
            consecutive_forbidden: 0,
            last_forbidden: now,
            open_until: None,
        }
    }
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
    let parsed_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<Value>(body).ok());
    let discord_code = parsed_body.as_ref().and_then(discord_error_code);
    let captcha_challenge = body
        .as_deref()
        .and_then(|body| super::captcha::parse_captcha_challenge(status, body));
    let captcha_fields_present = parsed_body.as_ref().is_some_and(|body| {
        body.get("captcha_key").is_some()
            || body.get("captcha_sitekey").is_some()
            || body.get("captcha_service").is_some()
    });
    let retry_after_millis = (status == StatusCode::TOO_MANY_REQUESTS).then(|| {
        let retry_after = retry_after_header
            .or_else(|| {
                parsed_body
                    .as_ref()
                    .and_then(|body| body.get("retry_after").and_then(Value::as_f64))
            })
            .unwrap_or(1.0)
            .max(0.0);
        seconds_to_millis_ceil(retry_after)
    });
    logging::debug(
        "rest",
        format!(
            "request rejected action={label:?} status={} discord_code={} captcha={} retry_after_ms={}",
            status.as_u16(),
            discord_code.as_deref().unwrap_or("none"),
            captcha_challenge.is_some() || captcha_fields_present,
            retry_after_millis
                .map(|delay| delay.to_string())
                .as_deref()
                .unwrap_or("none")
        ),
    );
    if status == StatusCode::UNAUTHORIZED {
        return AppError::DiscordAuthenticationStopped;
    }
    if let Some(retry_after_millis) = retry_after_millis {
        return AppError::DiscordRateLimited {
            action: label.to_owned(),
            retry_after_millis,
        };
    }
    if captcha_challenge.is_some() {
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
        return match discord_error_code(&value) {
            Some(code) => format!("{message} (Discord code {code})"),
            None => message,
        };
    }

    truncate_error_body(body)
}

fn discord_error_code(body: &Value) -> Option<String> {
    body.get("code").and_then(|code| {
        code.as_u64()
            .map(|code| code.to_string())
            .or_else(|| code.as_str().map(str::to_owned))
    })
}

fn rest_error_kind(error: &AppError) -> &'static str {
    match error {
        AppError::DiscordAuthenticationStopped => "authentication_stopped",
        AppError::DiscordRateLimited { .. } => "rate_limited",
        AppError::DiscordRequestCircuitOpen { .. } => "circuit_open",
        AppError::CaptchaRequired { .. } => "captcha_required",
        AppError::DiscordRequest(_) => "request_failed",
        _ => "other",
    }
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
