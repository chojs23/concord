use std::error::Error as StdError;

use thiserror::Error;

use crate::discord::{ActionBlockReason, DiscordAction};

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
    #[error("Discord rejected the token")]
    DiscordTokenRejected,
    #[error("message content must not be empty")]
    EmptyMessageContent,
    #[error("message content exceeds Discord's 2000 character limit: {len}")]
    MessageTooLong { len: usize },
    #[error("attachment exceeds upload limit: {filename} ({size} bytes, limit {limit} bytes)")]
    AttachmentTooLarge {
        filename: String,
        size: u64,
        limit: u64,
    },
    #[error("message has too many attachments: {count}")]
    TooManyAttachments { count: usize },
    #[error("Discord request failed: {0}")]
    DiscordRequest(String),
    #[error("Discord action blocked: cannot {action}: {reason}")]
    DiscordActionBlocked {
        action: DiscordAction,
        reason: ActionBlockReason,
    },
    #[error("Discord stopped authenticated requests after rejecting the session")]
    DiscordAuthenticationStopped,
    #[error("Discord rate limited {action}; try again in {retry_after_millis} ms")]
    DiscordRateLimited {
        action: String,
        retry_after_millis: u64,
    },
    #[error("Discord forum search index is warming; try again in {retry_after_millis} ms")]
    ForumSearchIndexWarming { retry_after_millis: u64 },
    #[error("message slowmode is active; try again in {retry_after_millis} ms")]
    MessageSlowModeActive { retry_after_millis: u64 },
    #[error(
        "Discord request circuit is open for {method} {path}; try again in {retry_after_millis} ms"
    )]
    DiscordRequestCircuitOpen {
        method: String,
        path: String,
        retry_after_millis: u64,
    },
    #[error("Discord requires a CAPTCHA to {action}")]
    CaptchaRequired { action: String },
    #[error("terminal I/O failed")]
    Io(#[from] std::io::Error),
    #[error("config file is not valid TOML")]
    ConfigTomlDeserialize(#[from] toml::de::Error),
    #[error("config file could not be written as TOML")]
    ConfigTomlSerialize(#[from] toml::ser::Error),
    #[error("credential store file is not valid TOML")]
    CredentialTomlDeserialize {
        #[source]
        source: toml::de::Error,
    },
    #[error("credential store file could not be written as TOML")]
    CredentialTomlSerialize {
        #[source]
        source: toml::ser::Error,
    },
    #[error("system keychain credential store failed")]
    CredentialKeychain {
        #[source]
        source: keyring::Error,
    },
    #[error("credential store task failed")]
    CredentialStoreTask {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("keymap config is invalid: {0}")]
    InvalidKeymapConfig(String),
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
