use std::time::Duration;

use serde_json::Value;
use serde_json::json;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker, GuildMarker},
};
use crate::{
    AppError, Result,
    discord::{
        ArchivedThreadsPage, ChannelInfo, ForumPostCreate, ForumPostDataInfo,
        MessageAttachmentUpload, MessageInfo, MessageSendLimits, ThreadMemberInfo,
        gateway::{
            parse_channel_info, parse_member_info, parse_message_info, parse_thread_member_info,
        },
        validate_message_payload,
    },
};

use super::messages::message_multipart_form;
use super::{DiscordRest, extra_fields};

const ARCHIVED_THREAD_PAGE_LIMIT: u16 = 50;

#[derive(Clone, Debug, PartialEq)]
pub struct CreatedForumPost {
    pub thread: ChannelInfo,
    pub current_user_member: Option<ThreadMemberInfo>,
    pub first_message: Option<MessageInfo>,
}

impl DiscordRest {
    /// Load public archived threads in Discord's archive-time order. Forum and
    /// media posts are public threads, so this is their normal archived-list
    /// endpoint. Active rows continue to come from Gateway state.
    pub async fn load_public_archived_threads(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        before: Option<&str>,
    ) -> Result<ArchivedThreadsPage> {
        let mut query = vec![("limit", ARCHIVED_THREAD_PAGE_LIMIT.to_string())];
        if let Some(before) = before {
            query.push(("before", before.to_owned()));
        }
        let raw: Value = self
            .send_json(
                self.raw_http
                    .get(format!(
                        "https://discord.com/api/v9/channels/{}/threads/archived/public",
                        channel_id.get()
                    ))
                    .query(&query),
                "public archived threads",
            )
            .await?;
        Ok(parse_public_archived_threads_response(
            &raw, guild_id, channel_id,
        ))
    }

