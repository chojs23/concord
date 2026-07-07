use crate::{
    DiscordClient, Result, config, discord::validate_token_header, error::AppError, token_store,
    tui,
};

pub(super) struct ResolvedToken {
    pub(super) token: String,
    pub(super) warnings: Vec<String>,
}

pub(super) async fn resolve_token() -> Result<ResolvedToken> {
    let mut warnings = Vec::new();

    if let Some(token) = token_store::env_token() {
        validate_token_header(&token)?;
        if let Err(error) = validate_token_with_discord(&token).await {
            match error {
                AppError::DiscordTokenRejected => return Err(AppError::DiscordTokenRejected),
                error => warnings.push(format!(
                    "CONCORD_TOKEN could not be verified: {error}; continuing with it for this session"
                )),
            }
        }
        return Ok(ResolvedToken { token, warnings });
    }

    let credential_store = match config::load_options() {
        Ok(options) => options.credentials.store,
        Err(error) => {
            warnings.push(format!(
                "config could not be loaded for credential settings: {error}; using auto credential storage"
            ));
            config::CredentialStoreMode::default()
        }
    };

    match load_token_from_store(credential_store).await {
        Ok(Some(token)) => {
            if let Err(error) = validate_token_header(&token) {
                warnings.push(format!(
                    "saved Discord token is invalid: {error}; enter a new token"
                ));
                delete_rejected_saved_token(credential_store, &mut warnings).await;
            } else if let Err(error) = validate_token_with_discord(&token).await {
                match error {
                    AppError::DiscordTokenRejected => {
                        warnings.push(
                            "saved Discord token was rejected by Discord; enter a new token"
                                .to_owned(),
                        );
                        delete_rejected_saved_token(credential_store, &mut warnings).await;
                    }
                    error => warnings.push(format!(
                        "saved Discord token could not be verified: {error}; enter a token to continue"
                    )),
                }
            } else {
                return Ok(ResolvedToken { token, warnings });
            }
        }
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "credential store unavailable: {error}; enter a token to continue for this session"
        )),
    }

    let token = loop {
        let login_notice = login_notice_for_token_warnings(&warnings);
        let token = tui::prompt_login(login_notice).await?;
        if let Err(error) = validate_token_header(&token) {
            warnings = vec![format!(
                "entered Discord token is invalid: {error}; enter a new token"
            )];
            continue;
        }
        match validate_token_with_discord(&token).await {
            Ok(()) => break token,
            Err(AppError::DiscordTokenRejected) => {
                warnings = vec![
                    "entered Discord token was rejected by Discord; enter a different token"
                        .to_owned(),
                ];
            }
            Err(error) => {
                warnings = vec![format!(
                    "Discord token could not be verified: {error}; try again"
                )];
            }
        }
    };
    match save_token_to_store(token.clone(), credential_store).await {
        Ok(token_store::TokenSaveLocation::PlaintextFile)
            if credential_store == config::CredentialStoreMode::Auto =>
        {
            warnings.push(
                "system keychain is unavailable; token was saved to the plaintext fallback credential store"
                    .to_owned(),
            );
        }
        Ok(_) => {}
        Err(error) => warnings.push(format!("token was not saved: {error}")),
    }

    Ok(ResolvedToken { token, warnings })
}

async fn validate_token_with_discord(token: &str) -> Result<()> {
    DiscordClient::new(token.to_owned())?
        .validate_token_authentication()
        .await
}

async fn load_token_from_store(store: config::CredentialStoreMode) -> Result<Option<String>> {
    tokio::task::spawn_blocking(move || token_store::load_token(store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
}

async fn save_token_to_store(
    token: String,
    store: config::CredentialStoreMode,
) -> Result<token_store::TokenSaveLocation> {
    tokio::task::spawn_blocking(move || token_store::save_token(&token, store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
}

async fn delete_token_from_store(store: config::CredentialStoreMode) -> Result<()> {
    tokio::task::spawn_blocking(move || token_store::delete_token(store))
        .await
        .map_err(|source| AppError::CredentialStoreTask { source })?
}

async fn delete_rejected_saved_token(
    store: config::CredentialStoreMode,
    warnings: &mut Vec<String>,
) {
    if let Err(error) = delete_token_from_store(store).await {
        warnings.push(format!(
            "rejected saved token could not be deleted: {error}"
        ));
    }
}

fn login_notice_for_token_warnings(warnings: &[String]) -> Option<String> {
    if warnings
        .iter()
        .any(|warning| warning.starts_with("entered Discord token"))
    {
        Some("Discord rejected that token; enter a different token.".to_owned())
    } else if warnings
        .iter()
        .any(|warning| warning.starts_with("saved Discord token was rejected"))
    {
        Some("Saved Discord token was rejected; enter a new token.".to_owned())
    } else if warnings
        .iter()
        .any(|warning| warning.starts_with("Discord token could not be verified"))
    {
        Some("Could not verify the token with Discord; try again.".to_owned())
    } else if warnings
        .iter()
        .any(|warning| warning.starts_with("saved Discord token"))
    {
        Some("Saved Discord token is invalid; enter a new token.".to_owned())
    } else if warnings.is_empty() {
        None
    } else {
        Some("Credential storage is unavailable; token may not be saved.".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::login_notice_for_token_warnings;

    #[test]
    fn login_notice_for_token_warnings_reports_user_action() {
        let cases = [
            (
                "saved Discord token is invalid: bad; enter a new token",
                "Saved Discord token is invalid; enter a new token.",
            ),
            (
                "saved Discord token was rejected by Discord; enter a new token",
                "Saved Discord token was rejected; enter a new token.",
            ),
            (
                "entered Discord token was rejected by Discord; enter a different token",
                "Discord rejected that token; enter a different token.",
            ),
            (
                "Discord token could not be verified: network down; try again",
                "Could not verify the token with Discord; try again.",
            ),
            (
                "credential store unavailable: permission denied",
                "Credential storage is unavailable; token may not be saved.",
            ),
        ];

        for (warning, expected) in cases {
            let warnings = vec![warning.to_owned()];
            assert_eq!(
                login_notice_for_token_warnings(&warnings).as_deref(),
                Some(expected)
            );
        }
    }
}
