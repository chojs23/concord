use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, RwLock,
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{
    Url,
    cookie::CookieStore,
    header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue, ORIGIN,
        PRAGMA, REFERER, USER_AGENT,
    },
};
use reqwest_cookie_store::{
    CookieStore as PersistentCookieStore, CookieStoreMutex as PersistentCookieStoreMutex,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{paths, support::private_file};

use super::auth_http::DISCORD_ORIGIN;
use super::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

/// Fallback used only when the live build number cannot be fetched at startup.
pub(super) const CLIENT_BUILD_NUMBER: u64 = 580_004;
pub(super) const CLIENT_BROWSER: &str = "Chrome";
pub(super) const CLIENT_BROWSER_VERSION: &str = "150.0.0.0";

const DISCORD_CHANNELS_REFERER: &str = "https://discord.com/channels/@me";
const DISCORD_ROOT_REFERER: &str = "https://discord.com/";
const DISCORD_EXPERIMENTS_URL: &str =
    "https://discord.com/api/v9/experiments?with_guild_experiments=true";
const DISCORD_APEX_EXPERIMENTS_URL: &str = "https://discord.com/api/v9/apex/experiments?surface=2";
const COOKIE_PERSIST_DEBOUNCE: Duration = Duration::from_millis(250);
pub(super) const DISCORD_LOCALE: &str = "en-US";
pub(super) const DISCORD_REFERRER_CURRENT: &str = "";
pub(super) const DISCORD_REFERRING_DOMAIN_CURRENT: &str = "";

#[derive(Clone, Debug)]
pub(crate) struct ClientFingerprint {
    pub(super) os: &'static str,
    pub(super) os_version: String,
    pub(super) system_locale: String,
    pub(super) timezone: String,
    pub(super) user_agent: String,
    pub(super) client_build_number: u64,
    anonymous_fingerprint: Option<String>,
    installation_id: Arc<RwLock<Option<String>>>,
    launch_signature: String,
    client_launch_id: String,
    client_heartbeat_session_id: String,
}

impl ClientFingerprint {
    pub(super) fn new(client_build_number: u64) -> Self {
        let os = operating_system();
        let os_arch = operating_system_arch();
        let os_version = chrome_os_version(os, &operating_system_version());
        Self {
            os,
            user_agent: web_user_agent(os, &os_version, os_arch),
            os_version,
            system_locale: system_locale(),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_owned()),
            client_build_number,
            anonymous_fingerprint: None,
            installation_id: Arc::new(RwLock::new(None)),
            launch_signature: generate_launch_signature(),
            client_launch_id: Uuid::new_v4().to_string(),
            client_heartbeat_session_id: Uuid::new_v4().to_string(),
        }
    }

    fn apply_session_identifiers(&mut self, identifiers: SessionIdentifiers) {
        let identifiers = identifiers.sanitized();
        self.anonymous_fingerprint = identifiers.fingerprint;
        *self
            .installation_id
            .write()
            .expect("installation id lock is not poisoned") = identifiers.installation;
    }

    pub(super) fn installation_id(&self) -> Option<String> {
        self.installation_id
            .read()
            .expect("installation id lock is not poisoned")
            .clone()
    }

    pub(super) fn update_installation_id(&self, installation_id: &str) -> Result<bool, String> {
        let Some(installation_id) = valid_session_identifier(Some(installation_id.to_owned()))
        else {
            return Ok(false);
        };
        let mut current = self
            .installation_id
            .write()
            .map_err(|_| "installation id lock is poisoned".to_owned())?;
        if current.as_deref() == Some(installation_id.as_str()) {
            return Ok(false);
        }
        *current = Some(installation_id.clone());
        drop(current);
        save_session_identifiers(&SessionIdentifiers {
            fingerprint: self.anonymous_fingerprint.clone(),
            installation: Some(installation_id),
        })?;
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn set_installation_id_for_test(&self, installation_id: &str) {
        *self
            .installation_id
            .write()
            .expect("installation id lock is not poisoned") = Some(installation_id.to_owned());
    }
}

#[derive(Serialize)]
struct SuperProperties<'a> {
    os: &'a str,
    browser: &'static str,
    device: &'static str,
    system_locale: &'a str,
    browser_user_agent: &'a str,
    browser_version: &'static str,
    os_version: &'a str,
    referrer: &'static str,
    referring_domain: &'static str,
    referrer_current: &'static str,
    referring_domain_current: &'static str,
    release_channel: &'static str,
    client_build_number: u64,
    client_event_source: Option<String>,
    has_client_mods: bool,
    client_launch_id: &'a str,
    launch_signature: &'a str,
    client_app_state: &'static str,
    client_heartbeat_session_id: &'a str,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct SessionIdentifiers {
    fingerprint: Option<String>,
    installation: Option<String>,
}

