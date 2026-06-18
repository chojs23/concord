use std::{fs, path::PathBuf};

use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

use crate::{AppError, Result, config::CredentialStoreMode, paths, support::private_file};

const KEYCHAIN_SERVICE: &str = "io.github.chojs23.concord.discord-token.v1";
const DEFAULT_ACCOUNT_ID: &str = "default";
const KEYCHAIN_ACCOUNT_PREFIX: &str = "account:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenSaveLocation {
    Keychain,
    PlaintextFile,
}

pub fn load_token(store: CredentialStoreMode) -> Result<Option<String>> {
    let account_id = selected_account_id();

    match store {
        CredentialStoreMode::Auto => match load_keychain_token(&account_id) {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) | Err(_) => load_fallback_token(&account_id),
        },
        CredentialStoreMode::Keychain => load_keychain_token(&account_id),
        CredentialStoreMode::Plain => load_fallback_token(&account_id),
    }
}

pub fn save_token(token: &str, store: CredentialStoreMode) -> Result<TokenSaveLocation> {
    let token = normalize_token(token)?;
    let account_id = selected_account_id();

    match store {
        CredentialStoreMode::Auto => match save_keychain_token(&account_id, &token) {
            Ok(()) => Ok(TokenSaveLocation::Keychain),
            Err(_) => {
                save_fallback_token(&account_id, &token)?;
                Ok(TokenSaveLocation::PlaintextFile)
            }
        },
        CredentialStoreMode::Keychain => {
            save_keychain_token(&account_id, &token)
                .map_err(|source| AppError::CredentialKeychain { source })?;
            Ok(TokenSaveLocation::Keychain)
        }
        CredentialStoreMode::Plain => {
            save_fallback_token(&account_id, &token)?;
            Ok(TokenSaveLocation::PlaintextFile)
        }
    }
}

pub fn delete_token(store: CredentialStoreMode) -> Result<()> {
    let account_id = selected_account_id();

    match store {
        CredentialStoreMode::Auto => {
            let keychain_result = delete_keychain_token(&account_id)
                .map_err(|source| AppError::CredentialKeychain { source });
            let fallback_result = delete_fallback_token(&account_id);

            fallback_result?;
            keychain_result
        }
        CredentialStoreMode::Keychain => {
            delete_keychain_token(&account_id)
                .map_err(|source| AppError::CredentialKeychain { source })?;
            Ok(())
        }
        CredentialStoreMode::Plain => delete_fallback_token(&account_id),
    }
}

fn credential_path() -> Result<PathBuf> {
    paths::credential_file().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not resolve user data directory",
        )
        .into()
    })
}

/// User-facing description of where the token will be saved.
pub fn credential_path_display() -> String {
    "your configured credential store".to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default)]
struct CredentialFile {
    selected_account: String,
    accounts: Vec<StoredAccount>,
}

impl Default for CredentialFile {
    fn default() -> Self {
        Self {
            selected_account: DEFAULT_ACCOUNT_ID.to_owned(),
            accounts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct StoredAccount {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    token: String,
}

impl CredentialFile {
    fn selected_account_id(&self) -> String {
        normalized_account_id(&self.selected_account).unwrap_or_else(default_account_id)
    }

    fn token_for_account(&self, account_id: &str) -> Option<String> {
        self.accounts
            .iter()
            .find(|account| normalized_account_id(&account.id).as_deref() == Some(account_id))
            .and_then(|account| normalize_token(&account.token).ok())
    }

    fn upsert_token(&mut self, account_id: &str, token: String) {
        self.selected_account = account_id.to_owned();
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|account| normalized_account_id(&account.id).as_deref() == Some(account_id))
        {
            account.id = account_id.to_owned();
            account.token = token;
            return;
        }

        self.accounts.push(StoredAccount {
            id: account_id.to_owned(),
            label: None,
            token,
        });
    }

    fn remove_token(&mut self, account_id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts
            .retain(|account| normalized_account_id(&account.id).as_deref() != Some(account_id));
        self.accounts.len() != before
    }
}

fn selected_account_id() -> String {
    match read_credential_file() {
        Ok(Some(credentials)) => credentials.selected_account_id(),
        Ok(None) | Err(_) => default_account_id(),
    }
}

fn load_keychain_token(account_id: &str) -> Result<Option<String>> {
    let entry =
        keychain_entry(account_id).map_err(|source| AppError::CredentialKeychain { source })?;
    match entry.get_password() {
        Ok(token) => Ok(normalize_token(&token).ok()),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(source) => Err(AppError::CredentialKeychain { source }),
    }
}

fn save_keychain_token(account_id: &str, token: &str) -> std::result::Result<(), KeyringError> {
    keychain_entry(account_id)?.set_password(token)
}

fn delete_keychain_token(account_id: &str) -> std::result::Result<(), KeyringError> {
    match keychain_entry(account_id)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(source) => Err(source),
    }
}

fn keychain_entry(account_id: &str) -> std::result::Result<Entry, KeyringError> {
    Entry::new(KEYCHAIN_SERVICE, &keychain_account(account_id))
}

fn keychain_account(account_id: &str) -> String {
    format!("{KEYCHAIN_ACCOUNT_PREFIX}{account_id}")
}

fn load_fallback_token(account_id: &str) -> Result<Option<String>> {
    Ok(read_credential_file()?.and_then(|credentials| credentials.token_for_account(account_id)))
}

fn save_fallback_token(account_id: &str, token: &str) -> Result<()> {
    let mut credentials = read_credential_file()?.unwrap_or_default();
    credentials.upsert_token(account_id, token.to_owned());
    write_credential_file(&credentials)
}

fn delete_fallback_token(account_id: &str) -> Result<()> {
    let Some(mut credentials) = read_credential_file()? else {
        return Ok(());
    };
    if !credentials.remove_token(account_id) {
        return Ok(());
    }
    if credentials.accounts.is_empty() {
        return remove_credential_file();
    }
    write_credential_file(&credentials)
}

fn remove_credential_file() -> Result<()> {
    let path = credential_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn read_credential_file() -> Result<Option<CredentialFile>> {
    let path = credential_path()?;
    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str::<CredentialFile>(&content)
            .map(Some)
            .map_err(|source| AppError::CredentialTomlDeserialize { source }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_credential_file(credentials: &CredentialFile) -> Result<()> {
    let path = credential_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        private_file::set_private_dir_permissions(parent)?;
    }

    let content = toml::to_string_pretty(credentials)
        .map_err(|source| AppError::CredentialTomlSerialize { source })?;
    private_file::write_private_file(&path, &content)
}

fn normalize_token(token: &str) -> std::result::Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::EmptyDiscordToken);
    }

