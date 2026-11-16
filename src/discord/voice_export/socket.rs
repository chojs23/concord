//! Private, session-owned endpoints for read-only voice snapshots.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use interprocess::local_socket::{
    ListenerOptions,
    tokio::{Listener, Stream as LocalStream, prelude::*},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(super) struct BoundListener {
    listener: Listener,
    #[cfg(unix)]
    _cleanup: SocketCleanup,
    #[cfg(unix)]
    _lock: fs::File,
    #[cfg(test)]
    _test_directory: Option<tempfile::TempDir>,
}

impl BoundListener {
    pub(super) async fn accept(&self) -> io::Result<Stream> {
        self.listener.accept().await.map(|inner| Stream { inner })
    }
}

pub(super) struct Stream {
    inner: LocalStream,
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(windows)]
impl Drop for Stream {
    fn drop(&mut self) {
        // A slow overlay must not keep a cancelled session alive in the pipe
        // library's background flush pool. Snapshots can be discarded on close.
        let LocalStream::NamedPipe(pipe) = &self.inner;
        pipe.inner().assume_flushed();
    }
}

#[cfg(unix)]
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[cfg(unix)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
    owner: u32,
}

#[cfg(unix)]
impl SocketIdentity {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "voice export endpoint is not a socket",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
        })
    }
}

#[cfg(unix)]
struct SocketCleanup {
    path: PathBuf,
    identity: SocketIdentity,
}

#[cfg(unix)]
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        // Another session may have replaced this path after our listener closed.
        if SocketIdentity::read(&self.path).ok() == Some(self.identity) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
pub(super) async fn bind() -> io::Result<BoundListener> {
    let owner = current_user_id()?;
    let directory = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => {
            let directory = crate::paths::state_dir()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no user state directory"))?
                .join("voice");
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&directory)?;
            directory
        }
    };
    validate_directory(&directory, owner)?;
    bind_path_owned(directory.join("concord-voice.sock"), owner).await
}

#[cfg(unix)]
fn current_user_id() -> io::Result<u32> {
    // A new file is owned by the effective user even when HOME is overridden.
    // This avoids trusting an environment-derived home directory for ownership.
    Ok(tempfile::tempfile()?.metadata()?.uid())
}

#[cfg(unix)]
fn validate_directory(path: &Path, owner: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || !metadata.file_type().is_dir()
        || metadata.uid() != owner
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "voice export directory must be an absolute, user-owned private directory",
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
pub(super) async fn bind_path(path: PathBuf) -> io::Result<BoundListener> {
    let owner = current_user_id()?;
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket has no parent directory",
        )
    })?;
    validate_directory(directory, owner)?;
    bind_path_owned(path, owner).await
}

#[cfg(unix)]
async fn bind_path_owned(path: PathBuf, owner: u32) -> io::Result<BoundListener> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};
    use tokio::time::{Duration, timeout};

    // Keep the lock file in place after close. Unlinking it would let a third
    // process lock a new inode while an existing owner still held the old one.
    let lock = lock_endpoint(&path.with_extension("lock"), owner)?;
    match SocketIdentity::read(&path) {
        Ok(identity) => {
            if identity.owner != owner {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "voice export socket belongs to another user",
                ));
            }
            match timeout(
                Duration::from_millis(250),
                tokio::net::UnixStream::connect(&path),
            )
            .await
            {
                Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    if SocketIdentity::read(&path)? != identity {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "voice export socket changed during stale socket check",
                        ));
                    }
                    fs::remove_file(&path)?;
                }
                Ok(Err(error)) if error.kind() == io::ErrorKind::NotFound => {}
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "voice export socket is already in use",
                    ));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let name = path.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .reclaim_name(false)
        .create_tokio()?;
    let cleanup = SocketCleanup {
        identity: SocketIdentity::read(&path)?,
        path,
    };
    // The parent is private before binding. macOS cannot set socket mode during
    // bind, so chmod here never leaves a cross-user exposure window.
    fs::set_permissions(&cleanup.path, fs::Permissions::from_mode(0o600))?;
    Ok(BoundListener {
        listener,
        _cleanup: cleanup,
        _lock: lock,
        #[cfg(test)]
        _test_directory: None,
    })
}

#[cfg(unix)]
fn lock_endpoint(path: &Path, owner: u32) -> io::Result<fs::File> {
    let lock = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let before = fs::symlink_metadata(path)?;
            if !before.file_type().is_file() || before.uid() != owner || before.mode() & 0o077 != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "voice export lock must be a user-owned private file",
                ));
            }
            let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
            let after = file.metadata()?;
            if before.dev() != after.dev() || before.ino() != after.ino() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "voice export lock changed while opening",
                ));
            }
            file
        }
        Err(error) => return Err(error),
    };
    lock.try_lock().map_err(|error| match error {
        fs::TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::AddrInUse,
            "another Concord session owns voice export",
        ),
        fs::TryLockError::Error(error) => error,
    })?;
    Ok(lock)
}

