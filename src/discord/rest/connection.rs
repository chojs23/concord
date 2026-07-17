use reqwest::StatusCode;

use crate::{AppError, Result};

use super::DiscordRest;

impl DiscordRest {
    pub async fn validate_token_authentication(&self) -> Result<()> {
        let label = "token validation";
        let response = self
            .execute_authenticated(
                self.raw_http.get("https://discord.com/api/v9/users/@me"),
                label,
            )
            .await?;
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
