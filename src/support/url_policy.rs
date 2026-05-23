pub(crate) fn normalize_openable_url(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_openable_url;

    #[test]
    fn openable_urls_allow_http_and_https() {
        assert_eq!(
            normalize_openable_url("https://example.com/?a=1&b=2").as_deref(),
            Some("https://example.com/?a=1&b=2")
        );
        assert_eq!(
            normalize_openable_url("http://example.com/path").as_deref(),
            Some("http://example.com/path")
        );
    }

    #[test]
    fn openable_urls_reject_non_web_schemes() {
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "discord://-/channels/1/2/3",
            "not a url",
        ] {
            assert_eq!(normalize_openable_url(value), None, "{value}");
        }
    }
}
