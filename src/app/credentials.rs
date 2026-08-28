use crate::{
    DiscordClient, Result, config,
    discord::{DiscordAuthSession, validate_token_header},
    error::AppError,
    logging, token_store, tui,
};

pub(super) struct ResolvedToken {
    pub(super) token: String,
    pub(super) warnings: Vec<String>,
}

pub(super) async fn resolve_token(auth_session: DiscordAuthSession) -> Result<ResolvedToken> {
    let mut login_notice = None;
    let mut post_login_warnings = Vec::new();

    if let Some(token) = token_store::env_token() {
        validate_token_header(&token)?;
        if let Err(error) = validate_token_with_discord(&token, auth_session.clone()).await {
            match error {
                AppError::DiscordTokenRejected => return Err(AppError::DiscordTokenRejected),
                error => post_login_warnings.push(format!(
                    "CONCORD_TOKEN could not be verified: {error}; continuing with it for this session"
                )),
            }
        }
        return Ok(ResolvedToken {
            token,
            warnings: post_login_warnings,
        });
    }

    let credential_store = match config::load_options() {
        Ok(options) => options.credentials.store,
        Err(error) => {
            let warning = format!(
                "config could not be loaded for credential settings: {error}; using auto credential storage"
            );
            login_notice =
                Some("Credential storage is unavailable; token may not be saved.".to_owned());
            post_login_warnings.push(warning);
            config::CredentialStoreMode::default()
        }
    };

    match load_token_from_store(credential_store).await {
        Ok(Some(token)) => {
            if let Err(error) = validate_token_header(&token) {
                report_login_error(
                    &mut login_notice,
                    format!("saved Discord token is invalid: {error}; enter a new token"),
                    "Saved Discord token is invalid; enter a new token.",
                );
                delete_rejected_saved_token(credential_store, &mut post_login_warnings).await;
            } else if let Err(error) =
                validate_token_with_discord(&token, auth_session.clone()).await
            {
                match error {
                    AppError::DiscordTokenRejected => {
                        report_login_error(
                            &mut login_notice,
                            "saved Discord token was rejected by Discord; enter a new token",
                            "Saved Discord token was rejected; enter a new token.",
                        );
                        delete_rejected_saved_token(credential_store, &mut post_login_warnings)
                            .await;
                    }
                    error => report_login_error(
                        &mut login_notice,
                        format!(
                            "saved Discord token could not be verified: {error}; enter a token to continue"
                        ),
                        "Could not verify the saved token with Discord; log in again.",
                    ),
                }
            } else {
                return Ok(ResolvedToken {
                    token,
                    warnings: post_login_warnings,
                });
            }
        }
        Ok(None) => {}
        Err(error) => {
            let warning = format!(
                "credential store unavailable: {error}; enter a token to continue for this session"
            );
            login_notice =
                Some("Credential storage is unavailable; token may not be saved.".to_owned());
            post_login_warnings.push(warning);
        }
    }

    let token = loop {
        // The login UI owns this notice for one attempt. A successful attempt
        // drops it instead of carrying the resolved error into the dashboard.
        let token =
            tui::prompt_login_with_auth_session(login_notice.take(), auth_session.clone()).await?;
        if let Err(error) = validate_token_header(&token) {
            report_login_error(
                &mut login_notice,
                format!("entered Discord token is invalid: {error}; enter a new token"),
                "That Discord token is invalid; enter a different token.",
            );
            continue;
        }
        match validate_token_with_discord(&token, auth_session.clone()).await {
            Ok(()) => break token,
            Err(AppError::DiscordTokenRejected) => {
                report_login_error(
                    &mut login_notice,
                    "entered Discord token was rejected by Discord; enter a different token",
                    "Discord rejected that token; enter a different token.",
                );
            }
            Err(error) => {
                report_login_error(
                    &mut login_notice,
                    format!("Discord token could not be verified: {error}; try again"),
                    "Could not verify the token with Discord; try again.",
                );
            }
        }
    };
    match save_token_to_store(token.clone(), credential_store).await {
        Ok(token_store::TokenSaveLocation::PlaintextFile)
            if credential_store == config::CredentialStoreMode::Auto =>
        {
            post_login_warnings.push(
                "system keychain is unavailable; token was saved to the plaintext fallback credential store"
                    .to_owned(),
            );
        }
        Ok(_) => {}
        Err(error) => post_login_warnings.push(format!("token was not saved: {error}")),
    }

    Ok(ResolvedToken {
        token,
        warnings: post_login_warnings,
    })
}

fn report_login_error(
    login_notice: &mut Option<String>,
    detail: impl AsRef<str>,
    notice: impl Into<String>,
) {
    logging::error("app", detail.as_ref());
    *login_notice = Some(notice.into());
}

async fn validate_token_with_discord(token: &str, auth_session: DiscordAuthSession) -> Result<()> {
    DiscordClient::new_with_auth_session(token.to_owned(), auth_session)?
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
