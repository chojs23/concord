//! RPC apps probe `discord-ipc-0`..`-9`, so we claim the lowest free slot: a
//! filesystem socket in the runtime dir on Unix, a named pipe on Windows.

use std::io;
use std::path::PathBuf;

use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::tokio::Listener;

const SLOT_COUNT: u8 = 10;

pub(super) struct BoundListener {
    pub listener: Listener,
    pub slot: u8,
    /// Socket file to unlink on shutdown. `None` on Windows, where the OS
    /// reclaims named pipes when the handle closes.
    pub path: Option<PathBuf>,
}

pub(super) struct SocketCleanup {
    path: Option<PathBuf>,
}

impl SocketCleanup {
    pub(super) fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub(super) fn bind_first_available() -> io::Result<BoundListener> {
    let mut last_error = None;
    for slot in 0..SLOT_COUNT {
        match bind_slot(slot) {
            Ok(bound) => return Ok(bound),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::AddrInUse, "no free discord-ipc slot")))
}

#[cfg(unix)]
fn bind_slot(slot: u8) -> io::Result<BoundListener> {
    use interprocess::local_socket::{GenericFilePath, ToFsName};

    let path = socket_path(slot);
    // A leftover socket file blocks binding. Probe it: if a live server answers,
    // skip to the next slot. If not, it is stale, so remove it and rebind.
    if path.exists() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{} already in use", path.display()),
                ));
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let name = path.clone().to_fs_name::<GenericFilePath>()?;
    let listener = ListenerOptions::new().name(name).create_tokio()?;
    Ok(BoundListener {
        listener,
        slot,
        path: Some(path),
    })
}

#[cfg(unix)]
fn socket_path(slot: u8) -> PathBuf {
    // RPC apps look in $XDG_RUNTIME_DIR first, then temp dirs, so bind in the same base.
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("TMPDIR"))
        .or_else(|| std::env::var_os("TMP"))
        .or_else(|| std::env::var_os("TEMP"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(format!("discord-ipc-{slot}"))
}

#[cfg(windows)]
fn bind_slot(slot: u8) -> io::Result<BoundListener> {
    use interprocess::local_socket::{GenericNamespaced, ToNsName};

    // The namespaced name maps to `\\.\pipe\discord-ipc-N`, where RPC clients look.
    let name = format!("discord-ipc-{slot}").to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_tokio()?;
    Ok(BoundListener {
        listener,
        slot,
        path: None,
    })
}
