pub(crate) fn normalize_openable_url(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_openable_url;

    #[test]
    fn only_http_and_https_urls_are_openable() {
        for (value, expected) in [
            (
                "https://example.com/?a=1&b=2",
                Some("https://example.com/?a=1&b=2"),
            ),
            ("http://example.com/path", Some("http://example.com/path")),
            ("javascript:alert(1)", None),
            ("file:///etc/passwd", None),
            ("discord://-/channels/1/2/3", None),
            ("not a url", None),
        ] {
            assert_eq!(
                normalize_openable_url(value).as_deref(),
                expected,
                "{value}"
            );
        }
    }
}