impl SessionIdentifiers {
    fn sanitized(self) -> Self {
        Self {
            fingerprint: valid_session_identifier(self.fingerprint),
            installation: valid_session_identifier(self.installation),
        }
    }

    fn merge_fetched(&mut self, other: Self) {
        let other = other.sanitized();
        if other.fingerprint.is_some() {
            self.fingerprint = other.fingerprint;
        }
        if other.installation.is_some() {
            self.installation = other.installation;
        }
    }

    fn is_empty(&self) -> bool {
        self.fingerprint.is_none() && self.installation.is_none()
    }
}

/// Creates the login-session fingerprint after reading Discord's current web
/// build. The returned HTTP client retains the cookies from that bootstrap
/// request and is reused for authentication and REST.
pub(crate) async fn load_client_fingerprint_and_http() -> (Arc<ClientFingerprint>, reqwest::Client)
{
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
    let mut fingerprint = ClientFingerprint::new(client_build_number);
    let mut identifiers = load_session_identifiers().unwrap_or_default();
    fingerprint.apply_session_identifiers(identifiers.clone());
    match fetch_session_identifiers(&client, &fingerprint).await {
        Some(fetched) => {
            identifiers.merge_fetched(fetched);
            fingerprint.apply_session_identifiers(identifiers.clone());
            if let Err(error) = save_session_identifiers(&identifiers) {
                crate::logging::debug(
                    "fingerprint",
                    format!("could not persist Discord session identifiers: {error}"),
                );
            }
        }
        None => crate::logging::debug("fingerprint", "could not fetch Discord session identifiers"),
    }
    (Arc::new(fingerprint), client)
}

pub(super) fn discord_channel_referer(
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
) -> String {
    match guild_id {
        Some(guild_id) => format!(
            "{DISCORD_ORIGIN}/channels/{}/{}",
            guild_id.get(),
            channel_id.get()
        ),
        None => format!("{DISCORD_CHANNELS_REFERER}/{}", channel_id.get()),
    }
}

fn valid_session_identifier(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty() && HeaderValue::from_str(value).is_ok())
}

fn load_session_identifiers() -> Option<SessionIdentifiers> {
    let path = paths::discord_browser_file()?;
    match load_session_identifiers_from_path(&path) {
        Ok(identifiers) => identifiers,
        Err(error) => {
            crate::logging::debug(
                "fingerprint",
                format!("could not load persisted Discord session identifiers: {error}"),
            );
            None
        }
    }
}

fn load_session_identifiers_from_path(
    path: &Path,
) -> std::result::Result<Option<SessionIdentifiers>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    toml::from_str::<SessionIdentifiers>(&content)
        .map(|identifiers| Some(identifiers.sanitized()))
        .map_err(|error| error.to_string())
}

fn save_session_identifiers(identifiers: &SessionIdentifiers) -> std::result::Result<(), String> {
    let Some(path) = paths::discord_browser_file() else {
        return Err("could not resolve user state directory".to_owned());
    };
    save_session_identifiers_to_path(&path, identifiers)
}

