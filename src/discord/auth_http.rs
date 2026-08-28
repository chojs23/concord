use std::sync::Arc;

use super::fingerprint::{
    CLIENT_BUILD_NUMBER, ClientFingerprint, DISCORD_ORIGIN, discord_api_request_headers,
    discord_http_client, insert_fingerprint_header,
};

pub(super) const DISCORD_LOGIN_REFERER: &str = "https://discord.com/login";

#[derive(Clone)]
pub(crate) struct DiscordAuthSession {
    fingerprint: Arc<ClientFingerprint>,
    http: reqwest::Client,
}

impl DiscordAuthSession {
    pub(crate) fn fallback() -> Self {
        Self::new(Arc::new(ClientFingerprint::new(CLIENT_BUILD_NUMBER)))
    }

    pub(crate) fn new(fingerprint: Arc<ClientFingerprint>) -> Self {
        let http = discord_http_client(&fingerprint);
        Self { fingerprint, http }
    }

    pub(crate) fn with_http(fingerprint: Arc<ClientFingerprint>, http: reqwest::Client) -> Self {
        Self { fingerprint, http }
    }

    pub(crate) fn fingerprint(&self) -> &ClientFingerprint {
        &self.fingerprint
    }

    pub(crate) fn fingerprint_arc(&self) -> Arc<ClientFingerprint> {
        Arc::clone(&self.fingerprint)
    }

    pub(crate) fn http(&self) -> reqwest::Client {
        self.http.clone()
    }
}

pub(super) fn discord_login_headers(fingerprint: &ClientFingerprint) -> reqwest::header::HeaderMap {
    use reqwest::header::{CACHE_CONTROL, HeaderValue, ORIGIN, PRAGMA};

    let mut headers = discord_api_request_headers(fingerprint, DISCORD_LOGIN_REFERER);
    headers.insert(ORIGIN, HeaderValue::from_static(DISCORD_ORIGIN));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    insert_fingerprint_header(&mut headers, fingerprint);
    headers
}

#[cfg(test)]
mod tests {
    use reqwest::header::{CACHE_CONTROL, ORIGIN, PRAGMA, REFERER};

    use super::*;
    use crate::discord::fingerprint::{CLIENT_BUILD_NUMBER, discord_rest_headers};

    #[test]
    fn login_headers_keep_the_explicit_auth_policy() {
        let fingerprint = ClientFingerprint::new(CLIENT_BUILD_NUMBER);
        let login = discord_login_headers(&fingerprint);
        let rest = discord_rest_headers(&fingerprint);

        for name in [
            "user-agent",
            "accept-language",
            "X-Discord-Locale",
            "X-Discord-Timezone",
            "X-Super-Properties",
        ] {
            assert_eq!(login.get(name), rest.get(name));
        }
        assert_eq!(
            login.get(REFERER).and_then(|value| value.to_str().ok()),
            Some(DISCORD_LOGIN_REFERER)
        );
        assert_eq!(
            login.get(ORIGIN).and_then(|value| value.to_str().ok()),
            Some(DISCORD_ORIGIN)
        );
        assert_eq!(
            login
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-cache")
        );
        assert_eq!(
            login.get(PRAGMA).and_then(|value| value.to_str().ok()),
            Some("no-cache")
        );
    }
}
