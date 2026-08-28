use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::{
    DiscordClient,
    discord::{
        AppEvent, AttachmentDownloadId, DownloadAttachmentSource, MediaPlaybackRequestId,
        MediaPlaybackTarget, ProfileAvatarUpload, read_profile_avatar_image,
    },
    logging,
};

use super::media_adapters;

pub(super) async fn load_attachment_preview(
    client: DiscordClient,
    url: String,
    permits: Arc<Semaphore>,
) {
    let Ok(_permit) = permits.acquire_owned().await else {
        publish_preview_failure(&client, url, "attachment preview limiter closed".to_owned()).await;
        return;
    };
    match timeout(
        media_adapters::ATTACHMENT_PREVIEW_TIMEOUT,
        media_adapters::fetch_attachment_preview(&url),
    )
    .await
    {
        Err(_) => {
            publish_preview_failure(&client, url, "download image preview timed out".to_owned())
                .await;
        }
        Ok(Ok(bytes)) => {
            client
                .publish_event(AppEvent::AttachmentPreviewLoaded { url, bytes })
                .await;
        }
        Ok(Err(message)) => publish_preview_failure(&client, url, message).await,
    }
}

pub(super) async fn load_profile_avatar_preview(
    client: DiscordClient,
    key: String,
    upload: ProfileAvatarUpload,
) {
    match read_profile_avatar_image(&upload).await {
        Ok(image) => {
            client
                .publish_event(AppEvent::AttachmentPreviewLoaded {
                    url: key,
                    bytes: image.bytes,
                })
                .await;
        }
        Err(message) => publish_preview_failure(&client, key, message).await,
    }
}

async fn publish_preview_failure(client: &DiscordClient, url: String, message: String) {
    logging::error("preview", &message);
    client
        .publish_event(AppEvent::AttachmentPreviewLoadFailed { url, message })
        .await;
}

pub(super) async fn open_url(client: DiscordClient, url: String) {
    if let Err(error) = media_adapters::open_url(&url) {
        let message = format!("open url failed: {error}");
        logging::error("app", &message);
        client
            .publish_event(AppEvent::GatewayError { message })
            .await;
    }
}

pub(super) async fn play_media(
    client: DiscordClient,
    target: MediaPlaybackTarget,
    request_id: Option<MediaPlaybackRequestId>,
) {
    let request_id = request_id.unwrap_or_else(|| MediaPlaybackRequestId::new(0));
    if let Err(error) =
        media_adapters::play_media(client.clone(), request_id, &target.url, &target.label).await
    {
        logging::error("media", format!("play media failed: {error}"));
        let label = if target.label.is_empty() {
            "media"
        } else {
            target.label.as_str()
        };
        client
            .publish_event(AppEvent::GatewayError {
                message: format!("play {label} failed: {error}"),
            })
            .await;
    }
}

pub(super) async fn download_attachment(
    client: DiscordClient,
    id: AttachmentDownloadId,
    url: String,
    filename: String,
    source: DownloadAttachmentSource,
    permits: Arc<Semaphore>,
) {
    let Ok(_permit) = permits.acquire_owned().await else {
        publish_download_failure(
            &client,
            id,
            filename,
            "attachment download limiter closed".to_owned(),
            source,
        )
        .await;
        return;
    };
    match media_adapters::download_attachment(&client, id, &url, &filename, source).await {
        Ok(path) => {
            client
                .publish_event(AppEvent::AttachmentDownloadCompleted {
                    id,
                    path: path.display().to_string(),
                    source,
                })
                .await;
        }
        Err(message) => publish_download_failure(&client, id, filename, message, source).await,
    }
}

async fn publish_download_failure(
    client: &DiscordClient,
    id: AttachmentDownloadId,
    filename: String,
    message: String,
    source: DownloadAttachmentSource,
) {
    logging::error("attachment", &message);
    client
        .publish_event(AppEvent::AttachmentDownloadFailed {
            id,
            filename,
            message,
            source,
        })
        .await;
}