fn save_session_identifiers_to_path(
    path: &Path,
    identifiers: &SessionIdentifiers,
) -> std::result::Result<(), String> {
    if identifiers.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        private_file::set_private_dir_permissions(parent).map_err(|error| error.to_string())?;
    }
    let content = toml::to_string(identifiers).map_err(|error| error.to_string())?;
    private_file::write_private_file(path, &content).map_err(|error| error.to_string())
}

pub(super) fn discord_http_client(fingerprint: &ClientFingerprint) -> reqwest::Client {
    discord_http_client_with_cookie_path(fingerprint, paths::discord_cookie_file())
}

fn discord_http_client_with_cookie_path(
    fingerprint: &ClientFingerprint,
    cookie_path: Option<PathBuf>,
) -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_provider(Arc::new(LoggingCookieJar::load(cookie_path)))
        .default_headers(discord_browser_headers(fingerprint))
        .build()
        .expect("static Discord REST client configuration is valid")
}

struct LoggingCookieJar {
    inner: Arc<PersistentCookieStoreMutex>,
    persistence: Option<CookiePersistence>,
}

impl LoggingCookieJar {
    fn load(path: Option<PathBuf>) -> Self {
        let store = path
            .as_deref()
            .and_then(|path| match load_cookie_store_from_path(path) {
                Ok(store) => store,
                Err(error) => {
                    crate::logging::debug(
                        "http",
                        format!("could not load persisted Discord cookies: {error}"),
                    );
                    None
                }
            })
            .unwrap_or_default();
        let inner = Arc::new(PersistentCookieStoreMutex::new(store));
        let persistence = path.and_then(|path| spawn_cookie_persistence(Arc::clone(&inner), path));
        Self { inner, persistence }
    }
}

impl CookieStore for LoggingCookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        self.inner.set_cookies(cookie_headers, url);
        if let Some(persistence) = &self.persistence {
            persistence.request();
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let cookies = discord_cookies(self.inner.cookies(url), url);
        if crate::logging::debug_logging_enabled() {
            crate::logging::debug(
                "http",
                format!(
                    "transport cookie endpoint={:?} names={:?}",
                    url.as_str(),
                    cookie_names_for_log(cookies.as_ref())
                ),
            );
        }
        cookies
    }
}

struct CookiePersistence {
    persist_tx: Option<SyncSender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl CookiePersistence {
    fn request(&self) {
        if let Some(persist_tx) = &self.persist_tx {
            let _ = persist_tx.try_send(());
        }
    }
}

impl Drop for CookiePersistence {
    fn drop(&mut self) {
        self.persist_tx.take();
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            crate::logging::debug("http", "Discord cookie persistence worker panicked");
        }
    }
}

fn spawn_cookie_persistence(
    inner: Arc<PersistentCookieStoreMutex>,
    path: PathBuf,
) -> Option<CookiePersistence> {
    let (persist_tx, persist_rx) = sync_channel(1);
    let worker = match thread::Builder::new()
        .name("concord-cookie-persist".to_owned())
        .spawn(move || persist_cookies_after_changes(inner, path, persist_rx))
    {
        Ok(worker) => worker,
        Err(error) => {
            crate::logging::debug(
                "http",
                format!("could not start Discord cookie persistence: {error}"),
            );
            return None;
        }
    };
    Some(CookiePersistence {
        persist_tx: Some(persist_tx),
        worker: Some(worker),
    })
}

fn persist_cookies_after_changes(
    inner: Arc<PersistentCookieStoreMutex>,
    path: PathBuf,
    persist_rx: Receiver<()>,
) {
    while persist_rx.recv().is_ok() {
        let disconnected = loop {
            match persist_rx.recv_timeout(COOKIE_PERSIST_DEBOUNCE) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => break false,
                Err(RecvTimeoutError::Disconnected) => break true,
            }
        };
        if let Err(error) = persist_cookie_store(&inner, &path) {
            crate::logging::debug(
                "http",
                format!("could not persist Discord cookies: {error}"),
            );
        }
        if disconnected {
            break;
        }
    }
}

