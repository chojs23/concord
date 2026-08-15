use std::{io::Read, time::Instant};

use ratatui::layout::Rect;
use ratatui_image::{
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};
use tokio::sync::mpsc;

use crate::{
    config::{AnimatePreviews, ImageProtocolPreference},
    discord::{AppCommand, AppEvent, MAX_UPLOAD_PREVIEW_BYTES, MessageAttachmentUpload},
    logging,
    tui::{
        commands as command_helpers,
        media::{
            AvatarImageCache, AvatarTarget, EmojiImageCache, EmojiImageTarget, ImagePreviewCache,
            ImagePreviewTarget, MediaImageDecodeCache, MediaImageDecodeDelivery,
            MediaImageDecodeKey, MediaImageDecodeResult, MediaProtocolBuildResult,
            MediaProtocolBuildTarget, clipped_media_protocol, decode_image_bytes,
            fixed_media_protocol_render_spec, picker_font_size, query_image_picker,
            spawn_media_image_decode, spawn_media_protocol_build, visible_avatar_targets_from_plan,
            visible_emoji_image_targets, visible_image_preview_targets_from_plan,
        },
        message::layout::MessageViewportPlan,
        state::DashboardState,
        ui::{self, ImagePreviewLayout, LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH},
    },
};

use super::placement::{FramePlacements, PlacementDiff};

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
    decoded_images: MediaImageDecodeCache,
    picker: Option<Picker>,
    image_targets: Vec<ImagePreviewTarget>,
    avatar_targets: Vec<AvatarTarget>,
    emoji_targets: Vec<EmojiImageTarget>,
    // Where overlay images sat last frame, so `prepare_frame` can tell which
    // moved/disappeared and need the selective clear pass.
    last_placements: FramePlacements,
    current_placements: FramePlacements,
    placement_diff: PlacementDiff,
    // The profile popup avatar isn't a message-pane target, so its resolved url
    // is carried from `prepare_frame` into the draw closures.
    popup_avatar_url: Option<String>,
}

