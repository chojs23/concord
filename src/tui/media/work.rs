use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use tokio::sync::{Notify, Semaphore};

const MAX_CONCURRENT_MEDIA_IMAGE_WORKERS: usize = 2;
const MAX_MEDIA_IMAGE_WORK_JOBS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MediaWorkError {
    Busy,
    Failed(String),
}

pub(in crate::tui) type MediaWorkResult<T> = std::result::Result<T, MediaWorkError>;

const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const FINISHED: u8 = 2;

/// One encoding attempt, distinct even when a preview returns to the same crop.
/// Queued work is cancelable. A running encoder must finish before its slot can
/// be reused, since blocking image code cannot be safely interrupted.
#[derive(Clone)]
pub(in crate::tui) struct MediaProtocolRequest(Arc<ProtocolRequestState>);

struct ProtocolRequestState {
    phase: AtomicU8,
    cancelled: AtomicBool,
    wake: Notify,
}

impl MediaProtocolRequest {
    pub(super) fn new() -> Self {
        Self(Arc::new(ProtocolRequestState {
            phase: AtomicU8::new(QUEUED),
            cancelled: AtomicBool::new(false),
            wake: Notify::new(),
        }))
    }

    pub(super) fn same_request(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(super) fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        let _ = self
            .0
            .phase
            .compare_exchange(QUEUED, FINISHED, Ordering::SeqCst, Ordering::SeqCst);
        // There is only one waiter per request. notify_one keeps a permit if
        // cancellation races with that worker entering the semaphore wait.
        self.0.wake.notify_one();
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    pub(super) async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.0.wake.notified().await;
        }
    }

    pub(super) fn try_start(&self) -> bool {
        self.0
            .phase
            .compare_exchange(QUEUED, RUNNING, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub(super) fn finish(&self) {
        self.0.phase.store(FINISHED, Ordering::SeqCst);
    }

    pub(super) fn is_finished(&self) -> bool {
        self.0.phase.load(Ordering::SeqCst) == FINISHED
    }
}

pub(super) fn media_image_work_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_MEDIA_IMAGE_WORKERS)))
}

pub(in crate::tui) fn media_image_job_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_MEDIA_IMAGE_WORK_JOBS)))
}