fn persist_cookie_store(
    inner: &PersistentCookieStoreMutex,
    path: &Path,
) -> std::result::Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        private_file::set_private_dir_permissions(parent).map_err(|error| error.to_string())?;
    }
    let store = inner
        .lock()
        .map_err(|_| "Discord cookie store lock is poisoned".to_owned())?;
    let mut content = Vec::new();
    cookie_store::serde::json::save_incl_expired_and_nonpersistent(&store, &mut content)
        .map_err(|error| error.to_string())?;
    drop(store);
    let content = String::from_utf8(content).map_err(|error| error.to_string())?;
    private_file::write_private_file(path, &content).map_err(|error| error.to_string())
}

fn cookie_names_for_log(cookies: Option<&HeaderValue>) -> String {
    cookies
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .filter_map(|cookie| cookie.trim().split_once('='))
                .map(|(name, _)| name)
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|names| !names.is_empty())
        .unwrap_or_else(|| "<none>".to_owned())
}

fn load_cookie_store_from_path(
    path: &Path,
) -> std::result::Result<Option<PersistentCookieStore>, String> {
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    cookie_store::serde::json::load(content.as_slice())
        .map(Some)
        .map_err(|error| error.to_string())
}

fn discord_cookies(cookies: Option<HeaderValue>, url: &Url) -> Option<HeaderValue> {
    let is_discord = url
        .host_str()
        .is_some_and(|host| host == "discord.com" || host.ends_with(".discord.com"));
    if !is_discord {
        return cookies;
    }

    let mut cookies = cookies
        .as_ref()
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|cookie| !cookie.is_empty() && !cookie.starts_with("locale="))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    cookies.push(format!("locale={DISCORD_LOCALE}"));
    HeaderValue::from_str(&cookies.join("; ")).ok()
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
    insert_chrome_client_hints(&mut headers, fingerprint.os);
    headers
}

pub(super) fn discord_rest_headers(fingerprint: &ClientFingerprint) -> HeaderMap {
    let mut headers = discord_browser_headers(fingerprint);
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
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_CHANNELS_REFERER));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert("Priority", HeaderValue::from_static("u=1, i"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert("X-Discord-Locale", HeaderValue::from_static(DISCORD_LOCALE));
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
    let installation_id = fingerprint.installation_id();
    insert_session_header(
        &mut headers,
        "X-Installation-ID",
        installation_id.as_deref(),
    );
    headers
}

pub(super) fn insert_session_headers(headers: &mut HeaderMap, fingerprint: &ClientFingerprint) {
    let installation_id = fingerprint.installation_id();
    insert_session_header(
        headers,
        "X-Fingerprint",
        fingerprint.anonymous_fingerprint.as_deref(),
    );
    insert_session_header(headers, "X-Installation-ID", installation_id.as_deref());
}

fn insert_chrome_client_hints(headers: &mut HeaderMap, os: &str) {
    headers.insert(
        "Sec-CH-UA",
        HeaderValue::from_static(
            "\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"150\", \"Google Chrome\";v=\"150\"",
        ),
    );
    headers.insert("Sec-CH-UA-Mobile", HeaderValue::from_static("?0"));
    headers.insert(
        "Sec-CH-UA-Platform",
        HeaderValue::from_static(match os {
            "Mac OS X" => "\"macOS\"",
            "Windows" => "\"Windows\"",
            _ => "\"Linux\"",
        }),
    );
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
        browser: CLIENT_BROWSER,
        device: "",
        system_locale: &fingerprint.system_locale,
        browser_user_agent: &fingerprint.user_agent,
        browser_version: CLIENT_BROWSER_VERSION,
        os_version: &fingerprint.os_version,
        referrer: "",
        referring_domain: "",
        referrer_current: DISCORD_REFERRER_CURRENT,
        referring_domain_current: DISCORD_REFERRING_DOMAIN_CURRENT,
        release_channel: "stable",
        client_build_number: fingerprint.client_build_number,
        client_event_source: None,
        has_client_mods: false,
        client_launch_id: &fingerprint.client_launch_id,
        launch_signature: &fingerprint.launch_signature,
        client_app_state: "focused",
        client_heartbeat_session_id: &fingerprint.client_heartbeat_session_id,
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

async fn fetch_session_identifiers(
    client: &reqwest::Client,
    fingerprint: &ClientFingerprint,
) -> Option<SessionIdentifiers> {
    let headers = discord_experiments_headers(fingerprint);
    let (legacy, apex) = tokio::join!(
        fetch_session_identifier(client, DISCORD_EXPERIMENTS_URL, headers.clone()),
        fetch_session_identifier(client, DISCORD_APEX_EXPERIMENTS_URL, headers),
    );
    let identifiers = SessionIdentifiers {
        fingerprint: legacy.and_then(|response| response.fingerprint),
        installation: apex.and_then(|response| response.installation),
    }
    .sanitized();
    (!identifiers.is_empty()).then_some(identifiers)
}

async fn fetch_session_identifier(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
) -> Option<SessionIdentifiers> {
    client
        .get(url)
        .headers(headers)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()
}

fn discord_experiments_headers(fingerprint: &ClientFingerprint) -> HeaderMap {
    let mut headers = discord_rest_headers(fingerprint);
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_ROOT_REFERER));
    insert_session_headers(&mut headers, fingerprint);
    headers
}

