pub(super) const DISCORD_ORIGIN: &str = "https://discord.com";
pub(super) const DISCORD_LOGIN_REFERER: &str = "https://discord.com/login";
pub(super) const DISCORD_WEB_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36";

pub(super) fn discord_web_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(DISCORD_WEB_USER_AGENT)
        .build()
}

pub(super) fn discord_login_headers() -> reqwest::header::HeaderMap {
    use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, ORIGIN, REFERER};

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(ORIGIN, HeaderValue::from_static(DISCORD_ORIGIN));
    headers.insert(REFERER, HeaderValue::from_static(DISCORD_LOGIN_REFERER));
    headers.insert("X-Discord-Locale", HeaderValue::from_static("en-US"));
    headers
}
