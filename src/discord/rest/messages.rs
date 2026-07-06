use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::{
    AppError, Result,
    discord::{
        BASE_ATTACHMENT_LIMIT_BYTES, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload,
        MessageInfo, ReplyReference, gateway::parse_message_info,
    },
};

use super::{DiscordRest, clone_array, extra_fields};

pub(in crate::discord) enum MessageEditRequest<'a> {
    Content(&'a str),
    Flags(u64),
}

impl DiscordRest {
    pub async fn send_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
        reply_to: Option<ReplyReference>,
        attachments: &[MessageAttachmentUpload],
        upload_limit: u64,
    ) -> Result<MessageInfo> {
        validate_message_payload(content, attachments, upload_limit)?;
        let body = message_request_body(content, reply_to, attachments);

        self.send_message_body(channel_id, body, attachments, upload_limit)
            .await
    }

    pub async fn send_tts_message(
        &self,
        channel_id: Id<ChannelMarker>,
        content: &str,
    ) -> Result<MessageInfo> {
        validate_message_content(content)?;
        let body = message_request_body_with_tts(content, None, &[], true);

        self.send_message_body(channel_id, body, &[], BASE_ATTACHMENT_LIMIT_BYTES)
            .await
    }

    async fn send_message_body(
        &self,
        channel_id: Id<ChannelMarker>,
        body: Value,
        attachments: &[MessageAttachmentUpload],
        upload_limit: u64,
    ) -> Result<MessageInfo> {
        let request = self.raw_http.post(format!(
            "https://discord.com/api/v9/channels/{}/messages",
            channel_id.get()
        ));

        let request = if attachments.is_empty() {
            request.json(&body)
        } else {
            request.multipart(message_multipart_form(body, attachments, upload_limit).await?)
        };

        let raw: Value = self.send_json(request, "send message").await?;
        parse_message_response(raw, "send message response").map(|response| response.message)
    }

    pub async fn edit_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        request: MessageEditRequest<'_>,
    ) -> Result<MessageInfo> {
        let (body, action) = edit_message_request_body(request)?;
        let raw: Value = self
            .send_json(
                self.raw_http
                    .patch(format!(
                        "https://discord.com/api/v9/channels/{}/messages/{}",
                        channel_id.get(),
                        message_id.get()
                    ))
                    .json(&body),
                action,
            )
            .await?;
        parse_message_response(raw, &format!("{action} response")).map(|response| response.message)
    }

    pub async fn delete_message(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}",
                channel_id.get(),
                message_id.get()
            )),
            "delete message",
        )
        .await
    }

    pub async fn load_message_history(
        &self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        let mut request = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/messages",
                channel_id.get()
            ))
            .query(&[("limit", limit.to_string())]);
        if let Some(message_id) = before {
            request = request.query(&[("before", message_id.to_string())]);
        }
        let raw_messages: Vec<Value> = self.send_json(request, "message history").await?;
        parse_message_list_response(raw_messages, "history message response")
            .map(|response| response.messages)
    }

    /// Recent messages that mention the current user across all guilds, in one
    /// request. This is the endpoint Discord's own inbox uses for its Mentions
    /// tab, so no per-channel fetching is needed. `roles`/`everyone` include
    /// role and @everyone mentions, matching the client default.
    pub async fn load_recent_mentions(&self, limit: u16) -> Result<Vec<MessageInfo>> {
        let request = self
            .raw_http
            .get("https://discord.com/api/v9/users/@me/mentions")
            .query(&[
                ("limit", limit.to_string()),
                ("roles", "true".to_owned()),
                ("everyone", "true".to_owned()),
            ]);
        let raw_messages: Vec<Value> = self.send_json(request, "recent mentions").await?;
        parse_message_list_response(raw_messages, "recent mentions response")
            .map(|response| response.messages)
    }

    pub async fn load_message_history_around(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.load_message_history_with_anchor(channel_id, "around", message_id, limit)
            .await
    }

    pub async fn load_message_history_after(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        self.load_message_history_with_anchor(channel_id, "after", message_id, limit)
            .await
    }

    async fn load_message_history_with_anchor(
        &self,
        channel_id: Id<ChannelMarker>,
        anchor_name: &str,
        message_id: Id<MessageMarker>,
        limit: u16,
    ) -> Result<Vec<MessageInfo>> {
        let request = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/messages",
                channel_id.get()
            ))
            .query(&[("limit", limit.to_string())])
            .query(&[(anchor_name, message_id.to_string())]);
        let raw_messages: Vec<Value> = self.send_json(request, "message history").await?;
        parse_message_list_response(raw_messages, "history message response")
            .map(|response| response.messages)
    }

    pub async fn load_pinned_messages(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Result<Vec<MessageInfo>> {
        let raw: Value = self
            .send_json(
                self.raw_http
                    .get(format!(
                        "https://discord.com/api/v9/channels/{}/messages/pins",
                        channel_id.get()
                    ))
                    .query(&[("limit", "50")]),
                "pins",
            )
            .await?;
        parse_pinned_messages_response(&raw).map(|response| response.messages)
    }

    pub async fn set_message_pinned(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) -> Result<()> {
        let request = if pinned {
            self.raw_http.put(format!(
                "https://discord.com/api/v9/channels/{}/pins/{}",
                channel_id.get(),
                message_id.get()
            ))
        } else {
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/pins/{}",
                channel_id.get(),
                message_id.get()
            ))
        };
        self.send_unit(request, "pin update").await
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PinnedMessagesResponse {
    pub(super) messages: Vec<MessageInfo>,
    pub(super) raw_items: Vec<Value>,
    pub(super) extra_fields: std::collections::BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MessageResponse {
    pub(super) message: MessageInfo,
    pub(super) raw_message: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct MessageListResponse {
    pub(super) messages: Vec<MessageInfo>,
    pub(super) raw_messages: Vec<Value>,
}

pub(super) fn parse_pinned_messages_response(raw: &Value) -> Result<PinnedMessagesResponse> {
    let messages: Vec<&Value> = match raw {
        Value::Array(items) => items.iter().collect(),
        Value::Object(object) => object
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("message"))
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let messages = messages
        .into_iter()
        .map(|raw| {
            parse_message_info(raw).ok_or_else(|| {
                AppError::DiscordRequest("pin message was missing required fields".to_owned())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PinnedMessagesResponse {
        messages,
        raw_items: match raw {
            Value::Array(_) => clone_array(Some(raw)),
            Value::Object(_) => clone_array(raw.get("items")),
            _ => Vec::new(),
        },
        extra_fields: extra_fields(raw, &["items"]),
    })
}

pub(super) fn parse_message_response(raw_message: Value, label: &str) -> Result<MessageResponse> {
    let message = parse_message_info(&raw_message)
        .ok_or_else(|| AppError::DiscordRequest(format!("{label} was missing required fields")))?;
    Ok(MessageResponse {
        message,
        raw_message,
    })
}

fn parse_message_list_response(
    raw_messages: Vec<Value>,
    label: &str,
) -> Result<MessageListResponse> {
    let messages = raw_messages
        .iter()
        .map(|raw| {
            parse_message_info(raw).ok_or_else(|| {
                AppError::DiscordRequest(format!("{label} was missing required fields"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MessageListResponse {
        messages,
        raw_messages,
    })
}

pub(super) fn message_request_body(
    content: &str,
    reply_to: Option<ReplyReference>,
    attachments: &[MessageAttachmentUpload],
) -> Value {
    message_request_body_with_tts(content, reply_to, attachments, false)
}

/// The official client tags every message create with a snowflake nonce. Its
/// absence is a self-bot signal.
fn generate_nonce() -> String {
    const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(DISCORD_EPOCH_MS);
    (now_ms.saturating_sub(DISCORD_EPOCH_MS) << 22).to_string()
}

pub(super) fn message_request_body_with_tts(
    content: &str,
    reply_to: Option<ReplyReference>,
    attachments: &[MessageAttachmentUpload],
    tts: bool,
) -> Value {
    let mut body = json!({ "content": content, "nonce": generate_nonce() });
    if tts {
        body["tts"] = Value::Bool(true);
    }
    if let Some(reply) = reply_to {
        body["message_reference"] = json!({ "message_id": reply.message_id.to_string() });
        // `parse` must be spelled out here: without it, dropping the reply ping
        // would also silence every in-content mention.
        if !reply.mention_author {
            body["allowed_mentions"] = json!({
                "parse": ["users", "roles", "everyone"],
                "replied_user": false,
            });
        }
    }
    if !attachments.is_empty() {
        body["attachments"] = Value::Array(
            attachments
                .iter()
                .enumerate()
                .map(|(index, attachment)| {
                    json!({
                        "id": index,
                        "filename": attachment.filename,
                    })
                })
                .collect(),
        );
    }
    body
}

pub(super) fn edit_message_request_body(
    request: MessageEditRequest<'_>,
) -> Result<(Value, &'static str)> {
    match request {
        MessageEditRequest::Content(content) => {
            validate_message_content(content)?;
            Ok((json!({ "content": content }), "edit message"))
        }
        MessageEditRequest::Flags(flags) => Ok((json!({ "flags": flags }), "update message flags")),
    }
}

pub(super) async fn message_multipart_form(
    body: Value,
    attachments: &[MessageAttachmentUpload],
    upload_limit: u64,
) -> Result<Form> {
    let actual_sizes = attachment_sizes(attachments).await?;
    validate_attachment_sizes(&actual_sizes, upload_limit)?;

    let mut form = Form::new().part(
        "payload_json",
        Part::text(body.to_string())
            .mime_str("application/json")
            .map_err(|error| AppError::DiscordRequest(format!("upload payload failed: {error}")))?,
    );

    for (index, attachment) in attachments.iter().enumerate() {
        let bytes = attachment_bytes(attachment).await?;
        validate_attachment_sizes(
            &[(attachment.filename.clone(), bytes.len() as u64)],
            upload_limit,
        )?;
        let content_type = upload_content_type(&attachment.filename);
        let part = Part::bytes(bytes)
            .file_name(attachment.filename.clone())
            .mime_str(&content_type)
            .map_err(|error| {
                AppError::DiscordRequest(format!(
                    "attachment {} content type failed: {error}",
                    attachment.filename
                ))
            })?;
        form = form.part(format!("files[{index}]"), part);
    }
    Ok(form)
}

async fn attachment_sizes(attachments: &[MessageAttachmentUpload]) -> Result<Vec<(String, u64)>> {
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
            attachment.size_bytes
        };
        sizes.push((attachment.filename.clone(), size));
    }
    Ok(sizes)
}

async fn attachment_bytes(attachment: &MessageAttachmentUpload) -> Result<Vec<u8>> {
    if let Some(bytes) = attachment.bytes() {
        return Ok(bytes.to_vec());
    }
    let Some(path) = attachment.path() else {
        return Err(AppError::DiscordRequest(format!(
            "attachment {} has no data",
            attachment.filename
        )));
    };
    tokio::fs::read(path).await.map_err(|error| {
        AppError::DiscordRequest(format!(
            "read attachment {} failed: {error}",
            attachment.filename
        ))
    })
}

pub(super) fn upload_content_type(filename: &str) -> String {
    mime_guess::from_path(filename)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

pub(super) fn validate_message_payload(
    content: &str,
    attachments: &[MessageAttachmentUpload],
    upload_limit: u64,
) -> Result<()> {
    if content.trim().is_empty() && attachments.is_empty() {
        return Err(AppError::EmptyMessageContent);
    }

    let len = content.chars().count();
    if len > 2_000 {
        return Err(AppError::MessageTooLong { len });
    }

    let sizes = attachments
        .iter()
        .map(|attachment| (attachment.filename.clone(), attachment.size_bytes))
        .collect::<Vec<_>>();
    validate_attachment_sizes(&sizes, upload_limit)
}

/// Discord applies `upload_limit` to each file independently, not to the
/// message's combined size, so this checks every file rather than the total.
fn validate_attachment_sizes(attachments: &[(String, u64)], upload_limit: u64) -> Result<()> {
    if attachments.len() > MAX_UPLOAD_ATTACHMENT_COUNT {
        return Err(AppError::TooManyAttachments {
            count: attachments.len(),
        });
    }

    for (filename, size) in attachments {
        if *size > upload_limit {
            return Err(AppError::AttachmentTooLarge {
                filename: filename.clone(),
                size: *size,
                limit: upload_limit,
            });
        }
    }

    Ok(())
}

pub(super) fn validate_message_content(content: &str) -> Result<()> {
    // No attachments here, so the limit is unused. Pass the base to reuse the
    // shared payload validator.
    validate_message_payload(content, &[], BASE_ATTACHMENT_LIMIT_BYTES)
}
