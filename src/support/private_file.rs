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

#[cfg(unix)]
pub(crate) fn write_private_file(path: &Path, content: &str) -> Result<()> {
    use std::{
        fs,
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())?;

    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn write_private_file(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
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

        let mode = path
            .metadata()
            .expect("file metadata should be available")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
