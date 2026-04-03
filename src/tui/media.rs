use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::{Resize, picker::Picker, protocol::StatefulProtocol};
use tokio::{sync::mpsc, task};
use twilight_model::id::{Id, marker::MessageMarker};

mod targets;

pub(super) use targets::{
    AvatarTarget, EmojiImageTarget, ImagePreviewTarget, image_preview_height_for_dimensions,
    visible_avatar_targets, visible_emoji_image_targets, visible_image_preview_targets,
};

use crate::{
    discord::{AppCommand, AppEvent},
    logging,
};

use super::ui::{AvatarImage, EmojiReactionImage, ImagePreview, ImagePreviewState};

const AVATAR_PREVIEW_WIDTH: u16 = 2;
const AVATAR_PREVIEW_HEIGHT: u16 = 2;
const PROFILE_POPUP_AVATAR_WIDTH: u16 = 8;
const PROFILE_POPUP_AVATAR_HEIGHT: u16 = 4;
const EMOJI_REACTION_THUMB_WIDTH: u16 = 2;
const EMOJI_REACTION_THUMB_HEIGHT: u16 = 1;
const MAX_IMAGE_PREVIEW_CACHE_ENTRIES: usize = 32;

fn query_image_picker(target: &str, unavailable_message: &str) -> Option<Picker> {
    match Picker::from_query_stdio() {
        Ok(picker) => Some(picker),
        Err(error) => {
            logging::error(target, format!("{unavailable_message}: {error}"));
            None
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ImagePreviewKey {
    message_id: Id<MessageMarker>,
    url: String,
}

pub(super) struct ImagePreviewCache {
    picker: Option<Picker>,
    entries: HashMap<ImagePreviewKey, ImagePreviewEntry>,
    tick: u64,
    decode_generation: u64,
}

pub(super) struct ImagePreviewDecodeJob {
    key: ImagePreviewKey,
    generation: u64,
    bytes: Arc<[u8]>,
    font_size: (u16, u16),
    render_info: ImagePreviewRenderInfo,
}

pub(super) struct ImagePreviewDecodeResult {
    key: ImagePreviewKey,
    generation: u64,
    result: std::result::Result<DynamicImage, String>,
}

enum ImagePreviewEntry {
    Loading {
        filename: String,
        render_info: ImagePreviewRenderInfo,
        last_used: u64,
    },
    Decoding {
        filename: String,
        generation: u64,
        last_used: u64,
    },
    Ready {
        filename: String,
        image: DynamicImage,
        protocol: Box<StatefulProtocol>,
        last_used: u64,
    },
    Failed {
        filename: String,
        message: String,
        last_used: u64,
    },
}

pub(super) struct AvatarImageCache {
    picker: Option<Picker>,
    entries: HashMap<String, AvatarImageEntry>,
}

enum AvatarImageEntry {
    Loading,
    Ready { image: DynamicImage },
    Failed,
}

pub(super) struct EmojiImageCache {
    picker: Option<Picker>,
    entries: HashMap<String, EmojiImageEntry>,
}

enum EmojiImageEntry {
    Loading,
    Ready {
        protocol: ratatui_image::protocol::Protocol,
    },
    Failed,
}

impl ImagePreviewCache {
    pub(super) fn new() -> Self {
        Self {
            picker: query_image_picker("preview", "inline image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        }
    }

    pub(super) fn render_state(&mut self, targets: &[ImagePreviewTarget]) -> Vec<ImagePreview<'_>> {
        self.prune_to_limit(targets);
        let picker = self.picker.clone();
        let target_by_key = targets
            .iter()
            .map(|target| (target.key(), target.preview_render_info()))
            .collect::<HashMap<_, _>>();
        let mut rendered_keys = HashSet::new();
        let mut previews = Vec::new();

        for (key, entry) in &mut self.entries {
            let Some(render_info) = target_by_key.get(key).copied() else {
                continue;
            };
            rendered_keys.insert(key.clone());
            tick_entry(entry, &mut self.tick);
            let state = match entry {
                ImagePreviewEntry::Loading { filename, .. }
                | ImagePreviewEntry::Decoding { filename, .. } => ImagePreviewState::Loading {
                    filename: filename.clone(),
                },
                ImagePreviewEntry::Ready {
                    image, protocol, ..
                } => {
                    if render_info.needs_crop()
                        && let Some(protocol) = picker
                            .as_ref()
                            .and_then(|picker| clipped_preview_protocol(picker, image, render_info))
                    {
                        ImagePreviewState::ReadyCropped(protocol)
                    } else {
                        ImagePreviewState::Ready {
                            protocol: protocol.as_mut(),
                        }
                    }
                }
                ImagePreviewEntry::Failed {
                    filename, message, ..
                } => ImagePreviewState::Failed {
                    filename: filename.clone(),
                    message: message.clone(),
                },
            };
            previews.push(ImagePreview {
                message_index: render_info.message_index,
                preview_height: render_info.preview_height,
                accent_color: render_info.accent_color,
                state,
            });
        }

        for target in targets.iter() {
            if !rendered_keys.contains(&target.key()) {
                previews.push(ImagePreview {
                    message_index: target.message_index,
                    preview_height: target.preview_height,
                    accent_color: target.accent_color,
                    state: ImagePreviewState::Loading {
                        filename: target.filename.clone(),
                    },
                });
            }
        }

        previews.sort_by_key(|preview| preview.message_index);
        previews
    }

    pub(super) fn next_requests(&mut self, targets: &[ImagePreviewTarget]) -> Vec<AppCommand> {
        let mut commands = Vec::new();
        let mut requested_urls = HashSet::new();
        for target in targets.iter().take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES) {
            let key = target.key();
            if self.entries.contains_key(&key) {
                continue;
            }

            let url = target.url.clone();
            let last_used = self.next_tick();
            self.entries.insert(
                key,
                ImagePreviewEntry::Loading {
                    filename: target.filename.clone(),
                    render_info: target.preview_render_info(),
                    last_used,
                },
            );
            if requested_urls.insert(url.clone()) {
                commands.push(AppCommand::LoadAttachmentPreview { url });
            }
        }
        self.prune_to_limit(targets);
        commands
    }

    pub(super) fn record_event(&mut self, event: &AppEvent) -> Vec<ImagePreviewDecodeJob> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                self.store_failed(url, message.clone());
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) -> Vec<ImagePreviewDecodeJob> {
        let keys = self.loading_keys_for_url(url);
        if keys.is_empty() {
            return Vec::new();
        }

        let Some(font_size) = self.picker.as_ref().map(Picker::font_size) else {
            for key in keys {
                let filename = self.filename_for_key(&key);
                let last_used = self.next_tick();
                self.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "inline preview unavailable in this terminal".to_owned(),
                        last_used,
                    },
                );
            }
            return Vec::new();
        };

        self.decode_jobs_for_loaded_keys(keys, bytes, font_size)
    }

    fn decode_jobs_for_loaded_keys(
        &mut self,
        keys: Vec<ImagePreviewKey>,
        bytes: &[u8],
        font_size: (u16, u16),
    ) -> Vec<ImagePreviewDecodeJob> {
        let bytes: Arc<[u8]> = Arc::from(bytes.to_vec());
        let mut jobs = Vec::new();
        for key in keys {
            let filename = self.filename_for_key(&key);
            let Some(render_info) = self.render_info_for_key(&key) else {
                let last_used = self.next_tick();
                self.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "preview dimensions unavailable".to_owned(),
                        last_used,
                    },
                );
                continue;
            };
            let last_used = self.next_tick();
            let generation = self.next_decode_generation();
            self.entries.insert(
                key.clone(),
                ImagePreviewEntry::Decoding {
                    filename,
                    generation,
                    last_used,
                },
            );
            jobs.push(ImagePreviewDecodeJob {
                key,
                generation,
                bytes: bytes.clone(),
                font_size,
                render_info,
            });
        }
        jobs
    }

    pub(super) fn store_decoded(&mut self, result: ImagePreviewDecodeResult) {
        let Some(ImagePreviewEntry::Decoding {
            filename,
            generation,
            ..
        }) = self.entries.get(&result.key)
        else {
            return;
        };
        if *generation != result.generation {
            return;
        }
        let filename = filename.clone();
        let last_used = self.next_tick();
        match result.result {
            Ok(image) => {
                let Some(picker) = self.picker.as_ref() else {
                    self.entries.insert(
                        result.key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: "inline preview unavailable in this terminal".to_owned(),
                            last_used,
                        },
                    );
                    return;
                };
                self.entries.insert(
                    result.key,
                    ImagePreviewEntry::Ready {
                        filename,
                        image: image.clone(),
                        protocol: Box::new(picker.new_resize_protocol(image)),
                        last_used,
                    },
                );
            }
            Err(message) => {
                self.entries.insert(
                    result.key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message,
                        last_used,
                    },
                );
            }
        }
    }

    fn render_info_for_key(&self, key: &ImagePreviewKey) -> Option<ImagePreviewRenderInfo> {
        match self.entries.get(key)? {
            ImagePreviewEntry::Loading { render_info, .. } => Some(*render_info),
            ImagePreviewEntry::Decoding { .. }
            | ImagePreviewEntry::Ready { .. }
            | ImagePreviewEntry::Failed { .. } => None,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn next_decode_generation(&mut self) -> u64 {
        self.decode_generation = self.decode_generation.saturating_add(1);
        self.decode_generation
    }

    fn prune_to_limit(&mut self, targets: &[ImagePreviewTarget]) {
        if self.entries.len() <= MAX_IMAGE_PREVIEW_CACHE_ENTRIES {
            return;
        }

        let protected = targets
            .iter()
            .take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES)
            .map(ImagePreviewTarget::key)
            .collect::<HashSet<_>>();
        let mut removable = self
            .entries
            .iter()
            .filter(|(key, _)| !protected.contains(*key))
            .map(|(key, entry)| (key.clone(), entry.last_used()))
            .collect::<Vec<_>>();
        removable.sort_by_key(|(_, last_used)| *last_used);

        for (key, _) in removable {
            if self.entries.len() <= MAX_IMAGE_PREVIEW_CACHE_ENTRIES {
                break;
            }
            self.entries.remove(&key);
        }
    }

    fn store_failed(&mut self, url: &str, message: String) {
        for key in self.loading_keys_for_url(url) {
            let filename = self.filename_for_key(&key);
            let last_used = self.next_tick();
            self.entries.insert(
                key,
                ImagePreviewEntry::Failed {
                    filename,
                    message: message.clone(),
                    last_used,
                },
            );
        }
    }

    fn loading_keys_for_url(&self, url: &str) -> Vec<ImagePreviewKey> {
        self.entries
            .iter()
            .filter(|(key, entry)| {
                key.url == url && matches!(entry, ImagePreviewEntry::Loading { .. })
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn filename_for_key(&self, key: &ImagePreviewKey) -> String {
        self.entries
            .get(key)
            .map(ImagePreviewEntry::filename)
            .unwrap_or("image")
            .to_owned()
    }
}

impl AvatarImageCache {
    pub(super) fn new() -> Self {
        Self {
            picker: query_image_picker("avatar", "avatar image picker unavailable"),
            entries: HashMap::new(),
        }
    }

    pub(super) fn render_state(&self, targets: &[AvatarTarget]) -> Vec<AvatarImage> {
        let Some(picker) = self.picker.as_ref() else {
            return Vec::new();
        };

        targets
            .iter()
            .filter_map(|target| {
                let AvatarImageEntry::Ready { image } = self.entries.get(&target.url)? else {
                    return None;
                };
                let render_info = ImagePreviewRenderInfo {
                    message_index: 0,
                    preview_width: AVATAR_PREVIEW_WIDTH,
                    preview_height: AVATAR_PREVIEW_HEIGHT,
                    visible_preview_height: target.visible_height,
                    top_clip_rows: target.top_clip_rows,
                    accent_color: None,
                };
                clipped_preview_protocol(picker, image, render_info).map(|protocol| AvatarImage {
                    row: target.row,
                    visible_height: target.visible_height,
                    protocol,
                })
            })
            .collect()
    }

    pub(super) fn next_requests(&mut self, targets: &[AvatarTarget]) -> Vec<AppCommand> {
        targets
            .iter()
            .filter_map(|target| self.next_request_for_url(&target.url))
            .collect()
    }

    /// Schedules an out-of-band avatar fetch (used by the profile popup,
    /// whose URL does not appear in the message-pane avatar targets).
    pub(super) fn next_request_for_url(&mut self, url: &str) -> Option<AppCommand> {
        if self.entries.contains_key(url) {
            return None;
        }
        self.entries
            .insert(url.to_owned(), AvatarImageEntry::Loading);
        Some(AppCommand::LoadAttachmentPreview {
            url: url.to_owned(),
        })
    }

    /// Renders a freshly sized protocol for the profile popup. The cache is
    /// keyed by URL, so this reuses any image already fetched by the message
    /// pane and naturally requests when the popup opens an unseen avatar.
    pub(super) fn popup_avatar_image(&self, url: &str) -> Option<AvatarImage> {
        let picker = self.picker.as_ref()?;
        let AvatarImageEntry::Ready { image } = self.entries.get(url)? else {
            return None;
        };
        let render_info = ImagePreviewRenderInfo {
            message_index: 0,
            preview_width: PROFILE_POPUP_AVATAR_WIDTH,
            preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            visible_preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            top_clip_rows: 0,
            accent_color: None,
        };
        clipped_preview_protocol(picker, image, render_info).map(|protocol| AvatarImage {
            row: 0,
            visible_height: PROFILE_POPUP_AVATAR_HEIGHT,
            protocol,
        })
    }

    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => self.store_failed(url),
            _ => {}
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) {
        if !self.entries.contains_key(url) {
            return;
        }

        if self.picker.is_none() {
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed);
            return;
        }

        match image::load_from_memory(bytes) {
            Ok(image) => {
                self.entries
                    .insert(url.to_owned(), AvatarImageEntry::Ready { image });
            }
            Err(_) => {
                self.entries
                    .insert(url.to_owned(), AvatarImageEntry::Failed);
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed);
        }
    }
}

impl EmojiImageCache {
    pub(super) fn new() -> Self {
        Self {
            picker: query_image_picker("emoji", "emoji image picker unavailable"),
            entries: HashMap::new(),
        }
    }

    pub(super) fn render_state(&self, targets: &[EmojiImageTarget]) -> Vec<EmojiReactionImage<'_>> {
        targets
            .iter()
            .filter_map(|target| {
                let EmojiImageEntry::Ready { protocol } = self.entries.get(&target.url)? else {
                    return None;
                };
                Some(EmojiReactionImage {
                    url: target.url.clone(),
                    protocol,
                })
            })
            .collect()
    }

    pub(super) fn next_requests(&mut self, targets: &[EmojiImageTarget]) -> Vec<AppCommand> {
        if self.picker.is_none() {
            return Vec::new();
        }

        let mut commands = Vec::new();
        let mut requested_urls = HashSet::new();
        for target in targets {
            if self.entries.contains_key(&target.url) {
                continue;
            }

            self.entries
                .insert(target.url.clone(), EmojiImageEntry::Loading);
            if requested_urls.insert(target.url.clone()) {
                commands.push(AppCommand::LoadAttachmentPreview {
                    url: target.url.clone(),
                });
            }
        }
        commands
    }

    pub(super) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => self.store_failed(url),
            _ => {}
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) {
        if !self.entries.contains_key(url) {
            return;
        }

        let Some(picker) = self.picker.as_ref() else {
            self.entries.insert(url.to_owned(), EmojiImageEntry::Failed);
            return;
        };

        match image::load_from_memory(bytes) {
            Ok(image) => {
                let render_info = ImagePreviewRenderInfo {
                    message_index: 0,
                    preview_width: EMOJI_REACTION_THUMB_WIDTH,
                    preview_height: EMOJI_REACTION_THUMB_HEIGHT,
                    visible_preview_height: EMOJI_REACTION_THUMB_HEIGHT,
                    top_clip_rows: 0,
                    accent_color: None,
                };
                if let Some(protocol) = clipped_preview_protocol(picker, &image, render_info) {
                    self.entries
                        .insert(url.to_owned(), EmojiImageEntry::Ready { protocol });
                } else {
                    self.entries.insert(url.to_owned(), EmojiImageEntry::Failed);
                }
            }
            Err(_) => {
                self.entries.insert(url.to_owned(), EmojiImageEntry::Failed);
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            self.entries.insert(url.to_owned(), EmojiImageEntry::Failed);
        }
    }
}