impl DashboardMediaRuntime {
    pub(super) fn new(protocol_preference: ImageProtocolPreference) -> Self {
        let picker = query_image_picker(protocol_preference);
        Self {
            image_previews: ImagePreviewCache::new(picker.clone()),
            avatar_images: AvatarImageCache::new(picker.clone()),
            emoji_images: EmojiImageCache::new(picker.clone()),
            decoded_images: MediaImageDecodeCache::new(),
            picker,
            image_targets: Vec::new(),
            avatar_targets: Vec::new(),
            emoji_targets: Vec::new(),
            last_placements: FramePlacements::default(),
            current_placements: FramePlacements::default(),
            placement_diff: PlacementDiff::default(),
            popup_avatar_url: None,
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
        let Some(picker) = self.picker.clone() else {
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

    pub(super) fn record_event(
        &mut self,
        event: &AppEvent,
        media_decode_tx: &mpsc::UnboundedSender<MediaImageDecodeResult>,
    ) {
        let preview_requests = self.image_previews.record_event(event);
        let requests = preview_requests
            .into_iter()
            .chain(self.avatar_images.record_event(event))
            .chain(self.emoji_images.record_event(event))
            .collect::<Vec<_>>();
        let AppEvent::AttachmentPreviewLoaded { url, bytes } = event else {
            return;
        };

        let outcome = self.decoded_images.request(url, bytes, requests);
        for delivery in outcome.deliveries {
            self.store_media_decode_delivery(delivery);
        }
        if let Some(job) = outcome.job {
            spawn_media_image_decode(job, media_decode_tx.clone());
        }
    }

    pub(super) fn store_media_decode(&mut self, result: MediaImageDecodeResult) {
        for delivery in self.decoded_images.complete(result) {
            self.store_media_decode_delivery(delivery);
        }
    }

    fn store_media_decode_delivery(&mut self, delivery: MediaImageDecodeDelivery) {
        match delivery.key {
            MediaImageDecodeKey::Preview(key) => {
                self.image_previews
                    .store_decoded(key, delivery.generation, delivery.result);
            }
            MediaImageDecodeKey::Avatar(key) => {
                self.avatar_images
                    .store_decoded(key, delivery.generation, delivery.result);
            }
            MediaImageDecodeKey::Emoji(url) => {
                self.emoji_images
                    .store_decoded(url, delivery.generation, delivery.result);
            }
        }
    }

    pub(super) fn store_media_protocol(&mut self, result: MediaProtocolBuildResult) {
        match &result.target {
            MediaProtocolBuildTarget::Preview { .. } => self.image_previews.store_protocol(result),
            MediaProtocolBuildTarget::Avatar { .. } => self.avatar_images.store_protocol(result),
            MediaProtocolBuildTarget::Emoji { .. } => self.emoji_images.store_protocol(result),
        }
    }

    fn schedule_protocol_builds(&mut self, tx: &mpsc::UnboundedSender<MediaProtocolBuildResult>) {
        for job in self
            .image_previews
            .take_protocol_jobs()
            .into_iter()
            .chain(self.avatar_images.take_protocol_jobs())
            .chain(self.emoji_images.take_protocol_jobs())
        {
            spawn_media_protocol_build(job, tx.clone());
        }
    }

    fn preview_layout_for_draw(
        &self,
        state: &mut DashboardState,
        area: Rect,
    ) -> ImagePreviewLayout {
        let mut preview_layout = ui::image_preview_layout(area, state);
        preview_layout.font_size = self.picker.as_ref().map(picker_font_size);
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
        area: Rect,
    ) {
        self.image_targets = visible_image_preview_targets_from_plan(state, layout, plan);
        let list = ui::image_preview_list_area(area, state);
        let occlusion_areas = ui::background_media_occlusion_areas(area, state);
        self.image_targets = clip_image_preview_targets_for_occlusions(
            std::mem::take(&mut self.image_targets),
            list,
            plan,
            &occlusion_areas,
            ui::avatar_gutter_width(state.show_avatars()),
        );
        self.avatar_targets = visible_avatar_targets_from_plan(state, layout, plan);
        self.emoji_targets = visible_emoji_image_targets(state);
    }

    /// Compute everything the next frame needs *before* drawing: layout, plan,
    /// image/avatar/emoji targets, and where each overlay image lands on screen.
    /// The resolved placements are diffed against the previous frame so the run
    /// loop knows whether a selective clear pass is required and which overlays
    /// to keep in it. The plan borrows `state`, so it is rebuilt here only to
    /// drive target computation and is not stored; the draw closures rebuild
    /// their own plan and reuse the stored owned targets.
    pub(super) fn prepare_frame(&mut self, state: &mut DashboardState, area: Rect) {
        ui::sync_view_heights(area, state);
        let preview_layout = self.preview_layout_for_draw(state, area);
        let messages = state.visible_messages();
        let selected = state.focused_message_selection();
        let plan = MessageViewportPlan::new(
            &messages,
            selected,
            state,
            preview_layout.content_width,
            preview_layout.preview_width,
            preview_layout.max_preview_height,
        );
        self.compute_targets_for_draw(state, preview_layout, &plan, area);
        self.popup_avatar_url = resolve_popup_avatar_url(state);

        let current = self.resolve_placements(state, &plan, area);
        self.placement_diff = current.diff(&self.last_placements);
        self.current_placements = current;
    }

    /// Resolve the absolute screen geometry of every overlay image this frame.
    /// Inline previews reuse the exact renderer path (`plan.row` +
    /// `inline_image_preview_screen_area`), the viewer uses a coarse size-based
    /// fingerprint (it is a single centered image), avatars use their absolute
    /// row, and the popup avatar uses its cropped visible area.
    fn resolve_placements(
        &self,
        state: &DashboardState,
        plan: &MessageViewportPlan<'_>,
        area: Rect,
    ) -> FramePlacements {
        let mut placements = FramePlacements::default();
        let list = ui::image_preview_list_area(area, state);

        for target in &self.image_targets {
            if target.viewer {
                // A single centered image. Its rect changes only on resize or
                // zoom, both of which move width/height, so size alone is enough.
                placements.insert_preview(
                    target.key(),
                    Rect::new(0, 0, target.preview_width, target.preview_height),
                );
                continue;
            }
            if target.thread_card {
                let Some(mut preview_area) = ui::thread_card::thread_card_image_preview_area(
                    list,
                    target.preview_y_offset_rows as isize,
                    target.preview_x_offset_columns,
                    target.preview_width,
                    target.preview_height,
                ) else {
                    continue;
                };
                preview_area.height = preview_area.height.min(target.visible_preview_height);
                placements.insert_preview(target.key(), preview_area);
                continue;
            }
            let Some(row_plan) = plan.row(target.message_index) else {
                continue;
            };
            let row =
                row_plan.image_preview_row(target.body_line_index, target.preview_y_offset_rows);
            let Some(mut preview_area) = ui::inline_image_preview_screen_area(
                list,
                row,
                target.preview_x_offset_columns,
                target.preview_width,
                target.preview_height,
                target.accent_color,
                ui::avatar_gutter_width(state.show_avatars()),
            ) else {
                continue;
            };
            preview_area.height = preview_area.height.min(target.visible_preview_height);
            placements.insert_preview(target.key(), preview_area);
        }

        for target in &self.avatar_targets {
            placements.insert_avatar(
                target.url().to_owned(),
                target.row(),
                (
                    target.row(),
                    target.visible_height(),
                    target.top_clip_rows(),
                ),
            );
        }

        let popup_avatar = self.popup_avatar_url.as_ref().and_then(|url| {
            ui::user_profile_popup_avatar_viewport(area, state)
                .map(|(avatar_area, _)| (url.clone(), state.circular_avatars(), avatar_area))
        });
        placements.set_popup_avatar(popup_avatar);

        placements
    }

    /// Promote this frame's placements to the baseline for the next diff. Called
    /// by the run loop after both frames have been drawn.
    pub(super) fn commit_placements(&mut self) {
        self.last_placements = std::mem::take(&mut self.current_placements);
    }

    pub(super) fn need_clear(&self) -> bool {
        // Kitty's unicode-placeholder protocol auto-removes a placement when its
        // placeholder cells are overwritten, so the normal cell diff erases moved
        // or removed images on its own. The separate erase frame would only add a
        // redundant repaint there (and a residual blink). Skip it for Kitty.
        // iTerm2 and Sixel still need it because they blit pixels the cell diff
        // cannot reach.
        self.placement_diff.need_clear
            && !self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.protocol_type() == ProtocolType::Kitty)
    }

    pub(super) fn sync_animation_visibility(&mut self, now: Instant, animate: AnimatePreviews) {
        self.image_previews
            .sync_animation_visibility(&self.image_targets, now, animate);
        self.avatar_images
            .sync_animation_visibility(&self.avatar_targets, now);
        self.emoji_images
            .sync_animation_visibility(&self.emoji_targets, now);
    }

    pub(super) fn pause_animations(&mut self) {
        self.image_previews.pause_animations();
        self.avatar_images.pause_animations();
        self.emoji_images.pause_animations();
    }

    /// Reports what the media caches are actually holding, next to the
    /// process's resident size. Cache totals that stay flat while RSS climbs
    /// mean the memory is not live data, which is the one thing a bounded
    /// cache cannot tell you from the outside.
    pub(super) fn log_memory_report(&self) {
        let (previews, preview_decoded, preview_protocols) = self.image_previews.retained_stats();
        let (avatars, avatar_decoded, avatar_protocols) = self.avatar_images.retained_stats();
        let (emoji, emoji_decoded, emoji_protocols) = self.emoji_images.retained_stats();
        let (shared, shared_decoded) = self.decoded_images.retained_stats();
        let protocols = preview_protocols
            .saturating_add(avatar_protocols)
            .saturating_add(emoji_protocols);
        logging::debug(
            "media",
            format!(
                "media cache report: previews={previews}/{preview_decoded} avatars={avatars}/{avatar_decoded} emoji={emoji}/{emoji_decoded} shared={shared}/{shared_decoded} protocol_bytes={protocols} live_bytes={} rss_kib={}",
                // Surface entries clone the shared images, so the shared total
                // already covers their decoded bytes.
                shared_decoded.saturating_add(protocols),
                resident_kib().unwrap_or(0),
            ),
        );
    }

    /// Drops everything that failed to load so the next frame requests it
    /// again, however many times it already failed. This is the manual way out
    /// when a picture stays broken past its automatic retries.
    pub(super) fn forget_failed_media(&mut self) {
        self.image_previews.forget_failures();
        self.avatar_images.forget_failures();
        self.emoji_images.forget_failures();
    }

    pub(super) fn next_animation_deadline(&self) -> Option<Instant> {
        [
            self.image_previews.next_animation_deadline(),
            self.avatar_images.next_animation_deadline(),
            self.emoji_images.next_animation_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(super) fn advance_animations(&mut self, now: Instant) -> bool {
        let preview_advanced = self.image_previews.advance_animations(now);
        let avatar_advanced = self.avatar_images.advance_animations(now);
        let emoji_advanced = self.emoji_images.advance_animations(now);
        preview_advanced || avatar_advanced || emoji_advanced
    }
}

/// Resolve which avatar url the profile popup should draw, mirroring the logic
/// in `draw_dashboard_frame`: a pending upload preview takes precedence, then
/// the loaded popup avatar, and only when avatars are enabled at all.
fn resolve_popup_avatar_url(state: &DashboardState) -> Option<String> {
    let pending = state.user_profile_popup_pending_avatar_preview_key();
    state
        .show_avatars()
        .then(|| pending.or_else(|| state.user_profile_popup_avatar_url()))
        .flatten()
        .map(str::to_owned)
}

fn clip_image_preview_targets_for_occlusions(
    targets: Vec<ImagePreviewTarget>,
    list: Rect,
    plan: &MessageViewportPlan<'_>,
    occlusion_areas: &[Rect],
    avatar_offset: u16,
) -> Vec<ImagePreviewTarget> {
    if occlusion_areas.is_empty() {
        return targets;
    }

    let mut clipped = Vec::new();
    for target in targets {
        if target.viewer {
            clipped.push(target);
            continue;
        }

        if target.thread_card {
            let Some(area) = ui::thread_card::thread_card_image_preview_area(
                list,
                target.preview_y_offset_rows as isize,
                target.preview_x_offset_columns,
                target.preview_width,
                target.preview_height,
            ) else {
                continue;
            };
            clipped.extend(visible_image_target_slices(target, area, occlusion_areas));
            continue;
        }

        let Some(row_plan) = plan.row(target.message_index) else {
            continue;
        };
        let row = row_plan.image_preview_row(target.body_line_index, target.preview_y_offset_rows);
        let Some(area) = ui::inline_image_preview_screen_area(
            list,
            row,
            target.preview_x_offset_columns,
            target.preview_width,
            target.preview_height,
            target.accent_color,
            avatar_offset,
        ) else {
            continue;
        };

        clipped.extend(visible_image_target_slices(target, area, occlusion_areas));
    }
    clipped
}

fn visible_image_target_slices(
    target: ImagePreviewTarget,
    area: Rect,
    occlusion_areas: &[Rect],
) -> Vec<ImagePreviewTarget> {
    let mut segments = vec![(area.y, area.y.saturating_add(area.height))];
    for occlusion in occlusion_areas {
        if !rects_intersect_horizontally(area, *occlusion) {
            continue;
        }
        let cut_start = area.y.max(occlusion.y);
        let cut_end = area
            .y
            .saturating_add(area.height)
            .min(occlusion.y.saturating_add(occlusion.height));
        if cut_start >= cut_end {
            continue;
        }
        segments = segments
            .into_iter()
            .flat_map(|(start, end)| {
                let mut next = Vec::new();
                if start < cut_start {
                    next.push((start, cut_start));
                }
                if cut_end < end {
                    next.push((cut_end, end));
                }
                next
            })
            .collect();
    }

    segments
        .into_iter()
        .filter_map(|(start, end)| {
            let additional_top = start.saturating_sub(area.y);
            let visible_height = end.saturating_sub(start);
            if visible_height == 0 {
                return None;
            }
            let mut slice = target.clone();
            slice.preview_y_offset_rows = slice
                .preview_y_offset_rows
                .saturating_add(usize::from(additional_top));
            slice.top_clip_rows = slice.top_clip_rows.saturating_add(additional_top);
            slice.visible_preview_height = visible_height;
            Some(slice)
        })
        .collect()
}

fn rects_intersect_horizontally(a: Rect, b: Rect) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && a.x < b.x.saturating_add(b.width)
        && b.x < a.x.saturating_add(a.width)
}

fn build_local_upload_preview_protocol(
    picker: &Picker,
    attachment: &MessageAttachmentUpload,
) -> std::result::Result<Protocol, String> {
    let bytes = local_upload_preview_bytes(attachment)?;
    let image = decode_image_bytes(&bytes)?;
    clipped_media_protocol(
        picker,
        &image,
        fixed_media_protocol_render_spec(LOCAL_UPLOAD_PREVIEW_WIDTH, LOCAL_UPLOAD_PREVIEW_HEIGHT),
    )
    .ok_or_else(|| "preview dimensions unavailable".to_owned())
}

fn local_upload_preview_bytes(
    attachment: &MessageAttachmentUpload,
) -> std::result::Result<Vec<u8>, String> {
    if let Some(bytes) = attachment.bytes() {
        if bytes.len() as u64 > MAX_UPLOAD_PREVIEW_BYTES {
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
    if metadata.len() > MAX_UPLOAD_PREVIEW_BYTES {
        return Err(format!(
            "attachment preview is too large: {} bytes",
            metadata.len()
        ));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| format!("open attachment preview failed: {error}"))?;
    let mut reader = file.take(MAX_UPLOAD_PREVIEW_BYTES.saturating_add(1));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read attachment preview failed: {error}"))?;
    if bytes.len() as u64 > MAX_UPLOAD_PREVIEW_BYTES {
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
    // The plan borrows `state`, so it cannot be carried out of `prepare_frame`;
    // it is rebuilt here while the targets `prepare_frame` computed are reused.
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

    let image_previews = media_runtime
        .image_previews
        .render_state(&media_runtime.image_targets);
    let rendered_emojis = media_runtime
        .emoji_images
        .render_state(&media_runtime.emoji_targets);
    let popup_avatar_url = media_runtime.popup_avatar_url.as_deref();
    let popup_avatar_clip = ui::user_profile_popup_avatar_viewport(area, state)
        .map(|(avatar_area, top_clip_rows)| (avatar_area.height, top_clip_rows));
    let (rendered_avatars, popup_avatar) = media_runtime.avatar_images.render_state_with_popup(
        &media_runtime.avatar_targets,
        popup_avatar_url,
        popup_avatar_clip,
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
    area
}

/// Draw the whole dashboard but with only the overlay images whose placement
/// stayed put this frame; the moved/removed ones are omitted so their old cells
/// get overpainted with plain content, erasing the stale terminal-graphic
/// pixels there. Unchanged overlays are kept so their cells match the previous
/// frame and the ratatui diff emits nothing for them. All emoji are always
/// drawn because they flow with text and the cell diff moves them naturally.
/// The run loop draws this once, immediately before the real frame, whenever an
/// overlay moved or was covered/uncovered, so the next frame redraws cleanly
/// with no ghost.
pub(super) fn clear_image_surfaces_frame(
    frame: &mut ratatui::Frame<'_>,
    state: &mut DashboardState,
    media_runtime: &mut DashboardMediaRuntime,
) -> Rect {
    let area = frame.area();
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

    // Keep only the overlays whose placement is identical to the previous frame.
    let unchanged_previews: Vec<ImagePreviewTarget> = media_runtime
        .image_targets
        .iter()
        .filter(|target| {
            media_runtime
                .placement_diff
                .unchanged_previews
                .contains(&target.key())
        })
        .cloned()
        .collect();
    let unchanged_avatars: Vec<AvatarTarget> = media_runtime
        .avatar_targets
        .iter()
        .filter(|target| {
            media_runtime
                .placement_diff
                .unchanged_avatars
                .contains(&(target.url().to_owned(), target.row()))
        })
        .cloned()
        .collect();

    let image_previews = media_runtime
        .image_previews
        .render_state(&unchanged_previews);
    let rendered_emojis = media_runtime
        .emoji_images
        .render_state(&media_runtime.emoji_targets);
    // Only keep the popup avatar when it did not move; otherwise omit it so its
    // old cells are overpainted.
    let popup_avatar_url = if media_runtime.placement_diff.popup_avatar_unchanged {
        media_runtime.popup_avatar_url.as_deref()
    } else {
        None
    };
    let popup_avatar_clip = ui::user_profile_popup_avatar_viewport(area, state)
        .map(|(avatar_area, top_clip_rows)| (avatar_area.height, top_clip_rows));
    let (rendered_avatars, popup_avatar) = media_runtime.avatar_images.render_state_with_popup(
        &unchanged_avatars,
        popup_avatar_url,
        popup_avatar_clip,
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
    media_protocol_tx: &mpsc::UnboundedSender<MediaProtocolBuildResult>,
) -> bool {
    let mut dirty = false;
    dirty |= media_runtime.schedule_local_upload_previews(state, local_upload_preview_tx);
    media_runtime.schedule_protocol_builds(media_protocol_tx);
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

/// Resident set size in KiB, for the media cache report. Linux only; other
/// platforms report zero rather than growing a dependency for a debug line.
fn resident_kib() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages.saturating_mul(4))
}

#[cfg(test)]
mod tests {
    use super::visible_image_target_slices;
    use crate::discord::ids::{Id, marker::MessageMarker};
    use crate::tui::media::ImagePreviewTarget;
    use ratatui::layout::Rect;

    #[test]
    fn image_target_slices_keep_visible_rows_above_bottom_overlay() {
        let slices = visible_image_target_slices(
            image_preview_target(),
            Rect::new(10, 2, 20, 10),
            &[Rect::new(0, 8, 80, 4)],
        );

        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].preview_y_offset_rows, 0);
        assert_eq!(slices[0].top_clip_rows, 0);
        assert_eq!(slices[0].visible_preview_height, 6);
    }

    #[test]
    fn image_target_slices_keep_rows_around_middle_overlay() {
        let slices = visible_image_target_slices(
            image_preview_target(),
            Rect::new(10, 2, 20, 10),
            &[Rect::new(0, 5, 80, 3)],
        );

        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].preview_y_offset_rows, 0);
        assert_eq!(slices[0].top_clip_rows, 0);
        assert_eq!(slices[0].visible_preview_height, 3);
        assert_eq!(slices[1].preview_y_offset_rows, 6);
        assert_eq!(slices[1].top_clip_rows, 6);
        assert_eq!(slices[1].visible_preview_height, 4);
    }

    fn image_preview_target() -> ImagePreviewTarget {
        ImagePreviewTarget {
            viewer: false,
            selected: false,
            thread_card: false,
            message_index: 0,
            preview_index: 0,
            body_line_index: None,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: 20,
            preview_height: 10,
            visible_preview_height: 10,
            top_clip_rows: 0,
            accent_color: None,
            show_play_marker: false,
            message_id: Id::<MessageMarker>::new(1),
            url: "https://cdn.discordapp.com/image.png".to_owned(),
            filename: "image.png".to_owned(),
        }
    }
}
