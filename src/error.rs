use std::num::ParseIntError;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("login cancelled before a Discord token was saved")]
    LoginCancelled,
    #[error("Discord token must not be empty")]
    EmptyDiscordToken,
    #[error("invalid DISCORD_DEFAULT_CHANNEL_ID value `{value}`")]
    InvalidChannelId {
        value: String,
        #[source]
        source: ParseIntError,
    },
    #[error("invalid DISCORD_ENABLE_MESSAGE_CONTENT value `{value}`")]
    InvalidMessageContentFlag { value: String },
    #[error("message content must not be empty")]
    EmptyMessageContent,
    #[error("message content exceeds Discord's 2000 character limit: {len}")]
    MessageTooLong { len: usize },
    #[error("Discord HTTP request failed")]
    Http(#[from] twilight_http::Error),
    #[error("failed to decode Discord response body")]
    DeserializeBody(#[from] twilight_http::response::DeserializeBodyError),
    #[error("terminal I/O failed")]
    Io(#[from] std::io::Error),
    #[error("QR login failed: {0}")]
    QrLogin(String),
    #[error("QR login was cancelled in the Discord mobile app")]
    QrLoginCancelled,
}