impl ImagePreviewTarget {
    fn key(&self) -> ImagePreviewKey {
        ImagePreviewKey {
            message_id: self.message_id,
            url: self.url.clone(),
        }
    }

    fn preview_render_info(&self) -> ImagePreviewRenderInfo {
        ImagePreviewRenderInfo {
            message_index: self.message_index,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            accent_color: self.accent_color,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImagePreviewRenderInfo {
    message_index: usize,
    preview_width: u16,
    preview_height: u16,
    visible_preview_height: u16,
    top_clip_rows: u16,
    accent_color: Option<u32>,
}

impl ImagePreviewRenderInfo {
    fn needs_crop(self) -> bool {
        self.top_clip_rows > 0 || self.visible_preview_height < self.preview_height
    }
}

impl ImagePreviewEntry {
    fn filename(&self) -> &str {
        match self {
            Self::Loading { filename, .. }
            | Self::Decoding { filename, .. }
            | Self::Ready { filename, .. }
            | Self::Failed { filename, .. } => filename,
        }
    }

    fn last_used(&self) -> u64 {
        match self {
            Self::Loading { last_used, .. }
            | Self::Decoding { last_used, .. }
            | Self::Ready { last_used, .. }
            | Self::Failed { last_used, .. } => *last_used,
        }
    }
}

fn tick_entry(entry: &mut ImagePreviewEntry, tick: &mut u64) {
    *tick = tick.saturating_add(1);
    let last_used = *tick;
    match entry {
        ImagePreviewEntry::Loading {
            last_used: value, ..
        }
        | ImagePreviewEntry::Decoding {
            last_used: value, ..
        }
        | ImagePreviewEntry::Ready {
            last_used: value, ..
        }
        | ImagePreviewEntry::Failed {
            last_used: value, ..
        } => *value = last_used,
    }
}

pub(super) fn spawn_image_preview_decode(
    job: ImagePreviewDecodeJob,
    tx: mpsc::UnboundedSender<ImagePreviewDecodeResult>,
) {
    task::spawn_blocking(move || {
        let result = decode_image_preview(job);
        let _ = tx.send(result);
    });
}

fn decode_image_preview(job: ImagePreviewDecodeJob) -> ImagePreviewDecodeResult {
    let result = decode_preview_sized_image(&job.bytes, job.font_size, job.render_info);
    ImagePreviewDecodeResult {
        key: job.key,
        generation: job.generation,
        result,
    }
}

fn decode_preview_sized_image(
    bytes: &[u8],
    font_size: (u16, u16),
    render_info: ImagePreviewRenderInfo,
) -> std::result::Result<DynamicImage, String> {
    let decoded =
        image::load_from_memory(bytes).map_err(|error| format!("decode failed: {error}"))?;
    preview_sized_image(&decoded, font_size, render_info)
        .ok_or_else(|| "preview dimensions unavailable".to_owned())
}

fn preview_sized_image(
    image: &DynamicImage,
    font_size: (u16, u16),
    render_info: ImagePreviewRenderInfo,
) -> Option<DynamicImage> {
    let (font_width, font_height) = font_size;
    let width = u32::from(render_info.preview_width).checked_mul(u32::from(font_width))?;
    let height = u32::from(render_info.preview_height).checked_mul(u32::from(font_height))?;
    if width == 0 || height == 0 {
        return None;
    }

    Some(image.resize(width, height, FilterType::Triangle))
}

fn clipped_preview_protocol(
    picker: &Picker,
    image: &DynamicImage,
    render_info: ImagePreviewRenderInfo,
) -> Option<ratatui_image::protocol::Protocol> {
    if render_info.preview_width == 0
        || render_info.preview_height == 0
        || render_info.visible_preview_height == 0
    {
        return None;
    }

    let (font_width, font_height) = picker.font_size();
    let full_width = u32::from(render_info.preview_width).checked_mul(u32::from(font_width))?;
    let full_height = u32::from(render_info.preview_height).checked_mul(u32::from(font_height))?;
    let crop_top = u32::from(render_info.top_clip_rows).checked_mul(u32::from(font_height))?;
    let crop_height = u32::from(render_info.visible_preview_height)
        .checked_mul(u32::from(font_height))?
        .min(full_height.saturating_sub(crop_top));
    if full_width == 0 || crop_height == 0 {
        return None;
    }

    let fitted = fit_image_to_canvas(image, full_width, full_height);
    let cropped = fitted.crop_imm(0, crop_top, full_width, crop_height);
    picker
        .new_protocol(
            cropped,
            Rect::new(
                0,
                0,
                render_info.preview_width,
                render_info.visible_preview_height,
            ),
            Resize::Fit(None),
        )
        .ok()
}

fn fit_image_to_canvas(image: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    let resized = image.resize(width, height, FilterType::Nearest);
    if resized.width() == width && resized.height() == height {
        return resized;
    }

    let mut canvas =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    image::imageops::overlay(&mut canvas, &resized, 0, 0);
    canvas
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use image::{DynamicImage, ImageBuffer, Rgba};
    use twilight_model::id::{Id, marker::MessageMarker};

    use crate::{
        discord::{
            AppCommand, AppEvent, AttachmentInfo, ChannelInfo, CustomEmojiInfo, EmbedInfo,
            MessageSnapshotInfo,
        },
        tui::{
            state::{DashboardState, FocusPane},
            ui::ImagePreviewLayout,
        },
    };

    use super::*;

    fn layout(list_height: usize) -> ImagePreviewLayout {
        ImagePreviewLayout {
            list_height,
            content_width: 200,
            preview_width: 16,
            max_preview_height: 3,
        }
    }

    #[test]
    fn image_preview_targets_stop_at_rendered_row_budget() {
        let mut state = state_with_image_messages(6, &[1, 3, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_include_preview_that_would_be_clipped() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_account_for_first_message_line_offset() {
        let mut state = state_with_image_messages(1, &[1]);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(&state, layout(2));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn avatar_targets_include_visible_author_avatar() {
        let state = state_with_avatar_messages(1);

        let targets = visible_avatar_targets(&state, layout(2));

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].row, 0);
        assert_eq!(targets[0].visible_height, 2);
        assert_eq!(targets[0].top_clip_rows, 0);
        assert_eq!(targets[0].url, "https://cdn.discordapp.com/avatar-1.png");
    }

    #[test]
    fn avatar_targets_clip_first_message_avatar_after_line_scroll() {
        let mut state = state_with_avatar_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_avatar_targets(&state, layout(1));

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].row, 0);
        assert_eq!(targets[0].visible_height, 1);
        assert_eq!(targets[0].top_clip_rows, 1);
    }

    #[test]
    fn image_preview_targets_include_top_clipped_preview_rows() {
        let mut state = state_with_image_messages(1, &[1]);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        for _ in 0..4 {
            state.scroll_message_viewport_down();
            state.clamp_message_viewport_for_image_previews(200, 16, 3);
        }

        let targets = visible_image_preview_targets(&state, layout(2));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
        assert_eq!(targets[0].visible_preview_height, 2);
        assert_eq!(targets[0].top_clip_rows, 1);
    }

    #[test]
    fn image_preview_targets_skip_preview_when_no_preview_row_is_visible() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(5);

        let targets = visible_image_preview_targets(&state, layout(5));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_request_is_created_for_clipped_draw_target() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(6);
        let targets = visible_image_preview_targets(&state, layout(6));

        let requests = cache.next_requests(&targets);

        assert_eq!(requests.len(), 1);
        assert_eq!(cache.entries.len(), 1);
        assert!(requests.contains(&AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/image-1.png".to_owned(),
        }));
    }

    #[test]
    fn video_attachment_does_not_request_original_as_image_preview() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("clip".to_owned()),
            mentions: Vec::new(),
            attachments: vec![video_attachment(2)],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(6));

