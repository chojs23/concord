use std::sync::Arc;

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::Protocol};
use tokio::{sync::mpsc, task};

use super::{
    MediaProtocolRenderSpec,
    avatar::AvatarFrameProtocolKey,
    clipped_media_protocol, emoji_protocol,
    preview::ImagePreviewKey,
    work::{MediaWorkError, MediaWorkResult, media_image_job_permits, media_image_work_permits},
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
}

pub(in crate::tui) struct MediaProtocolBuildResult {
    pub(in crate::tui) target: MediaProtocolBuildTarget,
    pub(in crate::tui) generation: u64,
    pub(in crate::tui) result: MediaWorkResult<Protocol>,
}

impl MediaProtocolBuildJob {
    pub(super) fn preview(
        key: ImagePreviewKey,
        generation: u64,
        render_spec: MediaProtocolRenderSpec,
        frame_index: usize,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self {
            target: MediaProtocolBuildTarget::Preview {
                key,
                render_spec,
                frame_index,
            },
            generation,
            picker,
            image,
        }
    }

    pub(super) fn avatar(
        url: String,
        generation: u64,
        key: AvatarFrameProtocolKey,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self {
            target: MediaProtocolBuildTarget::Avatar { url, key },
            generation,
            picker,
            image,
        }
    }

    pub(super) fn emoji(
        url: String,
        generation: u64,
        frame_index: usize,
        image_size: EmojiImageSize,
        picker: Picker,
        image: Arc<DynamicImage>,
    ) -> Self {
        Self {
            target: MediaProtocolBuildTarget::Emoji {
                url,
                frame_index,
                image_size,
            },
            generation,
            picker,
            image,
        }
    }
}

pub(in crate::tui) fn spawn_media_protocol_build(
    job: MediaProtocolBuildJob,
    tx: mpsc::UnboundedSender<MediaProtocolBuildResult>,
) {
    let work_permits = media_image_work_permits().clone();
    let Ok(job_permit) = media_image_job_permits().clone().try_acquire_owned() else {
        let _ = tx.send(MediaProtocolBuildResult {
            target: job.target,
            generation: job.generation,
            result: Err(MediaWorkError::Busy),
        });
        return;
    };
    let target = job.target.clone();
    let generation = job.generation;
    task::spawn(async move {
        let _job_permit = job_permit;
        let _permit = work_permits
            .acquire_owned()
            .await
            .expect("media work semaphore stays open");
        let result = match task::spawn_blocking(move || build_media_protocol(job)).await {
            Ok(result) => result,
            Err(error) => MediaProtocolBuildResult {
                target,
                generation,
                result: Err(MediaWorkError::Failed(format!(
                    "image protocol worker failed: {error}"
                ))),
            },
        };
        let _ = tx.send(result);
    });
}

pub(in crate::tui) fn build_media_protocol(job: MediaProtocolBuildJob) -> MediaProtocolBuildResult {
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
    MediaProtocolBuildResult {
        target: job.target,
        generation: job.generation,
        result,
    }
}