#[cfg(windows)]
pub(super) async fn bind() -> io::Result<BoundListener> {
    bind_name("concord-voice")
}

#[cfg(windows)]
fn bind_name(name: &str) -> io::Result<BoundListener> {
    use interprocess::{
        local_socket::{GenericNamespaced, ToNsName},
        os::windows::local_socket::ListenerOptionsExt,
    };

    // Interprocess rejects remote clients and claims the first pipe instance.
    // Restrict its DACL too, rather than relying on the user's default DACL.
    let listener = ListenerOptions::new()
        .name(name.to_ns_name::<GenericNamespaced>()?)
        .security_descriptor(windows_security::current_user_descriptor()?)
        .create_tokio()?;
    Ok(BoundListener {
        listener,
        #[cfg(test)]
        _test_directory: None,
    })
}

#[cfg(test)]
pub(super) async fn bind_for_test()
-> io::Result<(BoundListener, interprocess::local_socket::Name<'static>)> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};

        let directory = tempfile::tempdir()?;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
        let path = directory.path().join("voice.sock");
        let name = path.clone().to_fs_name::<GenericFilePath>()?.into_owned();
        let mut listener = bind_path(path).await?;
        listener._test_directory = Some(directory);
        Ok((listener, name))
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};

        let name = format!("concord-voice-test-{}", uuid::Uuid::new_v4());
        let listener = bind_name(&name)?;
        Ok((
            listener,
            name.to_ns_name::<GenericNamespaced>()?.into_owned(),
        ))
    }
}

#[cfg(windows)]
mod windows_security {
    use std::{
        ffi::c_void,
        io,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr,
    };

    use interprocess::os::windows::security_descriptor::{
        AsSecurityDescriptorExt, BorrowedSecurityDescriptor, SecurityDescriptor,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            // Both conversion functions below return LocalAlloc-owned buffers.
            unsafe { LocalFree(self.0) };
        }
    }

    pub(super) fn current_user_descriptor() -> io::Result<SecurityDescriptor> {
        let mut token = ptr::null_mut();
        // The pseudo process handle is always valid and token is an out pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // OpenProcessToken transferred ownership of a real token handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut required = 0;
        // The first call asks Windows for the required TokenUser buffer length.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut user = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        // usize storage provides the pointer alignment required by TOKEN_USER.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                user.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // A successful TokenUser query initialized this structure and its SID.
        let user = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() };
        let mut sid = ptr::null_mut();
        // The SID lives in the still-owned token information buffer.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid_allocation = LocalAllocation(sid.cast());
        let mut length = 0;
        // ConvertSidToStringSidW returns a valid NUL-terminated UTF-16 buffer.
        while unsafe { *sid.add(length) } != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid, length) });
        drop(sid_allocation);
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        // The SDDL buffer is NUL-terminated and descriptor is an out pointer.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let allocation = LocalAllocation(descriptor);
        // The conversion returned a valid security descriptor kept alive by allocation.
        unsafe { BorrowedSecurityDescriptor::from_ptr(allocation.0) }.to_owned_sd()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn voice_export_socket_is_private_and_owned_until_shutdown() {
        let directory = tempfile::tempdir().expect("private temp directory");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("private directory permissions");
        let path = directory.path().join("voice.sock");
        let bound = bind_path(path.clone()).await.expect("bind private socket");
        assert_eq!(
            fs::metadata(&path).expect("socket metadata").mode() & 0o777,
            0o600
        );
        let competing = bind_path(path.clone()).await;
        assert_eq!(
            competing.err().expect("single endpoint owner").kind(),
            io::ErrorKind::AddrInUse
        );

        let (accepted, connected) =
            tokio::join!(bound.accept(), tokio::net::UnixStream::connect(&path));
        let mut accepted = accepted.expect("accepted client");
        let mut connected = connected.expect("connected client");
        accepted
            .write_all(b"snapshot\n")
            .await
            .expect("snapshot write");
        let mut bytes = [0; 9];
        connected
            .read_exact(&mut bytes)
            .await
            .expect("snapshot read");
        assert_eq!(&bytes, b"snapshot\n");
        drop(accepted);
        drop(connected);
        drop(bound);
        assert!(!path.exists());
        assert!(path.with_extension("lock").exists());

        let stale = std::os::unix::net::UnixListener::bind(&path).expect("stale socket fixture");
        drop(stale);
        let rebound = bind_path(path.clone())
            .await
            .expect("recover own stale socket");
        fs::remove_file(&path).expect("replace path fixture");
        fs::write(&path, "new owner's file").expect("replacement file");
        drop(rebound);
        assert_eq!(
            fs::read_to_string(&path).expect("replacement remains"),
            "new owner's file"
        );
    }
}
