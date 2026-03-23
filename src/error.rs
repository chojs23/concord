use std::error::Error as StdError;

use thiserror::Error;
use twilight_http::error::ErrorType;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("login cancelled before a Discord token was saved")]
    LoginCancelled,
    #[error("Discord token must not be empty")]
    EmptyDiscordToken,
    #[error("Discord token is not a valid HTTP authorization header")]
    InvalidDiscordTokenHeader {
        #[source]
        source: reqwest::header::InvalidHeaderValue,
    },
    #[error("message content must not be empty")]
    EmptyMessageContent,
    #[error("message content exceeds Discord's 2000 character limit: {len}")]
    MessageTooLong { len: usize },
    #[error("Discord HTTP request failed")]
    Http(#[from] twilight_http::Error),
    #[error("failed to decode Discord response body")]
    DeserializeBody(#[from] twilight_http::response::DeserializeBodyError),
    #[error("Discord request failed: {0}")]
    DiscordRequest(String),
    #[error("terminal I/O failed")]
    Io(#[from] std::io::Error),
    #[error("QR login failed: {0}")]
    QrLogin(String),
    #[error("QR login was cancelled in the Discord mobile app")]
    QrLoginCancelled,
}

impl AppError {
    pub fn log_detail(&self) -> String {
        match self {
            Self::Http(error) => format_http_error_detail(error),
            _ => format_error_chain(self),
        }
    }
}

fn format_http_error_detail(error: &twilight_http::Error) -> String {
    let mut detail = format!(
        "Discord HTTP request failed: {}",
        format_http_error_kind(error.kind())
    );
    append_source_chain(&mut detail, error.source());
    detail
}

fn format_http_error_kind(kind: &ErrorType) -> String {
    match kind {
        ErrorType::BuildingRequest => "failed to build request".to_owned(),
        ErrorType::CreatingHeader { name } => format!("failed to create `{name}` header"),
        ErrorType::Json => "failed to serialize JSON request body".to_owned(),
        ErrorType::Parsing { body } => {
            format!(
                "failed to parse Discord response body ({} bytes)",
                body.len()
            )
        }
        ErrorType::RequestCanceled => "request was canceled".to_owned(),
        ErrorType::RequestError => "network request failed".to_owned(),
        ErrorType::RequestTimedOut => "request timed out".to_owned(),
        ErrorType::Response {
            body,
            error,
            status,
        } => format!(
            "Discord returned HTTP {status}; api_error={error}; response_body_bytes={}",
            body.len()
        ),
        ErrorType::Unauthorized => "Discord token is unauthorized or expired".to_owned(),
        ErrorType::Validation => "request failed validation before sending".to_owned(),
        _ => "unknown Twilight HTTP error type".to_owned(),
    }
}

fn format_error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut detail = error.to_string();
    append_source_chain(&mut detail, error.source());
    detail
}

fn append_source_chain(detail: &mut String, mut source: Option<&(dyn StdError + 'static)>) {
    let mut index = 1;
    while let Some(error) = source {
        detail.push_str(&format!("; source[{index}]={error}"));
        source = error.source();
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use twilight_http::{api_error::ApiError, error::ErrorType, response::StatusCode};

    use super::{AppError, format_http_error_kind};

    #[test]
    fn non_http_log_detail_includes_source_chain() {
        let error = AppError::InvalidDiscordTokenHeader {
            source: reqwest::header::HeaderValue::from_str("bad\nvalue")
                .expect_err("newline makes header invalid"),
        };

        let detail = error.log_detail();

        assert!(detail.contains("Discord token is not a valid HTTP authorization header"));
        assert!(detail.contains("source[1]="));
    }

    #[test]
    fn http_response_log_detail_omits_raw_body() {
        let api_error: ApiError =
            serde_json::from_str(r#"{"message":"Missing Access","code":50001}"#)
                .expect("api error json should parse");
        let detail = format_http_error_kind(&ErrorType::Response {
            body: b"raw response body with token-like-secret".to_vec(),
            error: api_error,
            status: StatusCode::FORBIDDEN,
        });

        assert!(detail.contains("HTTP 403"));
        assert!(detail.contains("Missing Access"));
        assert!(detail.contains("response_body_bytes=40"));
        assert!(!detail.contains("token-like-secret"));
    }

    #[test]
    fn parsing_log_detail_omits_raw_body() {
        let detail = format_http_error_kind(&ErrorType::Parsing {
            body: b"not-json-with-sensitive-text".to_vec(),
        });

        assert!(detail.contains("failed to parse Discord response body"));
        assert!(detail.contains("28 bytes"));
        assert!(!detail.contains("sensitive-text"));
    }

    #[test]
    fn unauthorized_log_detail_explains_token_expiry_without_token_value() {
        let detail = format_http_error_kind(&ErrorType::Unauthorized);

        assert_eq!(detail, "Discord token is unauthorized or expired");
    }
}
