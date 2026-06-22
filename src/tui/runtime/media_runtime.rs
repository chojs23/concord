use std::io::Read;

use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::Protocol};
use tokio::sync::mpsc;

use crate::{
    config::ImageProtocolPreference,
    discord::{AppCommand, DiscordClient, MAX_UPLOAD_FILE_BYTES, MessageAttachmentUpload},
    tui::{
        commands as command_helpers,
        media::{
            AvatarImageCache, AvatarTarget, EmojiImageCache, EmojiImageTarget, ImagePreviewCache,
            ImagePreviewTarget, MediaImageDecodeKey, MediaImageDecodeResult,
            clipped_preview_protocol, decode_image_bytes, fixed_image_preview_render_info,
            query_image_picker, visible_avatar_targets_from_plan, visible_emoji_image_targets,
            visible_image_preview_targets_from_plan,
        },
        message::layout::MessageViewportPlan,
        state::DashboardState,
        ui::{self, ImagePreviewLayout, LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH},
    },
};

use super::effects as effect_helpers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LocalUploadPreviewOwner {
    Composer,
    ForumPost,
}

pub(super) struct LocalUploadPreviewResult {
    pub(super) owner: LocalUploadPreviewOwner,
    pub(super) attachment_index: usize,
    pub(super) generation: u64,
    pub(super) filename: String,
    pub(super) result: std::result::Result<Protocol, String>,
}

pub(super) struct DashboardMediaRuntime {
    image_previews: ImagePreviewCache,
    avatar_images: AvatarImageCache,
    emoji_images: EmojiImageCache,
    local_upload_preview_picker: Option<Picker>,
    image_targets: Vec<ImagePreviewTarget>,
    avatar_targets: Vec<AvatarTarget>,
    emoji_targets: Vec<EmojiImageTarget>,
}

impl DashboardMediaRuntime {
    pub(super) fn new(protocol_preference: ImageProtocolPreference) -> Self {
        Self {
            image_previews: ImagePreviewCache::new_with_protocol_preference(protocol_preference),
            avatar_images: AvatarImageCache::new_with_protocol_preference(protocol_preference),
            emoji_images: EmojiImageCache::new_with_protocol_preference(protocol_preference),
            local_upload_preview_picker: query_image_picker(
                "local upload",
                "local upload image picker unavailable",
                protocol_preference,
            ),
            image_targets: Vec::new(),
            avatar_targets: Vec::new(),
            emoji_targets: Vec::new(),
        }
    }

    pub(super) fn schedule_local_upload_previews(
        &mut self,
        state: &mut DashboardState,
        tx: &mpsc::UnboundedSender<LocalUploadPreviewResult>,
    ) -> bool {
        let mut dirty = false;
        if let Some(work) = state.take_pending_forum_post_attachment_preview() {
            dirty |= self.schedule_local_upload_preview(
                state,
                tx,
                LocalUploadPreviewOwner::ForumPost,
                work,
            );
        }
        if let Some(work) = state.take_pending_composer_attachment_preview() {
            dirty |= self.schedule_local_upload_preview(
                state,
                tx,
                LocalUploadPreviewOwner::Composer,
                work,
            );
        }
        dirty
    }

