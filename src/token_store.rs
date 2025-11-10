use crate::{AppError, Result};

const SERVICE: &str = "discord-client-terminal";
const ACCOUNT: &str = "discord-bot-token";

pub fn load_token() -> Result<Option<String>> {
    let entry = credential_entry()?;

    match entry.get_password() {
        Ok(token) => Ok(normalize_token(&token).ok()),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn save_token(token: &str) -> Result<()> {
    let token = normalize_token(token)?;
    credential_entry()?.set_password(&token)?;
    Ok(())
}

fn normalize_token(token: &str) -> std::result::Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::EmptyDiscordToken);
    }

    Ok(token.to_owned())
}

#[cfg(target_os = "macos")]
fn credential_entry() -> Result<keyring_core::Entry> {
    use std::sync::Arc;

    let store: Arc<keyring_core::CredentialStore> =
        apple_native_keyring_store::keychain::Store::new()?;
    keyring_core::set_default_store(store);
    keyring_core::Entry::new(SERVICE, ACCOUNT).map_err(Into::into)
}

#[cfg(not(target_os = "macos"))]
fn credential_entry() -> Result<keyring_core::Entry> {
    Err(AppError::UnsupportedCredentialStore)
}

#[cfg(test)]
mod tests {
    use crate::{AppError, token_store::normalize_token};

    #[test]
    fn trims_token_before_saving() {
        let token = normalize_token("  token  ").expect("token should normalize");
        assert_eq!(token, "token");
    }

    #[test]
    fn rejects_empty_token() {
        let error = normalize_token("   ").expect_err("blank token must fail");
        assert!(matches!(error, AppError::EmptyDiscordToken));
    }
}
