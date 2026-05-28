use std::sync::mpsc::sync_channel;
use std::thread::{self, JoinHandle as ThreadJoinHandle};

use tokio::runtime::{Builder, Handle};
use tokio::sync::oneshot;

use crate::logging;

/// Dedicated tokio runtime for the voice data plane.
///
/// The audio path (mic capture pump, opus encode/decode, UDP send/receive) is
/// real-time work that must not be starved by anything else the app does. We
/// run it on a current-thread tokio runtime pinned to its own OS thread so
/// TUI redraws, gateway events, and image decoding on the main runtime can't
/// delay packet processing.
pub(super) struct VoiceAudioRuntime {
    handle: Handle,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: Option<ThreadJoinHandle<()>>,
}

impl VoiceAudioRuntime {
    pub(super) fn start() -> Result<Self, String> {
        let (handle_tx, handle_rx) = sync_channel::<Handle>(1);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let worker = thread::Builder::new()
            .name("voice-audio".to_owned())
            .spawn(move || {
                let runtime = match Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        logging::error(
                            "voice",
                            format!("voice audio runtime build failed: {error}"),
                        );
                        return;
                    }
                };
                let _ = handle_tx.send(runtime.handle().clone());
                runtime.block_on(async move {
                    let _ = shutdown_rx.await;
                });
                logging::debug("voice", "voice audio runtime shut down");
            })
            .map_err(|error| format!("voice audio thread spawn failed: {error}"))?;
        let handle = handle_rx
            .recv()
            .map_err(|_| "voice audio runtime handle receive failed".to_owned())?;
        logging::debug("voice", "voice audio runtime started");
        Ok(Self {
            handle,
            shutdown_tx: Some(shutdown_tx),
            worker: Some(worker),
        })
    }

    pub(super) fn handle(&self) -> &Handle {
        &self.handle
    }
}

impl Drop for VoiceAudioRuntime {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(worker) = self.worker.take()
            && let Err(error) = worker.join()
        {
            logging::debug(
                "voice",
                format!("voice audio runtime thread panicked: {error:?}"),
            );
        }
    }
}