    /// Follow a forum post by joining its thread, so the current user receives
    /// notifications (and can then mute it).
    pub async fn follow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.put(format!(
                "https://discord.com/api/v9/channels/{}/thread-members/@me",
                thread_id.get()
            )),
            "follow post",
        )
        .await
    }

    /// Unfollow a forum post by leaving its thread.
    pub async fn unfollow_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/thread-members/@me",
                thread_id.get()
            )),
            "unfollow post",
        )
        .await
    }

    /// Archive ("close") or unarchive a thread (regular thread or forum post).
    pub async fn set_thread_archived(
        &self,
        thread_id: Id<ChannelMarker>,
        archived: bool,
    ) -> Result<()> {
        self.edit_thread(thread_id, &json!({ "archived": archived }))
            .await
    }

    /// Lock or unlock a thread. While locked, members without manage permissions
    /// can no longer reply.
    pub async fn set_thread_locked(
        &self,
        thread_id: Id<ChannelMarker>,
        locked: bool,
    ) -> Result<()> {
        self.edit_thread(thread_id, &json!({ "locked": locked }))
            .await
    }

    /// Pin or unpin a forum post within its parent forum. The pin lives in the
    /// channel `flags` bitfield, so we flip only the PINNED bit and preserve the
    /// other flags (for example REQUIRE_TAG).
    pub async fn set_thread_pinned(
        &self,
        thread_id: Id<ChannelMarker>,
        pinned: bool,
        current_flags: u64,
    ) -> Result<()> {
        const THREAD_FLAG_PINNED: u64 = 1 << 1;
        let flags = if pinned {
            current_flags | THREAD_FLAG_PINNED
        } else {
            current_flags & !THREAD_FLAG_PINNED
        };
        self.edit_thread(thread_id, &json!({ "flags": flags }))
            .await
    }

    /// Edit a thread's general settings in one `PATCH` call: the title, applied
    /// tags (forum posts only), slow-mode cooldown, and auto-archive duration.
    /// This is the popup-driven counterpart to the single-field archive/lock/pin
    /// helpers above.
    pub async fn edit_thread_settings(
        &self,
        thread_id: Id<ChannelMarker>,
        name: &str,
        applied_tags: &[Id<ForumTagMarker>],
        rate_limit_per_user: Option<u64>,
        auto_archive_duration: u64,
    ) -> Result<()> {
        let mut body = json!({
            "name": name,
            "applied_tags": applied_tags
                .iter()
                .map(|tag_id| Value::String(tag_id.to_string()))
                .collect::<Vec<_>>(),
            "auto_archive_duration": auto_archive_duration,
        });
        if let Some(rate_limit_per_user) = rate_limit_per_user {
            body.as_object_mut()
                .expect("thread edit body is an object")
                .insert(
                    "rate_limit_per_user".to_owned(),
                    Value::from(rate_limit_per_user),
                );
        }
        self.edit_thread(thread_id, &body).await
    }

    /// Permanently delete a thread by deleting its underlying channel.
    pub async fn delete_thread(&self, thread_id: Id<ChannelMarker>) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}",
                thread_id.get()
            )),
            "delete thread",
        )
        .await
    }

    /// Apply a partial `PATCH /channels/{id}` edit to a thread. Shared by the
    /// archive/lock/pin actions, which each send one field.
    async fn edit_thread(&self, thread_id: Id<ChannelMarker>, body: &Value) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/channels/{}",
                    thread_id.get()
                ))
                .json(body),
            "edit thread",
        )
        .await
    }

    pub async fn create_forum_post(
        &self,
        post: &ForumPostCreate,
        limits: MessageSendLimits,
        slow_mode: Option<Duration>,
    ) -> Result<CreatedForumPost> {
        let _channel_guard = self.message_sends.acquire(post.channel_id).await;
        self.message_sends
            .ensure_cooldown_elapsed(post.channel_id)?;
        let body = create_forum_post_request_body(
            &post.title,
            &post.content,
            &post.applied_tags,
            &post.attachments,
            limits,
        )?;
        let request = self.raw_http.post(format!(
            "https://discord.com/api/v9/channels/{}/threads",
            post.channel_id.get()
        ));
        let request = if post.attachments.is_empty() {
            request.json(&body)
        } else {
            request.multipart(
                message_multipart_form(body, &post.attachments, limits.max_attachment_bytes)
                    .await?,
            )
        };

        let result = self
            .send_json(request, "create forum post")
            .await
            .and_then(|raw: Value| parse_create_forum_post_response(&raw, Some(post.channel_id)));
        match &result {
            Ok(_) => {
                if let Some(slow_mode) = slow_mode {
                    self.message_sends
                        .record_cooldown(post.channel_id, slow_mode);
                }
            }
            Err(AppError::DiscordRateLimited {
                retry_after_millis, ..
            }) => self
                .message_sends
                .record_cooldown(post.channel_id, Duration::from_millis(*retry_after_millis)),
            Err(_) => {}
        }
        result
    }

    pub async fn load_forum_post_data(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        thread_ids: &[Id<ChannelMarker>],
    ) -> Result<Vec<ForumPostDataInfo>> {
        if thread_ids.is_empty() {
            return Ok(Vec::new());
        }
        let body = json!({
            "thread_ids": thread_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        });
        let raw: Value = self
            .send_json(
                self.raw_http
                    .post(format!(
                        "https://discord.com/api/v9/channels/{}/post-data",
                        channel_id.get()
                    ))
                    .json(&body),
                "forum post data",
            )
            .await?;
        Ok(parse_forum_post_data_response(&raw, guild_id, thread_ids))
    }
}

