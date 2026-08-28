use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancellation state for an in-flight stream capture preparation.
///
/// This lives outside `capture` because it is runtime plumbing rather than
/// capture machinery: `run_broadcast_capture_preparation` uses it to abandon a
/// preparation that is still waiting on the previous broadcast's cleanup, and
/// shutdown uses it to join that task promptly. Builds without the
/// `stream-broadcast` feature swap `capture` for a stub, so keeping one shared
/// implementation here is what stops the two from drifting apart. External
/// cancellation is kept separate from the worker stop flag because a failed
/// preparation drops its worker handle as part of cleanup. That cleanup must
/// not hide the original failure from the runtime and UI.
#[derive(Clone, Default)]
pub(super) struct StreamCaptureCancellation {
    cancelled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl StreamCaptureCancellation {
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.stop.store(true, Ordering::Release);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    #[cfg_attr(not(feature = "stream-broadcast"), allow(dead_code))]
    pub(super) fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_stop_does_not_mark_preparation_as_externally_cancelled() {
        let cancellation = StreamCaptureCancellation::default();

        cancellation.flag().store(true, Ordering::Release);

        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn external_cancellation_also_stops_the_capture_worker() {
        let cancellation = StreamCaptureCancellation::default();
        let stop = cancellation.flag();

        cancellation.cancel();

        assert!(cancellation.is_cancelled());
        assert!(stop.load(Ordering::Acquire));
    }
}
