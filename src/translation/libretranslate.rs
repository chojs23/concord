use serde::{Deserialize, Serialize};

use super::{
    TranslationError, TranslationFuture, TranslationProvider, TranslationRequest,
    TranslationResult, parse_endpoint,
};

const PROVIDER: &str = "LibreTranslate";

pub(super) struct LibreTranslateProvider {
    http: wreq::Client,
    endpoint: url::Url,
    api_key: Option<String>,
}

impl LibreTranslateProvider {
    pub(super) fn new(
        http: wreq::Client,
        endpoint: &str,
        api_key: Option<String>,
    ) -> Result<Self, TranslationError> {
        Ok(Self {
            http,
            endpoint: parse_endpoint(PROVIDER, endpoint)?,
            api_key,
        })
    }

    fn request(&self, request: TranslationRequest<'_>) -> wreq::RequestBuilder {
        self.http
            .post(self.endpoint.as_str())
            .json(&LibreTranslateRequest {
                q: request.text,
                source: "auto",
                target: request.target_language,
                format: "text",
                api_key: self.api_key.as_deref(),
            })
    }
}

impl TranslationProvider for LibreTranslateProvider {
    fn translate<'a>(&'a self, request: TranslationRequest<'a>) -> TranslationFuture<'a> {
        Box::pin(async move {
            let response =
                self.request(request)
                    .send()
                    .await
                    .map_err(|source| TranslationError::Request {
                        provider: PROVIDER,
                        source,
                    })?;
            if !response.status().is_success() {
                return Err(TranslationError::Http {
                    provider: PROVIDER,
                    status: response.status(),
                });
            }
            let response = response
                .json::<LibreTranslateResponse>()
                .await
                .map_err(|source| TranslationError::InvalidResponse {
                    provider: PROVIDER,
                    source,
                })?;
            if response.translated_text.trim().is_empty() {
                return Err(TranslationError::EmptyResponse { provider: PROVIDER });
            }
            Ok(TranslationResult {
                text: response.translated_text,
            })
        })
    }
}

#[derive(Serialize)]
struct LibreTranslateRequest<'a> {
    q: &'a str,
    source: &'static str,
    target: &'a str,
    format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'a str>,
}

#[derive(Deserialize)]
struct LibreTranslateResponse {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn request_and_response_follow_libretranslate_contract() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let provider = LibreTranslateProvider::new(
            wreq::Client::new(),
            "http://127.0.0.1:5000/translate",
            Some("secret".to_owned()),
        )
        .expect("test endpoint is valid");

        let mut request = provider
            .request(TranslationRequest {
                text: "hello",
                target_language: "ko",
            })
            .build()
            .expect("request should build");

        let body = request
            .body_mut()
            .take()
            .expect("request has a JSON body")
            .collect()
            .await
            .expect("request body is readable")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body).expect("body should be JSON");
        assert_eq!(
            body,
            serde_json::json!({
                "q": "hello",
                "source": "auto",
                "target": "ko",
                "format": "text",
                "api_key": "secret"
            })
        );

        let response: LibreTranslateResponse = serde_json::from_value(serde_json::json!({
            "translatedText": "안녕하세요",
            "detectedLanguage": { "confidence": 100.0, "language": "en" }
        }))
        .expect("response should parse");

        assert_eq!(response.translated_text, "안녕하세요");
    }
}