    Ok(token.to_owned())
}

fn normalized_account_id(account_id: &str) -> Option<String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return None;
    }

    Some(account_id.to_owned())
}

fn default_account_id() -> String {
    DEFAULT_ACCOUNT_ID.to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        AppError,
        token_store::{CredentialFile, StoredAccount, normalize_token},
    };

    #[test]
    fn normalize_token_trims_and_rejects_empty_values() {
        assert_eq!(
            normalize_token("  token  ").expect("token should normalize"),
            "token"
        );

        let error = normalize_token("   ").expect_err("blank token must fail");
        assert!(matches!(error, AppError::EmptyDiscordToken));
    }

    #[test]
    fn credential_file_defaults_to_default_account() {
        let credentials = CredentialFile::default();

        assert_eq!(credentials.selected_account_id(), "default");
        assert_eq!(credentials.token_for_account("default"), None);
    }

    #[test]
    fn credential_file_reads_selected_account_token() {
        let credentials = CredentialFile {
            selected_account: "personal".to_owned(),
            accounts: vec![
                StoredAccount {
                    id: "default".to_owned(),
                    label: None,
                    token: "default-token".to_owned(),
                },
                StoredAccount {
                    id: "personal".to_owned(),
                    label: Some("Personal".to_owned()),
                    token: "  selected-token  ".to_owned(),
                },
            ],
        };

        assert_eq!(credentials.selected_account_id(), "personal");
        assert_eq!(
            credentials.token_for_account("personal").as_deref(),
            Some("selected-token")
        );
    }

    #[test]
    fn credential_file_upserts_account_token() {
        let mut credentials = CredentialFile::default();

        credentials.upsert_token("personal", "new-token".to_owned());
        credentials.upsert_token("personal", "updated-token".to_owned());

        assert_eq!(credentials.selected_account_id(), "personal");
        assert_eq!(credentials.accounts.len(), 1);
        assert_eq!(
            credentials.token_for_account("personal").as_deref(),
            Some("updated-token")
        );
    }

    #[test]
    fn credential_file_removes_selected_account_token() {
        let mut credentials = CredentialFile {
            selected_account: "personal".to_owned(),
            accounts: vec![
                StoredAccount {
                    id: "default".to_owned(),
                    label: None,
                    token: "default-token".to_owned(),
                },
                StoredAccount {
                    id: "personal".to_owned(),
                    label: Some("Personal".to_owned()),
                    token: "personal-token".to_owned(),
                },
            ],
        };

        assert!(credentials.remove_token("personal"));

        assert_eq!(credentials.selected_account_id(), "personal");
        assert_eq!(credentials.token_for_account("personal"), None);
        assert_eq!(
            credentials.token_for_account("default").as_deref(),
            Some("default-token")
        );
    }

    #[test]
    fn credential_file_keeps_accounts_when_removing_missing_token() {
        let mut credentials = CredentialFile {
            selected_account: "personal".to_owned(),
            accounts: vec![StoredAccount {
                id: "personal".to_owned(),
                label: Some("Personal".to_owned()),
                token: "personal-token".to_owned(),
            }],
        };

        assert!(!credentials.remove_token("work"));

        assert_eq!(credentials.selected_account_id(), "personal");
        assert_eq!(
            credentials.token_for_account("personal").as_deref(),
            Some("personal-token")
        );
    }
}