pub(super) fn create_forum_post_request_body(
    title: &str,
    content: &str,
    applied_tags: &[Id<ForumTagMarker>],
    attachments: &[MessageAttachmentUpload],
    limits: MessageSendLimits,
) -> Result<Value> {
    let title = validate_forum_post_title(title)?;
    validate_message_payload(content, attachments, limits)?;

    let mut body = json!({
        "name": title,
        "message": {
            "content": content,
        },
    });
    if !applied_tags.is_empty() {
        body["applied_tags"] = Value::Array(
            applied_tags
                .iter()
                .map(|tag_id| Value::String(tag_id.to_string()))
                .collect(),
        );
    }
    if !attachments.is_empty() {
        body["message"]["attachments"] = Value::Array(
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
    Ok(body)
}

fn validate_forum_post_title(title: &str) -> Result<&str> {
    let title = title.trim();
    let len = title.chars().count();
    if len == 0 {
        return Err(AppError::DiscordRequest(
            "forum post title cannot be empty".to_owned(),
        ));
    }
    if len > 100 {
        return Err(AppError::DiscordRequest(format!(
            "forum post title is too long: {len}/100"
        )));
    }
    Ok(title)
}

pub(super) fn parse_create_forum_post_response(
    raw: &Value,
    parent_channel_id: Option<Id<ChannelMarker>>,
) -> Result<CreatedForumPost> {
    let mut thread = parse_channel_info(raw, None).ok_or_else(|| {
        AppError::DiscordRequest("create forum post response was missing thread".to_owned())
    })?;
    if thread.parent_id.is_none() {
        thread.parent_id = parent_channel_id;
    }
    let current_user_member = raw
        .get("member")
        .filter(|member| member.is_object())
        .or_else(|| raw.get("thread_member").filter(|member| member.is_object()))
        .and_then(|member| {
            parse_thread_member_info(member, thread.guild_id, Some(thread.channel_id))
        });
    let first_message = raw.get("message").and_then(parse_message_info);
    Ok(CreatedForumPost {
        thread,
        current_user_member,
        first_message,
    })
}

pub(super) fn parse_public_archived_threads_response(
    raw: &Value,
    guild_id: Id<GuildMarker>,
    parent_channel_id: Id<ChannelMarker>,
) -> ArchivedThreadsPage {
    let raw_threads = raw
        .get("threads")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let threads = raw_threads
        .iter()
        .filter_map(|raw_thread| {
            let mut thread = parse_channel_info(raw_thread, Some(guild_id))?;
            if thread.parent_id.is_none() {
                thread.parent_id = Some(parent_channel_id);
            }
            (thread.parent_id == Some(parent_channel_id) && thread.thread_archived() == Some(true))
                .then_some(thread)
        })
        .collect::<Vec<_>>();
    let thread_ids = threads
        .iter()
        .map(|thread| thread.channel_id)
        .collect::<std::collections::BTreeSet<_>>();
    let members = raw
        .get("members")
        .and_then(Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| parse_thread_member_info(member, Some(guild_id), None))
                .filter(|member| {
                    member
                        .thread_id
                        .is_some_and(|thread_id| thread_ids.contains(&thread_id))
                })
                .collect()
        })
        .unwrap_or_default();

    // `before` is not returned as a separate cursor. Discord documents that
    // the rows are sorted by archive timestamp descending, so the final raw
    // row's timestamp is the next page cursor even if that row was malformed
    // and could not become a local channel.
    let next_before = raw_threads.iter().rev().find_map(|thread| {
        thread
            .get("thread_metadata")
            .and_then(|metadata| metadata.get("archive_timestamp"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && !raw_threads.is_empty()
        && next_before.is_some();

    ArchivedThreadsPage {
        threads,
        members,
        has_more,
        next_before,
        extra_fields: extra_fields(raw, &["threads", "members", "has_more"]),
    }
}

pub(super) fn parse_forum_post_data_response(
    raw: &Value,
    guild_id: Id<GuildMarker>,
    requested_thread_ids: &[Id<ChannelMarker>],
) -> Vec<ForumPostDataInfo> {
    let Some(posts) = raw.get("threads").and_then(Value::as_object) else {
        return Vec::new();
    };
    requested_thread_ids
        .iter()
        .filter_map(|thread_id| {
            let raw_post = posts.get(&thread_id.to_string())?;
            let owner = raw_post
                .get("owner")
                .filter(|owner| !owner.is_null())
                .and_then(|owner| parse_member_info(owner, Some(guild_id)));
            let first_message = raw_post
                .get("first_message")
                .filter(|message| !message.is_null())
                .and_then(parse_message_info)
                .filter(|message| message.channel_id == *thread_id);
            Some(ForumPostDataInfo {
                thread_id: *thread_id,
                owner,
                first_message,
                extra_fields: extra_fields(raw_post, &["owner", "first_message"]),
            })
        })
        .collect()
}
