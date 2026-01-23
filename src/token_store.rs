use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{AppError, Result};

const CONFIG_DIR: &str = ".concord";
const CREDENTIAL_FILE: &str = "credential";

pub fn load_token() -> Result<Option<String>> {
    let path = credential_path()?;

    match fs::read_to_string(&path) {
        Ok(token) => Ok(normalize_token(&token).ok()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn save_token(token: &str) -> Result<()> {
    let token = normalize_token(token)?;
    let path = credential_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_dir_permissions(parent)?;
    }

    write_private_file(&path, &token)?;
    Ok(())
}

fn credential_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "HOME environment variable is not set",
        )
    })?;

    Ok(PathBuf::from(home).join(CONFIG_DIR).join(CREDENTIAL_FILE))
}

fn normalize_token(token: &str) -> std::result::Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::EmptyDiscordToken);
    }

    Ok(token.to_owned())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_private_file(path: &Path, token: &str) -> Result<()> {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;

    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, token: &str) -> Result<()> {
    fs::write(path, token)?;
    Ok(())
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
