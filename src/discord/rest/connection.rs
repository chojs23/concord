use reqwest::StatusCode;

use crate::{AppError, Result};

use super::DiscordRest;

impl DiscordRest {
    /// Fire a cheap REST call to establish the HTTPS connection up front.
    /// `reqwest::Client` lazily opens a TCP+TLS+HTTP/2 connection on the first
    /// request, which costs ~500ms-1s of round-trips. The first user-facing
    /// fetch (e.g. opening a forum) would otherwise pay that cost on top of
    /// the search index cold-start, doubled because we issue two parallel
    /// search calls. Priming the pool at startup lets the first real request
    /// reuse the warmed connection and start in single-digit milliseconds.
    pub async fn prime_connection_pool(&self) -> Result<()> {
        self.validate_token_authentication_with_label("connection prime")
            .await
    }

    pub async fn validate_token_authentication(&self) -> Result<()> {
        self.validate_token_authentication_with_label("token validation")
            .await
    }

    async fn validate_token_authentication_with_label(&self, label: &str) -> Result<()> {
        let response = self
            .authenticated(self.raw_http.get("https://discord.com/api/v9/users/@me"))
            .send()
            .await
            .map_err(|error| {
                AppError::DiscordRequest(format!("{label} request failed: {error}"))
            })?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(AppError::DiscordTokenRejected);
        }
        response
            .error_for_status()
            .map_err(|error| AppError::DiscordRequest(format!("{label} failed: {error}")))?;
        Ok(())
    }
}
