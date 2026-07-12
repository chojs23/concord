use super::fingerprint::{ClientFingerprint, discord_http_client, discord_rest_headers};

pub(super) const DISCORD_ORIGIN: &str = "https://discord.com";
pub(super) const DISCORD_LOGIN_REFERER: &str = "https://discord.com/login";

pub(super) fn discord_web_client(fingerprint: &ClientFingerprint) -> reqwest::Client {
    discord_http_client(fingerprint)
}

pub(super) fn discord_login_headers(fingerprint: &ClientFingerprint) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderValue, REFERER};

    let mut headers = discord_rest_headers(fingerprint);
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_LOGIN_REFERER));
    headers
}

#[cfg(test)]
mod tests {
    use reqwest::header::REFERER;

    use super::*;
    use crate::discord::fingerprint::{CLIENT_BUILD_NUMBER, discord_rest_headers};

    #[test]
    fn login_headers_share_the_rest_fingerprint() {
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
    }
}
