use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use thiserror::Error;

use crate::config::{TranslationOptions, TranslationProviderKind};

mod deepl;
mod libretranslate;

const DEFAULT_DEEPL_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
const DEFAULT_LIBRETRANSLATE_ENDPOINT: &str = "http://127.0.0.1:5000/translate";
const DEFAULT_DEEPL_API_KEY_ENV: &str = "DEEPL_API_KEY";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) struct TranslationRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) target_language: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TranslationResult {
    pub(crate) text: String,
}

type TranslationFuture<'a> = Pin<
    Box<dyn Future<Output = std::result::Result<TranslationResult, TranslationError>> + Send + 'a>,
>;

trait TranslationProvider: Send + Sync {
    fn translate<'a>(&'a self, request: TranslationRequest<'a>) -> TranslationFuture<'a>;
}

#[derive(Clone)]
pub(crate) struct TranslationService {
    provider: Option<Arc<dyn TranslationProvider>>,
    initialization_error: Option<String>,
}

impl TranslationService {
    pub(crate) fn from_options(options: TranslationOptions) -> Self {
        let Some(kind) = options.provider else {
            return Self {
                provider: None,
                initialization_error: None,
            };
        };

        match provider_from_options(kind, &options) {
            Ok(provider) => Self {
                provider: Some(provider),
                initialization_error: None,
            },
            Err(error) => Self {
                provider: None,
                initialization_error: Some(error.to_string()),
            },
        }
    }

    pub(crate) async fn translate(
        &self,
        text: &str,
        target_language: &str,
    ) -> std::result::Result<TranslationResult, TranslationError> {
        if let Some(error) = &self.initialization_error {
            return Err(TranslationError::Configuration(error.clone()));
        }
        let provider = self
            .provider
            .as_ref()
            .ok_or(TranslationError::NotConfigured)?;
        let target_language = target_language.trim();
        if target_language.is_empty() {
            return Err(TranslationError::MissingTargetLanguage);
        }
        provider
            .translate(TranslationRequest {
                text,
                target_language,
            })
            .await
    }
}

fn provider_from_options(
    kind: TranslationProviderKind,
    options: &TranslationOptions,
) -> std::result::Result<Arc<dyn TranslationProvider>, TranslationError> {
    let http = crate::support::tls::http_client_builder()
        .map_err(TranslationError::Configuration)?
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("concord/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|source| TranslationError::Client { source })?;

    match kind {
        TranslationProviderKind::DeepL => {
            let api_key_env = api_key_environment(options, Some(DEFAULT_DEEPL_API_KEY_ENV))
                .expect("DeepL always has a default API key environment variable");
            let api_key = api_key_from_options(options, Some(DEFAULT_DEEPL_API_KEY_ENV))
                .ok_or_else(|| TranslationError::MissingApiKey {
                    provider: kind.label(),
                    env: api_key_env.to_owned(),
                })?;
            let endpoint = options
                .endpoint
                .as_deref()
                .unwrap_or(DEFAULT_DEEPL_ENDPOINT);
            Ok(Arc::new(deepl::DeepLProvider::new(
                http, endpoint, api_key,
            )?))
        }
        TranslationProviderKind::LibreTranslate => {
            let api_key = api_key_from_options(options, None);
            let endpoint = options
                .endpoint
                .as_deref()
                .unwrap_or(DEFAULT_LIBRETRANSLATE_ENDPOINT);
            Ok(Arc::new(libretranslate::LibreTranslateProvider::new(
                http, endpoint, api_key,
            )?))
        }
    }
}

fn api_key_from_options(
    options: &TranslationOptions,
    default_environment: Option<&str>,
) -> Option<String> {
    let environment_key =
        api_key_environment(options, default_environment).and_then(|name| std::env::var(name).ok());
    select_api_key(environment_key.as_deref(), options.api_key.as_deref())
}

fn api_key_environment<'a>(
    options: &'a TranslationOptions,
    default_environment: Option<&'a str>,
) -> Option<&'a str> {
    options
        .api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or(default_environment)
}

fn select_api_key(environment_key: Option<&str>, config_key: Option<&str>) -> Option<String> {
    [environment_key, config_key]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|key| !key.is_empty())
        .map(str::to_owned)
}

fn parse_endpoint(
    provider: &'static str,
    endpoint: &str,
) -> std::result::Result<url::Url, TranslationError> {
    url::Url::parse(endpoint.trim()).map_err(|error| TranslationError::InvalidEndpoint {
        provider,
        message: error.to_string(),
    })
}

#[derive(Debug, Error)]
pub(crate) enum TranslationError {
    #[error("translation is not configured")]
    NotConfigured,
    #[error("translation target language is not configured")]
    MissingTargetLanguage,
    #[error(
        "{provider} API key is not configured; set translation.api_key or environment variable {env}"
    )]
    MissingApiKey { provider: &'static str, env: String },
    #[error("translation configuration is invalid: {0}")]
    Configuration(String),
    #[error("could not create the translation HTTP client")]
    Client {
        #[source]
        source: wreq::Error,
    },
    #[error("{provider} endpoint is not a valid URL: {message}")]
    InvalidEndpoint {
        provider: &'static str,
        message: String,
    },
    #[error("{provider} request failed")]
    Request {
        provider: &'static str,
        #[source]
        source: wreq::Error,
    },
    #[error("{provider} returned HTTP {status}")]
    Http {
        provider: &'static str,
        status: wreq::StatusCode,
    },
    #[error("{provider} returned an invalid response")]
    InvalidResponse {
        provider: &'static str,
        #[source]
        source: wreq::Error,
    },
    #[error("{provider} returned no translation")]
    EmptyResponse { provider: &'static str },
}

#[cfg(test)]
mod tests {
    use super::select_api_key;

    #[test]
    fn environment_api_key_overrides_config_with_empty_values_ignored() {
        let cases = [
            (Some(" environment "), Some("config"), Some("environment")),
            (Some(""), Some(" config "), Some("config")),
            (None, Some("config"), Some("config")),
            (Some("  "), Some("  "), None),
        ];

        for (environment, config, expected) in cases {
            assert_eq!(select_api_key(environment, config).as_deref(), expected);
        }
    }
}
