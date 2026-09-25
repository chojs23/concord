//! Direct, opt-in KLIPY API access, separate from Discord authentication.
use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::{Client, Url};
use serde::Deserialize;

use crate::config::KlipyOptions;

type Result<T> = std::result::Result<T, String>;
const API_ROOT: &str = "https://api.klipy.com/api/v1/";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct KlipyClient {
    http: Client,
    api_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Gif {
    #[serde(rename = "type")]
    kind: Option<String>,
    pub slug: String,
    pub title: String,
    pub file: BTreeMap<String, Formats>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Formats {
    pub gif: Option<Media>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Media {
    pub url: String,
}

impl Gif {
    pub fn media_url(&self, preview: bool) -> Option<&str> {
        let sizes = if preview {
            ["xs", "sm", "md", "hd"]
        } else {
            ["hd", "md", "sm", "xs"]
        };
        sizes.into_iter().find_map(|size| {
            let media = self.file.get(size)?.gif.as_ref()?;
            valid_media_url(&media.url).then_some(media.url.as_str())
        })
    }
}

// Keep the original string, including delivery parameters, for both loading and sharing.
fn valid_media_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[derive(Debug, Deserialize)]
pub(crate) struct GifPage {
    pub data: Vec<Gif>,
    pub has_next: bool,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    result: bool,
    data: Option<T>,
}

impl KlipyClient {
    pub(crate) fn new(options: &KlipyOptions) -> Result<Self> {
        let env = options.api_key_env.as_deref().unwrap_or("KLIPY_API_KEY");
        let api_key = std::env::var(env)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| options.api_key.clone().filter(|s| !s.trim().is_empty()))
            .ok_or_else(|| format!("Set {env} or [klipy].api_key to use GIF search"))?;
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("concord/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| "Could not initialize KLIPY".to_owned())?;
        Ok(Self { http, api_key })
    }

    fn endpoint(&self, action: &str, slug: Option<&str>) -> Url {
        let mut url = Url::parse(API_ROOT).expect("static API URL");
        {
            let mut path = url
                .path_segments_mut()
                .expect("API URL supports path segments");
            path.pop_if_empty()
                .push(&self.api_key)
                .push("gifs")
                .push(action);
            if let Some(slug) = slug {
                path.push(slug);
            }
        }
        url
    }

    fn search_request(&self, query: &str, page: u32) -> reqwest::RequestBuilder {
        let endpoint = self.endpoint(
            if query.is_empty() {
                "trending"
            } else {
                "search"
            },
            None,
        );
        let mut request = self.http.get(endpoint).query(&[
            ("page", page.to_string()),
            ("per_page", "8".to_owned()),
            ("format_filter", "gif".to_owned()),
        ]);
        if !query.is_empty() {
            request = request.query(&[("q", query)]);
        }
        request
    }

    pub(crate) async fn search(&self, query: &str, page: u32) -> Result<GifPage> {
        let response = self
            .search_request(query, page)
            .send()
            .await
            .map_err(api_error)?;
        let bytes = response_bytes(response, MAX_RESPONSE_BYTES).await?;
        parse_page(&bytes)
    }

    pub(crate) async fn share(&self, slug: &str, query: &str) -> Result<()> {
        let response = self
            .http
            .post(self.endpoint("share", Some(slug)))
            .json(&serde_json::json!({"q": query}))
            .send()
            .await
            .map_err(api_error)?;
        let bytes = response_bytes(response, MAX_RESPONSE_BYTES).await?;
        let result: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid KLIPY share response".to_owned())?;
        if result.get("result").and_then(|v| v.as_bool()) == Some(true) {
            Ok(())
        } else {
            Err("KLIPY could not register the share".to_owned())
        }
    }

    pub(crate) async fn preview(&self, url: &str) -> Result<Vec<u8>> {
        if !valid_media_url(url) {
            return Err("Invalid preview URL".to_owned());
        }
        let response = self.http.get(url).send().await.map_err(api_error)?;
        response_bytes(response, MAX_PREVIEW_BYTES).await
    }
}

fn parse_page(bytes: &[u8]) -> Result<GifPage> {
    // Unsupported/malformed results fail the whole page instead of silently
    // filtering or changing the provider's result order (e.g. ads-enabled keys).
    let response: ApiResponse<GifPage> = serde_json::from_slice(bytes)
        .map_err(|_| "Unsupported KLIPY response; use a key with ads disabled".to_owned())?;
    if !response.result {
        return Err("KLIPY search failed".to_owned());
    }
    let page = response
        .data
        .ok_or_else(|| "Missing KLIPY results".to_owned())?;
    if page
        .data
        .iter()
        .any(|gif| gif.kind.as_deref() == Some("ad"))
    {
        return Err("KLIPY ads are unsupported; use a key with ads disabled".to_owned());
    }
    Ok(page)
}

fn api_error(error: reqwest::Error) -> String {
    // The API key is a URL path segment. Never log reqwest's URL or response bodies.
    if error.is_timeout() {
        "KLIPY request timed out".to_owned()
    } else {
        "KLIPY request failed".to_owned()
    }
}

async fn response_bytes(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    match response.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err("KLIPY rejected the API key".to_owned()),
        429 => return Err("KLIPY rate limit reached; try again later".to_owned()),
        status => return Err(format!("KLIPY returned HTTP {status}")),
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("KLIPY response is too large".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(api_error)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("KLIPY response is too large".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn client() -> KlipyClient {
        let _ = rustls::crypto::ring::default_provider().install_default();
        KlipyClient::new(&KlipyOptions {
            api_key: Some("private/key?".to_owned()),
            api_key_env: Some("CONCORD_KLIPY_TEST_UNSET_KEY".to_owned()),
        })
        .unwrap()
    }

    #[test]
    fn klipy_requests_encode_key_and_verbatim_query_without_discord_auth() {
        let client = client();
        let request = client.search_request(" cats & 🐈+? ", 3).build().unwrap();
        assert_eq!(request.url().path(), "/api/v1/private%2Fkey%3F/gifs/search");
        let params = request.url().query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(params.get("q").unwrap(), " cats & 🐈+? ");
        assert_eq!(params.get("page").unwrap(), "3");
        assert_eq!(params.get("format_filter").unwrap(), "gif");
        assert!(!request.headers().contains_key("authorization"));
        let trending = client.search_request("", 1).build().unwrap();
        assert!(trending.url().path().ends_with("/gifs/trending"));
        assert!(!trending.url().query_pairs().any(|(key, _)| key == "q"));
        assert!(
            client
                .endpoint("share", Some("a/b?"))
                .path()
                .ends_with("/share/a%2Fb%3F")
        );
    }

    #[test]
    fn klipy_pages_preserve_order_and_original_delivery_urls() {
        let page = json!({"result": true, "data": {"has_next": true, "data": [
            {"slug":"first", "title":"First", "file": {
                "xs":{"gif":{"url":"https://static.klipy.com/small.gif?delivery=a%2Fb&x=1"}},
                "hd":{"gif":{"url":"https://static.klipy.com/full.gif?delivery=a%2Fb&x=1"}}
            }},
            {"slug":"second", "title":"Second", "file": {"md":{"gif":{"url":"https://static1.klipy.com/second.gif"}}}}
        ]}});
        let parsed = parse_page(&serde_json::to_vec(&page).unwrap()).unwrap();
        assert!(parsed.has_next);
        assert_eq!(
            parsed
                .data
                .iter()
                .map(|gif| gif.slug.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(
            parsed.data[0].media_url(true),
            Some("https://static.klipy.com/small.gif?delivery=a%2Fb&x=1")
        );
        assert_eq!(
            parsed.data[0].media_url(false),
            Some("https://static.klipy.com/full.gif?delivery=a%2Fb&x=1")
        );
        assert_eq!(
            parsed.data[1].media_url(true),
            parsed.data[1].media_url(false)
        );
    }

    #[test]
    fn klipy_empty_malformed_and_unsupported_pages_are_explicit() {
        assert!(
            parse_page(br#"{"result":true,"data":{"has_next":false,"data":[]}}"#)
                .unwrap()
                .data
                .is_empty()
        );
        for bytes in [
            br#"{"result":false}"#.as_slice(),
            br#"{"result":true,"data":{"has_next":false,"data":[{"type":"ad"}]}}"#,
            b"not json",
        ] {
            assert!(parse_page(bytes).is_err());
        }
        assert!(parse_page(br#"{"result":true,"data":{"has_next":false,"data":[{"type":"ad","slug":"ad","title":"Ad","file":{}}]}}"#).is_err());
        for url in [
            "file:///tmp/test.gif",
            "http://example.com/test.gif",
            "https://secret@example.com/image.gif",
            "bad",
        ] {
            assert!(!valid_media_url(url));
        }
    }

    async fn response(status: u16, body: &'static str) -> reqwest::Response {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await;
            socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        client()
            .http
            .get(format!("http://{addr}/private-api-key"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn klipy_http_errors_and_response_limits_do_not_expose_secrets() {
        for (status, expected) in [
            (401, "API key"),
            (403, "API key"),
            (429, "rate limit"),
            (500, "HTTP 500"),
        ] {
            let error = response_bytes(response(status, "private-api-key").await, 32)
                .await
                .unwrap_err();
            assert!(error.contains(expected));
            assert!(!error.contains("private-api-key"));
        }
        assert!(
            response_bytes(response(200, "oversized").await, 3)
                .await
                .unwrap_err()
                .contains("too large")
        );
        assert_eq!(
            response_bytes(response(200, "ok").await, 3).await.unwrap(),
            b"ok"
        );
    }
}