    fn schedule_local_upload_preview(
        &self,
        state: &mut DashboardState,
        tx: &mpsc::UnboundedSender<LocalUploadPreviewResult>,
        owner: LocalUploadPreviewOwner,
        work: (usize, u64, String, MessageAttachmentUpload),
    ) -> bool {
        let (attachment_index, generation, filename, upload) = work;
        let Some(picker) = self.local_upload_preview_picker.clone() else {
            store_local_upload_preview_result(
                state,
                owner,
                attachment_index,
                generation,
                filename,
                Err("inline preview unavailable in this terminal".to_owned()),
            );
            return true;
        };
        let tx = tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = build_local_upload_preview_protocol(&picker, &upload);
            let _ = tx.send(LocalUploadPreviewResult {
                owner,
                attachment_index,
                generation,
                filename,
                result,
            });
        });
        true
    }

    pub(super) fn effect_context<'a>(
        &'a mut self,
        state: &'a mut DashboardState,
        client: &'a DiscordClient,
        media_decode_tx: &'a mpsc::UnboundedSender<MediaImageDecodeResult>,
    ) -> effect_helpers::EffectContext<'a> {
        effect_helpers::EffectContext {
            state,
            client,
            image_previews: &mut self.image_previews,
            avatar_images: &mut self.avatar_images,
            emoji_images: &mut self.emoji_images,
            media_decode_tx,
        }
    }

    pub(super) fn store_media_decode(&mut self, result: MediaImageDecodeResult) {
        let MediaImageDecodeResult {
            key,
            generation,
            result,
        } = result;
        match key {
            MediaImageDecodeKey::Preview(key) => {
                self.image_previews.store_decoded(key, generation, result);
            }
            MediaImageDecodeKey::Avatar(key) => {
                self.avatar_images.store_decoded(key, generation, result);
            }
            MediaImageDecodeKey::Emoji(url) => {
                self.emoji_images.store_decoded(url, generation, result);
            }
        }
    }

    fn preview_layout_for_draw(
        &self,
        state: &mut DashboardState,
        area: Rect,
    ) -> ImagePreviewLayout {
        let mut preview_layout = ui::image_preview_layout(area, state);
        preview_layout.font_size = self.image_previews.font_size();
        if !state.show_images() {
            preview_layout.preview_width = 0;
            preview_layout.max_preview_height = 0;
            preview_layout.viewer_preview_width = 0;
            preview_layout.viewer_max_preview_height = 0;
        }
        state.clamp_message_viewport_for_image_previews(
            preview_layout.content_width,
            preview_layout.preview_width,
            preview_layout.max_preview_height,
        );
        preview_layout
    }

    fn compute_targets_for_draw(
        &mut self,
        state: &DashboardState,
        layout: ImagePreviewLayout,
        plan: &MessageViewportPlan<'_>,
    ) {
        self.image_targets = visible_image_preview_targets_from_plan(state, layout, plan);
        self.avatar_targets = visible_avatar_targets_from_plan(state, layout, plan);
        self.emoji_targets = visible_emoji_image_targets(state);
    }
}

fn build_local_upload_preview_protocol(
    picker: &Picker,
    attachment: &MessageAttachmentUpload,
) -> std::result::Result<Protocol, String> {
    let bytes = local_upload_preview_bytes(attachment)?;
    let image = decode_image_bytes(&bytes)?;
    clipped_preview_protocol(
        picker,
        &image,
        fixed_image_preview_render_info(LOCAL_UPLOAD_PREVIEW_WIDTH, LOCAL_UPLOAD_PREVIEW_HEIGHT),
    )
    .ok_or_else(|| "preview dimensions unavailable".to_owned())
}

fn local_upload_preview_bytes(
    attachment: &MessageAttachmentUpload,
) -> std::result::Result<Vec<u8>, String> {
    if let Some(bytes) = attachment.bytes() {
        if bytes.len() as u64 > MAX_UPLOAD_FILE_BYTES {
            return Err(format!(
                "attachment preview is too large: {} bytes",
                bytes.len()
            ));
        }
        return Ok(bytes.to_vec());
    }

    let Some(path) = attachment.path() else {
        return Err("attachment preview has no image data".to_owned());
    };
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("stat attachment preview failed: {error}"))?;
    if !metadata.is_file() {
        return Err("attachment preview must be a regular file".to_owned());
    }
    if metadata.len() > MAX_UPLOAD_FILE_BYTES {
        return Err(format!(
            "attachment preview is too large: {} bytes",
            metadata.len()
        ));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| format!("open attachment preview failed: {error}"))?;
    let mut reader = file.take(MAX_UPLOAD_FILE_BYTES.saturating_add(1));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read attachment preview failed: {error}"))?;
    if bytes.len() as u64 > MAX_UPLOAD_FILE_BYTES {
        return Err(format!(
            "attachment preview is too large: {} bytes",
            bytes.len()
        ));
    }
    Ok(bytes)
}

pub(super) fn store_local_upload_preview_result(
    state: &mut DashboardState,
    owner: LocalUploadPreviewOwner,
    attachment_index: usize,
    generation: u64,
    filename: String,
    result: std::result::Result<Protocol, String>,
) {
    match owner {
        LocalUploadPreviewOwner::Composer => state.store_composer_attachment_preview_result(
            attachment_index,
            generation,
            filename,
            result,
        ),
        LocalUploadPreviewOwner::ForumPost => state.store_forum_post_attachment_preview_result(
            attachment_index,
            generation,
            filename,
            result,
        ),
    }
}