fn insert_session_header(headers: &mut HeaderMap, name: &'static str, value: Option<&str>) {
    let Some(value) = value.and_then(|value| HeaderValue::from_str(value).ok()) else {
        return;
    };
    headers.insert(name, value);
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

fn chrome_os_version(os: &str, host_os_version: &str) -> String {
    match os {
        "Windows" => "10".to_owned(),
        "Mac OS X" => "10.15.7".to_owned(),
        _ => host_os_version.to_owned(),
    }
}

fn system_locale() -> String {
    sys_locale::get_locale()
        .as_deref()
        .and_then(normalize_system_locale)
        .unwrap_or_else(|| "en-US".to_owned())
}

fn normalize_system_locale(raw: &str) -> Option<String> {
    let locale = raw.split(['.', '@']).next()?.replace('_', "-");
    if locale.eq_ignore_ascii_case("C") || locale.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    let mut subtags = locale.split('-');
    let language = subtags.next()?;
    if !(2..=8).contains(&language.len())
        || !language
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        || subtags.any(|subtag| {
            subtag.is_empty()
                || subtag.len() > 8
                || !subtag
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric())
        })
        || HeaderValue::from_str(&locale).is_err()
    {
        return None;
    }
    Some(locale)
}

pub(super) fn accept_language(locale: &str) -> String {
    let language = locale.split('-').next().unwrap_or(locale);
    if language.eq_ignore_ascii_case("en") {
        if locale == language {
            locale.to_owned()
        } else {
            format!("{locale},en;q=0.9")
        }
    } else if locale == language {
        format!("{locale},en-US;q=0.9,en;q=0.8")
    } else {
        format!("{locale},{language};q=0.9,en-US;q=0.8,en;q=0.7")
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
    let version = if cfg!(target_os = "linux") {
        command_output("uname", &["-r"])
    } else if cfg!(target_os = "macos") {
        command_output("sw_vers", &["-productVersion"])
    } else if cfg!(target_os = "windows") {
        command_output("cmd", &["/C", "ver"]).and_then(|output| windows_version(&output))
    } else {
        None
    };
    version.unwrap_or_default()
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|version| !version.is_empty())
}

fn windows_version(output: &str) -> Option<String> {
    output
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .find(|part| {
            part.contains('.')
                && part
                    .split('.')
                    .all(|component| !component.is_empty() && component.parse::<u32>().is_ok())
        })
        .map(str::to_owned)
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
