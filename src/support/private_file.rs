use std::path::Path;

use crate::Result;

#[cfg(unix)]
pub(crate) fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Writes to a sibling temp file and renames it into place, so a crash or
/// full disk mid-write can never leave a truncated file behind.
#[cfg(unix)]
pub(crate) fn write_private_file(path: &Path, content: &str) -> Result<()> {
    use std::{fs, io::Write, os::unix::fs::PermissionsExt};

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)?;
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn write_private_file(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::{set_private_dir_permissions, write_private_file};

    #[test]
    fn sets_private_directory_permissions() {
        let dir = tempfile::tempdir().expect("tempdir should be created");

        set_private_dir_permissions(dir.path()).expect("directory permissions should be set");

        let mode = dir
            .path()
            .metadata()
            .expect("directory metadata should be available")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn writes_private_file_permissions() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let path = dir.path().join("private.toml");

        write_private_file(&path, "token = 'secret'").expect("private file should be written");
        write_private_file(&path, "token = 'rotated'")
            .expect("existing private file should be replaced");

        let content = std::fs::read_to_string(&path).expect("private file should be readable");
        assert_eq!(content, "token = 'rotated'");
        let mode = path
            .metadata()
            .expect("file metadata should be available")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