pub(super) fn draw_dashboard_frame(
    frame: &mut ratatui::Frame<'_>,
    state: &mut DashboardState,
    media_runtime: &mut DashboardMediaRuntime,
) -> Rect {
    let area = frame.area();
    super::image_layer::begin_frame(area);
    ui::sync_view_heights(area, state);
    let preview_layout = media_runtime.preview_layout_for_draw(state, area);
    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let viewport_plan = MessageViewportPlan::new(
        &messages,
        selected,
        state,
        preview_layout.content_width,
        preview_layout.preview_width,
        preview_layout.max_preview_height,
    );
    media_runtime.compute_targets_for_draw(state, preview_layout, &viewport_plan);

    let image_previews = media_runtime
        .image_previews
        .render_state(&media_runtime.image_targets);
    let rendered_emojis = media_runtime
        .emoji_images
        .render_state(&media_runtime.emoji_targets);
    let pending_popup_avatar_key = state.user_profile_popup_pending_avatar_preview_key();
    let popup_avatar_url = state
        .show_avatars()
        .then(|| pending_popup_avatar_key.or_else(|| state.user_profile_popup_avatar_url()))
        .flatten();
    let (rendered_avatars, popup_avatar) = media_runtime.avatar_images.render_state_with_popup(
        &media_runtime.avatar_targets,
        popup_avatar_url,
        state.circular_avatars(),
    );
    ui::render_with_message_viewport_plan(
        frame,
        state,
        image_previews,
        rendered_avatars,
        rendered_emojis,
        popup_avatar,
        Some(&viewport_plan),
    );
    super::image_layer::end_frame(frame.buffer_mut());
    area
}

pub(super) async fn drain_pending_commands_after_draw(
    state: &mut DashboardState,
    commands: &mpsc::Sender<AppCommand>,
) -> bool {
    let pending_commands = state.drain_pending_commands();
    send_commands_until_closed(state, commands, pending_commands).await
}

pub(super) async fn schedule_media_loads_after_draw(
    state: &mut DashboardState,
    media_runtime: &mut DashboardMediaRuntime,
    commands: &mpsc::Sender<AppCommand>,
    local_upload_preview_tx: &mpsc::UnboundedSender<LocalUploadPreviewResult>,
) -> bool {
    let mut dirty = false;
    dirty |= media_runtime.schedule_local_upload_previews(state, local_upload_preview_tx);
    send_media_request_commands(
        state,
        commands,
        media_runtime
            .image_previews
            .next_requests(&media_runtime.image_targets),
        &mut dirty,
    )
    .await;
    send_media_request_commands(
        state,
        commands,
        media_runtime
            .avatar_images
            .next_requests(&media_runtime.avatar_targets),
        &mut dirty,
    )
    .await;

    // Profile popup avatar isn't part of the message-pane targets, so schedule
    // its fetch separately. It uses a larger avatar CDN size than message-pane
    // avatars, so it may have its own cache entry.
    if state.show_avatars() {
        let command = if let Some(key) = state.user_profile_popup_pending_avatar_preview_key() {
            media_runtime
                .avatar_images
                .next_request_for_profile_upload(key, || {
                    state.user_profile_popup_pending_avatar_upload()
                })
        } else if let Some(url) = state.user_profile_popup_avatar_url().map(str::to_owned) {
            media_runtime.avatar_images.next_request_for_url(&url)
        } else {
            None
        };
        if let Some(command) = command {
            send_media_request_commands(state, commands, [command], &mut dirty).await;
        }
    }

    send_media_request_commands(
        state,
        commands,
        media_runtime
            .emoji_images
            .next_requests(&media_runtime.emoji_targets),
        &mut dirty,
    )
    .await;
    dirty
}

async fn send_media_request_commands(
    state: &mut DashboardState,
    commands: &mpsc::Sender<AppCommand>,
    media_commands: impl IntoIterator<Item = AppCommand>,
    dirty: &mut bool,
) {
    for command in media_commands {
        *dirty = true;
        if command_helpers::send_or_record_closed(state, commands, command)
            .await
            .is_channel_closed()
        {
            break;
        }
    }
}

async fn send_commands_until_closed(
    state: &mut DashboardState,
    commands: &mpsc::Sender<AppCommand>,
    pending_commands: impl IntoIterator<Item = AppCommand>,
) -> bool {
    for command in pending_commands {
        if command_helpers::send_or_record_closed(state, commands, command)
            .await
            .is_channel_closed()
        {
            return true;
        }
    }
    false
}
