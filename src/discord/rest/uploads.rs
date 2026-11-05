use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use url::Url;
use wreq::{Client, header::CONTENT_TYPE};

use crate::{
    AppError, Result,
    discord::{
        MessageAttachmentUpload, ids::Id, ids::marker::ChannelMarker, validate_attachment_sizes,
    },
};

use super::DiscordRest;

#[derive(Deserialize)]
struct AttachmentUploadResponse {
    attachments: Vec<CloudAttachment>,
}

#[derive(Deserialize)]
struct CloudAttachment {
    id: Value,
    upload_url: String,
    upload_filename: String,
}

impl DiscordRest {
    /// Upload file bytes before creating a message, including a forum's first
    /// message. Only the returned metadata belongs in the message JSON.
    // TODO: Clean up unused uploads after confirmed failures, without deleting
    // attachments when message delivery is uncertain.
    pub(super) async fn upload_attachments(
        &self,
        channel_id: Id<ChannelMarker>,
        attachments: &[MessageAttachmentUpload],
        upload_limit: u64,
    ) -> Result<Vec<Value>> {
        if attachments.is_empty() {
            return Ok(Vec::new());
        }

        // Signed storage URLs are not Discord API endpoints. Do not inherit
        // Discord cookies or headers, follow redirects, or retry uploads.
        let upload_http = crate::support::tls::http_client_builder()
            .map_err(AppError::DiscordRequest)?
            .https_only(true)
            .redirect(wreq::redirect::Policy::none())
            .retry(wreq::retry::Policy::never())
            .build()
            .map_err(|error| {
                AppError::DiscordRequest(format!("create attachment upload client failed: {error}"))
            })?;
        self.upload_attachments_to(
            &format!(
                "https://discord.com/api/v9/channels/{}/attachments",
                channel_id.get()
            ),
            attachments,
            upload_limit,
            &upload_http,
        )
        .await
    }

    async fn upload_attachments_to(
        &self,
        endpoint: &str,
        attachments: &[MessageAttachmentUpload],
        upload_limit: u64,
        upload_http: &Client,
    ) -> Result<Vec<Value>> {
        let sizes = attachment_sizes(attachments).await?;
        validate_attachment_sizes(
            attachments.len(),
            attachments
                .iter()
                .zip(&sizes)
                .map(|(attachment, size)| (attachment.filename.as_str(), *size)),
            upload_limit,
        )?;
        let files: Vec<Value> = attachments
            .iter()
            .zip(&sizes)
            .enumerate()
            .map(|(index, (attachment, size))| {
                json!({
                    "id": index.to_string(),
                    "filename": attachment.filename,
                    "file_size": size,
                })
            })
            .collect();
        let response: AttachmentUploadResponse = self
            .send_json(
                self.raw_http
                    .post(endpoint)
                    .json(&json!({ "files": files })),
                "create attachment uploads",
            )
            .await?;

        // Match echoed IDs rather than response order, so each file receives
        // the URL reserved for its own name and size.
        let mut slots = HashMap::with_capacity(attachments.len());
        for slot in response.attachments {
            let index = slot
                .id
                .as_str()
                .and_then(|id| id.parse::<usize>().ok())
                .or_else(|| slot.id.as_u64().and_then(|id| usize::try_from(id).ok()))
                .ok_or_else(|| {
                    AppError::DiscordRequest(
                        "attachment upload response has no valid ID".to_owned(),
                    )
                })?;
            if index >= attachments.len()
                || slot.upload_filename.is_empty()
                || slots.insert(index, slot).is_some()
            {
                return Err(AppError::DiscordRequest(
                    "attachment upload response does not match the requested files".to_owned(),
                ));
            }
        }
        if slots.len() != attachments.len() {
            return Err(AppError::DiscordRequest(
                "attachment upload response is missing files".to_owned(),
            ));
        }

        let mut uploaded = Vec::with_capacity(attachments.len());
        for (index, (attachment, expected_size)) in attachments.iter().zip(sizes).enumerate() {
            let slot = slots.remove(&index).expect("every upload ID was checked");
            let url = Url::parse(&slot.upload_url).map_err(|_| {
                AppError::DiscordRequest("attachment upload URL is invalid".to_owned())
            })?;
            if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
                return Err(AppError::DiscordRequest(
                    "attachment upload URL contains unexpected credentials or a fragment"
                        .to_owned(),
                ));
            }

            // Read one file at a time and cap reads in case it grew after the
            // size check. Its bytes must still match the reserved upload size.
            let bytes = attachment_bytes(attachment, upload_limit).await?;
            validate_attachment_sizes(
                1,
                std::iter::once((attachment.filename.as_str(), bytes.len() as u64)),
                upload_limit,
            )?;
            if bytes.len() as u64 != expected_size {
                return Err(AppError::DiscordRequest(format!(
                    "attachment {} changed size before upload; attach it again",
                    attachment.filename
                )));
            }
            let response = upload_http
                .put(url.as_str())
                .header(CONTENT_TYPE, upload_content_type(&attachment.filename))
                .body(bytes)
                .send()
                .await
                .map_err(|error| {
                    // Signed URLs carry upload credentials, including in HTTP
                    // errors. Keep them out of user-visible errors and logs.
                    AppError::DiscordRequest(format!(
                        "upload attachment {} failed: {}",
                        attachment.filename,
                        error.without_uri()
                    ))
                })?;
            if !response.status().is_success() {
                return Err(AppError::DiscordRequest(format!(
                    "upload attachment {} failed: HTTP {}",
                    attachment.filename,
                    response.status()
                )));
            }
            uploaded.push(json!({
                "id": index.to_string(),
                "filename": attachment.filename,
                "uploaded_filename": slot.upload_filename,
            }));
        }
        Ok(uploaded)
    }
}

async fn attachment_sizes(attachments: &[MessageAttachmentUpload]) -> Result<Vec<u64>> {
    let mut sizes = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let size = if let Some(path) = attachment.path() {
            tokio::fs::metadata(path)
                .await
                .map_err(|error| {
                    AppError::DiscordRequest(format!(
                        "stat attachment {} failed: {error}",
                        attachment.filename
                    ))
                })?
                .len()
        } else {
            attachment.bytes().map_or(0, |bytes| bytes.len() as u64)
        };
        sizes.push(size);
    }
    Ok(sizes)
}

async fn attachment_bytes(attachment: &MessageAttachmentUpload, limit: u64) -> Result<Vec<u8>> {
    if let Some(bytes) = attachment.bytes() {
        return Ok(bytes.to_vec());
    }
    let Some(path) = attachment.path() else {
        return Err(AppError::DiscordRequest(format!(
            "attachment {} has no data",
            attachment.filename
        )));
    };
    let file = tokio::fs::File::open(path).await.map_err(|error| {
        AppError::DiscordRequest(format!(
            "open attachment {} failed: {error}",
            attachment.filename
        ))
    })?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| {
            AppError::DiscordRequest(format!(
                "read attachment {} failed: {error}",
                attachment.filename
            ))
        })?;
    Ok(bytes)
}

pub(super) fn upload_content_type(filename: &str) -> String {
    mime_guess::from_path(filename)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

#[cfg(test)]
mod tests;
