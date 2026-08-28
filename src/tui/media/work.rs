use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;

const MAX_CONCURRENT_MEDIA_IMAGE_WORKERS: usize = 2;
const MAX_MEDIA_IMAGE_WORK_JOBS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MediaWorkError {
    Busy,
    Failed(String),
}

pub(in crate::tui) type MediaWorkResult<T> = std::result::Result<T, MediaWorkError>;

pub(super) fn media_image_work_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_MEDIA_IMAGE_WORKERS)))
}

pub(super) fn media_image_job_permits() -> &'static Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_MEDIA_IMAGE_WORK_JOBS)))
}
