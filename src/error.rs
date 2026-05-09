use std::error::Error as StdError;

use thiserror::Error;

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
    #[error("attachment exceeds Discord's 10 MiB default upload limit: {filename} ({size} bytes)")]
    AttachmentTooLarge { filename: String, size: u64 },
    #[error("attachments exceed Discord's 25 MiB request limit: {size} bytes")]
    AttachmentsTooLarge { size: u64 },
    #[error("message has too many attachments: {count}")]
    TooManyAttachments { count: usize },
    #[error("Discord request failed: {0}")]
    DiscordRequest(String),
    #[error("terminal I/O failed")]
    Io(#[from] std::io::Error),
    #[error("config file is not valid TOML")]
    ConfigTomlDeserialize(#[from] toml::de::Error),
    #[error("config file could not be written as TOML")]
    ConfigTomlSerialize(#[from] toml::ser::Error),
    #[error("QR login failed: {0}")]
    QrLogin(String),
    #[error("QR login was cancelled in the Discord mobile app")]
    QrLoginCancelled,
}

impl AppError {
    pub fn log_detail(&self) -> String {
        format_error_chain(self)
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
    use super::AppError;

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
}
