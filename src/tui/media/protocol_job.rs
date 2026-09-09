use std::sync::Arc;

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::Protocol};
use tokio::{sync::mpsc, task};

use super::{
    MediaProtocolRenderSpec,
    avatar::AvatarFrameProtocolKey,
    clipped_media_protocol, emoji_protocol,
    preview::ImagePreviewKey,
    work::{
        MediaProtocolRequest, MediaWorkError, MediaWorkResult, media_image_job_permits,
        media_image_work_permits,
    },
};
use crate::tui::text::EmojiImageSize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MediaProtocolBuildTarget {
    Preview {
        key: ImagePreviewKey,
        render_spec: MediaProtocolRenderSpec,
        frame_index: usize,
    },
    Avatar {
        url: String,
        key: AvatarFrameProtocolKey,
    },
    Emoji {
        url: String,
        frame_index: usize,
        image_size: EmojiImageSize,
    },
}

pub(in crate::tui) struct MediaProtocolBuildJob {
    target: MediaProtocolBuildTarget,
    generation: u64,
    picker: Picker,
    image: Arc<DynamicImage>,
    request: Option<MediaProtocolRequest>,
}

pub(in crate::tui) struct MediaProtocolBuildResult {
    pub(in crate::tui) target: MediaProtocolBuildTarget,
    pub(in crate::tui) generation: u64,
    pub(in crate::tui) result: MediaWorkResult<Protocol>,
    pub(in crate::tui) request: Option<MediaProtocolRequest>,
}

impl MediaProtocolBuildJob {
    fn new(
        target: MediaProtocolBuildTarget,
        generation: u64,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self {
            target,
            generation,
            picker,
            image,
            request: None,
        }
    }

    pub(super) fn preview(
        key: ImagePreviewKey,
        generation: u64,
        render_spec: MediaProtocolRenderSpec,
        frame_index: usize,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self::new(
            MediaProtocolBuildTarget::Preview {
                key,
                render_spec,
                frame_index,
            },
            generation,
            picker,
            image,
        )
    }

    pub(super) fn avatar(
        url: String,
        generation: u64,
        key: AvatarFrameProtocolKey,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self::new(
            MediaProtocolBuildTarget::Avatar { url, key },
            generation,
            picker,
            image,
        )
    }

    pub(super) fn emoji(
        url: String,
        generation: u64,
        frame_index: usize,
        image_size: EmojiImageSize,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self::new(
            MediaProtocolBuildTarget::Emoji {
                url,
                frame_index,
                image_size,
            },
            generation,
            picker,
            image,
        )
    }

    pub(super) fn with_request(mut self, request: MediaProtocolRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(MediaProtocolRequest::is_cancelled)
    }

    fn complete(self, result: MediaWorkResult<Protocol>) -> MediaProtocolBuildResult {
        if let Some(request) = &self.request {
            request.finish();
        }
        MediaProtocolBuildResult {
            target: self.target,
            generation: self.generation,
            request: self.request,
            result,
        }
    }
}

pub(in crate::tui) fn spawn_media_protocol_build(
    job: MediaProtocolBuildJob,
    tx: mpsc::UnboundedSender<MediaProtocolBuildResult>,
) {
    let work_permits = media_image_work_permits().clone();
    let Ok(job_permit) = media_image_job_permits().clone().try_acquire_owned() else {
        let _ = tx.send(job.complete(Err(MediaWorkError::Busy)));
        return;
    };
    let target = job.target.clone();
    let generation = job.generation;
    let request = job.request.clone();
    task::spawn(async move {
        let _job_permit = job_permit;
        let _permit = if let Some(request) = &request {
            tokio::select! {
                biased;
                _ = request.cancelled() => {
                    let _ = tx.send(job.complete(Err(MediaWorkError::Busy)));
                    return;
                }
                permit = work_permits.acquire_owned() => {
                    permit.expect("media work semaphore stays open")
                }
            }
        } else {
            work_permits
                .acquire_owned()
                .await
                .expect("media work semaphore stays open")
        };
        let result = match task::spawn_blocking(move || build_media_protocol(job)).await {
            Ok(result) => result,
            Err(error) => {
                if let Some(request) = &request {
                    request.finish();
                }
                MediaProtocolBuildResult {
                    target,
                    generation,
                    request,
                    result: Err(MediaWorkError::Failed(format!(
                        "image protocol worker failed: {error}"
                    ))),
                }
            }
        };
        let _ = tx.send(result);
    });
}

pub(in crate::tui) fn build_media_protocol(job: MediaProtocolBuildJob) -> MediaProtocolBuildResult {
    if job
        .request
        .as_ref()
        .is_some_and(|request| !request.try_start())
    {
        return job.complete(Err(MediaWorkError::Busy));
    }
    let result = match &job.target {
        MediaProtocolBuildTarget::Preview { render_spec, .. } => {
            clipped_media_protocol(&job.picker, &job.image, *render_spec).ok_or_else(|| {
                MediaWorkError::Failed("image protocol dimensions unavailable".to_owned())
            })
        }
        MediaProtocolBuildTarget::Avatar { key, .. } => {
            let render_spec = key.render_spec();
            clipped_media_protocol(&job.picker, &job.image, render_spec).ok_or_else(|| {
                MediaWorkError::Failed("image protocol dimensions unavailable".to_owned())
            })
        }
        MediaProtocolBuildTarget::Emoji { image_size, .. } => {
            emoji_protocol(&job.picker, &job.image, *image_size).ok_or_else(|| {
                MediaWorkError::Failed("emoji protocol dimensions unavailable".to_owned())
            })
        }
    };
    job.complete(result)
}