        assert!(targets.is_empty());
    }

    #[test]
    fn image_preview_targets_include_embed_thumbnail() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: vec![youtube_embed()],
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(8));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(
            targets[0].url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        );
        assert_eq!(targets[0].filename, "embed-thumbnail");
    }

    #[test]
    fn image_preview_targets_include_forwarded_image_attachments() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(2)],
        });

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(targets[0].url, "https://cdn.discordapp.com/image-2.png");
    }

    #[test]
    fn image_preview_targets_follow_the_scrolled_message_window() {
        let mut state = state_with_image_messages(8, &[1, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(7));

        assert!(target_message_ids(&targets).is_empty());
    }

    #[test]
    fn image_preview_targets_include_image_messages_in_scrolloff_context() {
        let mut state = state_with_image_messages(8, &[5, 6, 7]);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(14);
        while state.selected_message() > 3 {
            state.move_up();
        }
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(&state, layout(14));

        assert_eq!(target_message_ids(&targets), vec![Id::new(5)]);
    }

    #[test]
    fn image_preview_request_is_created_for_draw_target() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);

        assert!(cache.entries.is_empty());
        assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
        assert!(cache.entries.is_empty());

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert_eq!(
            requests,
            vec![AppCommand::LoadAttachmentPreview {
                url: target.url.clone()
            }]
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn image_preview_cache_evicts_least_recently_used_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing_targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64)
            .map(image_preview_target)
            .collect::<Vec<_>>();
        cache.next_requests(&existing_targets);
        cache.render_state(std::slice::from_ref(&existing_targets[0]));

        let new_target = image_preview_target(999);
        cache.next_requests(std::slice::from_ref(&new_target));

        assert_eq!(cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&existing_targets[0].key()));
        assert!(!cache.entries.contains_key(&existing_targets[1].key()));
        assert!(cache.entries.contains_key(&new_target.key()));
    }

    #[test]
    fn image_preview_cache_limits_visible_requests() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64 + 2)
            .map(image_preview_target)
            .collect::<Vec<_>>();

        let requests = cache.next_requests(&targets);

        assert_eq!(cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert_eq!(requests.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&targets[0].key()));
        assert!(
            !cache
                .entries
                .contains_key(&targets[MAX_IMAGE_PREVIEW_CACHE_ENTRIES].key())
        );
    }

    #[test]
    fn image_preview_store_loaded_preserves_existing_non_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing = image_preview_target(1).key();
        let loading = ImagePreviewTarget {
            message_id: Id::new(2),
            ..image_preview_target(1)
        }
        .key();
        cache.entries.insert(
            existing.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );
        cache.entries.insert(
            loading.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info: image_preview_target(1).preview_render_info(),
                last_used: 2,
            },
        );

        cache.store_loaded(&existing.url, &[]);

        assert!(matches!(
            cache.entries.get(&existing),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "existing failure"
        ));
        assert!(matches!(
            cache.entries.get(&loading),
            Some(ImagePreviewEntry::Failed { message, .. })
                if message == "inline preview unavailable in this terminal"
        ));
    }

    #[test]
    fn image_preview_loaded_bytes_start_decode_jobs_for_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);
        let key = target.key();
        let render_info = target.preview_render_info();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info,
                last_used: 1,
            },
        );

        let jobs = cache.decode_jobs_for_loaded_keys(vec![key.clone()], b"image bytes", (10, 20));

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].key, key);
        assert_eq!(jobs[0].generation, 1);
        assert_eq!(jobs[0].bytes.as_ref(), b"image bytes");
        assert_eq!(jobs[0].font_size, (10, 20));
        assert_eq!(jobs[0].render_info, render_info);
        assert!(matches!(
            cache.entries.get(&jobs[0].key),
            Some(ImagePreviewEntry::Decoding { filename, generation, .. })
                if filename == "loading.png" && *generation == 1
        ));
    }

    #[test]
    fn image_preview_store_decoded_records_decode_failure() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let key = image_preview_target(1).key();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Decoding {
                filename: "loading.png".to_owned(),
                generation: 1,
                last_used: 1,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: invalid image".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Failed { filename, message, .. })
                if filename == "loading.png" && message == "decode failed: invalid image"
        ));
    }

    #[test]
    fn image_preview_store_decoded_ignores_stale_results() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let key = image_preview_target(1).key();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: stale".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Failed { filename, message, .. })
                if filename == "existing.png" && message == "existing failure"
        ));
    }

    #[test]
    fn image_preview_store_decoded_ignores_replaced_decoding_generation() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let key = image_preview_target(1).key();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Decoding {
                filename: "newer.png".to_owned(),
                generation: 2,
                last_used: 2,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: old generation".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Decoding { filename, generation, .. })
                if filename == "newer.png" && *generation == 2
        ));
    }

    #[test]
    fn decode_preview_sized_image_reports_invalid_bytes() {
        let error = decode_preview_sized_image(
            b"not an image",
            (10, 20),
            image_preview_target(1).preview_render_info(),
        )
        .expect_err("invalid bytes should fail to decode");

        assert!(error.starts_with("decode failed:"));
    }

    #[test]
    fn image_preview_store_failed_preserves_existing_non_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing = image_preview_target(1).key();
        let loading = ImagePreviewTarget {
            message_id: Id::new(2),
            ..image_preview_target(1)
        }
        .key();
        cache.entries.insert(
            existing.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );
        cache.entries.insert(
            loading.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info: image_preview_target(1).preview_render_info(),
                last_used: 2,
            },
        );

        cache.store_failed(&existing.url, "new failure".to_owned());

        assert!(matches!(
            cache.entries.get(&existing),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "existing failure"
        ));
        assert!(matches!(
            cache.entries.get(&loading),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "new failure"
        ));
    }

    #[test]
    fn preview_sized_image_stays_within_preview_pixel_bounds() {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(400, 400, Rgba([0, 0, 0, 255])));
        let render_info = ImagePreviewRenderInfo {
            message_index: 0,
            preview_width: 16,
            preview_height: 3,
            visible_preview_height: 3,
            top_clip_rows: 0,
            accent_color: None,
        };

        let resized = preview_sized_image(&image, (10, 20), render_info)
            .expect("preview dimensions should produce resized image");

        assert!(resized.width() <= 160);
        assert!(resized.height() <= 60);
        assert!(resized.width() < image.width());
        assert!(resized.height() < image.height());
    }

    #[test]
    fn emoji_image_targets_include_visible_custom_reactions() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::GuildEmojisUpdate {
            guild_id: Id::new(1),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
        });
        state.focus_pane(FocusPane::Messages);
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        let targets = visible_emoji_image_targets(&state);

        assert_eq!(
            targets,
            vec![EmojiImageTarget {
                url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            }]
        );
    }

    #[test]
    fn emoji_image_request_is_created_for_visible_target() {
        let mut cache = EmojiImageCache::new();
        let target = EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        };

        if cache.picker.is_none() {
            return;
        }

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert_eq!(
            requests,
            vec![AppCommand::LoadAttachmentPreview {
                url: target.url.clone(),
            }]
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn emoji_image_cache_skips_requests_without_image_protocol() {
        let mut cache = EmojiImageCache {
            picker: None,
            entries: HashMap::new(),
        };
        let target = EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        };

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert!(requests.is_empty());
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn wide_image_preview_height_is_shorter_than_square_image() {
        let wide = image_preview_height_for_dimensions(60, 10, Some(2400), Some(600));
        let square = image_preview_height_for_dimensions(60, 10, Some(800), Some(800));

        assert!(wide < square);
        assert_eq!(wide, 5);
        assert_eq!(square, 10);
    }

    #[test]
    fn screenshot_like_wide_image_uses_compact_preview_height() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(481), Some(160)),
            6
        );
    }

    #[test]
    fn small_image_preview_height_does_not_upscale_to_full_width() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(100)),
            4
        );
    }

    #[test]
    fn tiny_image_preview_height_stays_compact_but_visible() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(32), Some(32)),
            3
        );
    }

    #[test]
    fn small_wide_image_preview_height_stays_compact() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(40)),
            3
        );
    }

    #[test]
    fn medium_small_square_image_preview_height_stays_below_max() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(128), Some(128)),
            5
        );
    }

    #[test]
    fn image_preview_height_falls_back_to_max_without_dimensions() {
        assert_eq!(image_preview_height_for_dimensions(60, 10, None, None), 10);
    }

    #[test]
    fn image_preview_height_falls_back_to_max_with_zero_dimensions() {
        assert_eq!(
            image_preview_height_for_dimensions(60, 10, Some(0), Some(100)),
            10
        );
    }

    fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: image_message_ids
                    .contains(&id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn state_with_avatar_messages(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: Some(format!("https://cdn.discordapp.com/avatar-{id}.png")),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn target_message_ids(targets: &[ImagePreviewTarget]) -> Vec<Id<MessageMarker>> {
        targets.iter().map(|target| target.message_id).collect()
    }

    fn image_preview_target(id: u64) -> ImagePreviewTarget {
        ImagePreviewTarget {
            message_index: 0,
            preview_width: 16,
            preview_height: 3,
            visible_preview_height: 3,
            top_clip_rows: 0,
            accent_color: None,
            message_id: Id::new(id),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            filename: format!("image-{id}.png"),
        }
    }

    fn image_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("image-{id}.png"),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }

    fn video_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("clip-{id}.mp4"),
            url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
            proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }

    fn youtube_embed() -> EmbedInfo {
        EmbedInfo {
            color: Some(0xff0000),
            provider_name: Some("YouTube".to_owned()),
            author_name: None,
            title: Some("Example Video".to_owned()),
            description: Some("A video description".to_owned()),
            fields: Vec::new(),
            footer_text: None,
            url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
            thumbnail_width: Some(480),
            thumbnail_height: Some(360),
            image_url: None,
            image_width: None,
            image_height: None,
            video_url: Some("https://www.youtube.com/embed/dQw4w9WgXcQ".to_owned()),
        }
    }

    fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(format!("forwarded {id}")),
            mentions: Vec::new(),
            attachments: vec![image_attachment(id)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }
}
