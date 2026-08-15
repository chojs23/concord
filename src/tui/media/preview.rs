use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use crate::discord::ids::{Id, marker::MessageMarker};
use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent},
    tui::ui::{ImagePreview, ImagePreviewState},
};

use super::{
    ImagePreviewTarget, MediaProtocolRenderSpec,
    cache::{MediaImageCacheCore, MediaImageCacheEntry, RenderProtocolCache},
    decode::{DecodedMediaImage, MediaImageDecodeKey, MediaImageDecodeRequest},
    estimated_media_protocol_bytes, picker_font_size,
    protocol_job::{MediaProtocolBuildJob, MediaProtocolBuildResult},
    work::{MediaWorkError, MediaWorkResult},
};

pub(super) const MAX_IMAGE_PREVIEW_CACHE_ENTRIES: usize = 16;
const IMAGE_PREVIEW_CACHE_DECODED_BYTE_BUDGET: u64 = 48 * 1024 * 1024;
const ANIMATION_PROTOCOL_WINDOW_FRAMES: usize = 2;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct ImagePreviewKey {
    viewer: bool,
    message_id: Id<MessageMarker>,
    preview_index: usize,
    preview_y_offset_rows: usize,
    visible_preview_height: u16,
    top_clip_rows: u16,
    pub(super) url: String,
}

pub(in crate::tui) struct ImagePreviewCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<ImagePreviewKey, ImagePreviewEntry>,
    pub(super) protocol_jobs: Vec<MediaProtocolBuildJob>,
}

pub(super) enum ImagePreviewEntry {
    Loading {
        filename: String,
        protocol_spec: MediaProtocolRenderSpec,
        last_used: u64,
    },
    Decoding {
        filename: String,
        generation: u64,
        protocol_spec: MediaProtocolRenderSpec,
        last_used: u64,
    },
    Ready {
        filename: String,
        generation: u64,
        image: DecodedMediaImage,
        protocol_spec: MediaProtocolRenderSpec,
        protocols: Box<RenderProtocolCache<usize>>,
        last_used: u64,
    },
    Failed {
        filename: String,
        message: String,
        last_used: u64,
    },
}

impl ImagePreviewCache {
    pub(in crate::tui) fn new(picker: Option<Picker>) -> Self {
        Self {
            picker,
            cache: MediaImageCacheCore::new(),
            protocol_jobs: Vec::new(),
        }
    }

    pub(in crate::tui) fn render_state(
        &mut self,
        targets: &[ImagePreviewTarget],
    ) -> Vec<ImagePreview<'_>> {
        self.prune_to_limit(targets);
        let picker = self.picker.clone();
        let target_by_key = targets
            .iter()
            .enumerate()
            .map(|(index, target)| (target.key(), (index, target)))
            .collect::<HashMap<_, _>>();
        let mut rendered_keys = HashSet::new();
        let mut previews = Vec::new();

        let mut tick = self.cache.tick;
        for (key, entry) in &mut self.cache.entries {
            let Some((order, target)) = target_by_key.get(key).copied() else {
                continue;
            };
            let render_spec = target.protocol_render_spec();
            rendered_keys.insert(key.clone());
            tick = tick.saturating_add(1);
            entry.touch(tick);
            let state = match entry {
                ImagePreviewEntry::Loading { filename, .. }
                | ImagePreviewEntry::Decoding { filename, .. } => ImagePreviewState::Loading {
                    filename: filename.clone(),
                },
                ImagePreviewEntry::Ready {
                    filename,
                    generation,
                    image,
                    protocol_spec,
                    protocols,
                    ..
                } => {
                    let current_frame_index = image.current_frame_index();
                    if *protocol_spec != render_spec {
                        **protocols = RenderProtocolCache::new();
                        *protocol_spec = render_spec;
                        image.pause_animation();
                    }
                    if let Some(picker) = picker.as_ref() {
                        // Build the current frame first, then keep one frame ahead.
                        // The existing single pending job bounds work when several
                        // animated previews are visible at once.
                        for frame_index in protocol_window_frame_indices(image) {
                            if protocols.get(&frame_index).is_some()
                                || protocols.is_terminally_failed(&frame_index)
                            {
                                continue;
                            }
                            if protocols.request_build(&frame_index) {
                                self.protocol_jobs.push(MediaProtocolBuildJob::preview(
                                    key.clone(),
                                    *generation,
                                    render_spec,
                                    frame_index,
                                    picker.clone(),
                                    image.frame_shared(frame_index),
                                ));
                            }
                            break;
                        }
                    }
                    match protocols.get_or_last(&current_frame_index) {
                        Some(protocol) => ImagePreviewState::Ready { protocol },
                        None => ImagePreviewState::Loading {
                            filename: filename.clone(),
                        },
                    }
                }
                ImagePreviewEntry::Failed {
                    filename, message, ..
                } => ImagePreviewState::Failed {
                    filename: filename.clone(),
                    message: message.clone(),
                },
            };
            previews.push((order, target.render(state)));
        }
        self.cache.tick = tick;

        for (order, target) in targets.iter().enumerate() {
            if !rendered_keys.contains(&target.key()) {
                previews.push((
                    order,
                    target.render(ImagePreviewState::Loading {
                        filename: target.filename.clone(),
                    }),
                ));
            }
        }

        previews.sort_by_key(|(order, _)| *order);
        previews.into_iter().map(|(_, preview)| preview).collect()
    }

    pub(in crate::tui) fn next_requests(
        &mut self,
        targets: &[ImagePreviewTarget],
    ) -> Vec<AppCommand> {
        let mut intents = Vec::new();
        let now = Instant::now();
        let mut requested_urls = self
            .cache
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, ImagePreviewEntry::Loading { .. }))
            .map(|(key, _)| key.url.clone())
            .collect::<HashSet<_>>();
        for target in targets.iter().take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES) {
            let key = target.key();
            if self.cache.entries.contains_key(&key) && !self.cache.take_due_retry(&key, now) {
                continue;
            }

            let url = target.url.clone();
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                key,
                ImagePreviewEntry::Loading {
                    filename: target.filename.clone(),
                    protocol_spec: target.protocol_render_spec(),
                    last_used,
                },
            );
            if requested_urls.insert(url.clone()) {
                intents.push(AppCommand::LoadAttachmentPreview { url });
            }
        }
        self.prune_to_limit(targets);
        intents
    }

    pub(in crate::tui) fn record_event(
        &mut self,
        event: &AppEvent,
    ) -> Vec<MediaImageDecodeRequest> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, .. } => self.store_loaded(url),
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                self.store_failed(url, message.clone());
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn store_loaded(&mut self, url: &str) -> Vec<MediaImageDecodeRequest> {
        let keys = self.loading_keys_for_url(url);
        if keys.is_empty() {
            return Vec::new();
        }

        let Some(_) = self.picker.as_ref() else {
            for key in keys {
                let filename = self.filename_for_key(&key);
                let last_used = self.cache.next_tick();
                self.cache.entries.insert(
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

        self.decode_requests_for_loaded_keys(keys)
    }

    fn decode_requests_for_loaded_keys(
        &mut self,
        keys: Vec<ImagePreviewKey>,
    ) -> Vec<MediaImageDecodeRequest> {
        let mut requests = Vec::new();
        for key in keys {
            let filename = self.filename_for_key(&key);
            let Some(protocol_spec) = self.protocol_spec_for_key(&key) else {
                let last_used = self.cache.next_tick();
                self.cache.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "preview dimensions unavailable".to_owned(),
                        last_used,
                    },
                );
                continue;
            };
            let last_used = self.cache.next_tick();
            let generation = self.cache.next_decode_generation();
            self.cache.entries.insert(
                key.clone(),
                ImagePreviewEntry::Decoding {
                    filename,
                    generation,
                    protocol_spec,
                    last_used,
                },
            );
            requests.push(MediaImageDecodeRequest {
                key: MediaImageDecodeKey::Preview(key),
                generation,
            });
        }
        requests
    }

    pub(in crate::tui) fn store_decoded(
        &mut self,
        key: ImagePreviewKey,
        result_generation: u64,
        result: MediaWorkResult<DecodedMediaImage>,
    ) {
        let Some((filename, protocol_spec)) = self.cache.entries.get(&key).and_then(|entry| {
            if let ImagePreviewEntry::Decoding {
                filename,
                protocol_spec,
                ..
            } = entry
            {
                Some((filename.clone(), *protocol_spec))
            } else {
                None
            }
        }) else {
            return;
        };

        if !self
            .cache
            .decoded_generation_matches(&key, result_generation)
        {
            return;
        }

        let last_used = self.cache.next_tick();
        match result {
            Ok(image) => {
                if self.picker.is_none() {
                    self.cache.entries.insert(
                        key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: "inline preview unavailable in this terminal".to_owned(),
                            last_used,
                        },
                    );
                    return;
                }
                self.cache.entries.insert(
                    key,
                    ImagePreviewEntry::Ready {
                        filename,
                        generation: result_generation,
                        image,
                        protocol_spec,
                        protocols: Box::new(RenderProtocolCache::new()),
                        last_used,
                    },
                );
            }
            Err(MediaWorkError::Busy) => {
                self.cache.entries.remove(&key);
            }
            Err(MediaWorkError::Failed(message)) => {
                self.cache.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message,
                        last_used,
                    },
                );
            }
        }
    }

    fn protocol_spec_for_key(&self, key: &ImagePreviewKey) -> Option<MediaProtocolRenderSpec> {
        match self.cache.entries.get(key)? {
            ImagePreviewEntry::Loading { protocol_spec, .. }
            | ImagePreviewEntry::Decoding { protocol_spec, .. } => Some(*protocol_spec),
            ImagePreviewEntry::Ready { .. } | ImagePreviewEntry::Failed { .. } => None,
        }
    }

    pub(in crate::tui) fn sync_animation_visibility(
        &mut self,
        targets: &[ImagePreviewTarget],
        now: Instant,
    ) {
        let visible = targets
            .iter()
            .map(ImagePreviewTarget::key)
            .collect::<HashSet<_>>();
        for (key, entry) in &mut self.cache.entries {
            let ImagePreviewEntry::Ready {
                image, protocols, ..
            } = entry
            else {
                continue;
            };
            if !visible.contains(key) {
                image.pause_animation();
                continue;
            }
            if image.next_frame_deadline().is_none()
                && protocol_window_frame_indices(image).all(|frame_index| {
                    protocols.get(&frame_index).is_some()
                        || protocols.is_terminally_failed(&frame_index)
                })
            {
                image.start_animation(now);
            }
        }
    }

    pub(in crate::tui) fn retained_stats(&self) -> (usize, u64, u64) {
        self.cache.retained_stats()
    }

    pub(in crate::tui) fn forget_failures(&mut self) {
        self.cache.forget_failures();
    }

    pub(in crate::tui) fn pause_animations(&mut self) {
        self.cache.pause_animations();
    }

    pub(in crate::tui) fn next_animation_deadline(&self) -> Option<Instant> {
        self.cache.next_animation_deadline()
    }

    pub(in crate::tui) fn advance_animations(&mut self, now: Instant) -> bool {
        self.cache.advance_animations(now)
    }

    pub(in crate::tui) fn take_protocol_jobs(&mut self) -> Vec<MediaProtocolBuildJob> {
        std::mem::take(&mut self.protocol_jobs)
    }

    fn estimated_protocol_bytes(&self, render_spec: MediaProtocolRenderSpec) -> u64 {
        let font_size = self.picker.as_ref().map_or((10, 20), picker_font_size);
        estimated_media_protocol_bytes(render_spec, font_size)
    }

    pub(in crate::tui) fn store_protocol(&mut self, completed: MediaProtocolBuildResult) {
        let super::protocol_job::MediaProtocolBuildTarget::Preview {
            key,
            render_spec,
            frame_index,
        } = completed.target
        else {
            return;
        };
        let protocol_bytes = self.estimated_protocol_bytes(render_spec);
        let failure = match self.cache.entries.get_mut(&key) {
            Some(ImagePreviewEntry::Ready {
                filename,
                generation,
                protocol_spec,
                protocols,
                ..
            }) if *generation == completed.generation && *protocol_spec == render_spec => protocols
                .store_result(frame_index, completed.result, protocol_bytes)
                .err()
                .map(|error| (filename.clone(), error)),
            _ => None,
        };
        if let Some((filename, message)) = failure {
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                key,
                ImagePreviewEntry::Failed {
                    filename,
                    message,
                    last_used,
                },
            );
        }
    }

    fn prune_to_limit(&mut self, targets: &[ImagePreviewTarget]) {
        let protected = targets
            .iter()
            .take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES)
            .map(ImagePreviewTarget::key)
            .collect::<HashSet<_>>();
        self.cache.prune_to_limits(
            MAX_IMAGE_PREVIEW_CACHE_ENTRIES,
            IMAGE_PREVIEW_CACHE_DECODED_BYTE_BUDGET,
            |key| protected.contains(key),
        );
    }

    pub(super) fn store_failed(&mut self, url: &str, message: String) {
        for key in self.loading_keys_for_url(url) {
            let filename = self.filename_for_key(&key);
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                key.clone(),
                ImagePreviewEntry::Failed {
                    filename,
                    message: message.clone(),
                    last_used,
                },
            );
            self.cache.note_failed_entry(key);
        }
    }

    fn loading_keys_for_url(&self, url: &str) -> Vec<ImagePreviewKey> {
        self.cache
            .entries
            .iter()
            .filter(|(key, entry)| {
                key.url == url && matches!(entry, ImagePreviewEntry::Loading { .. })
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn filename_for_key(&self, key: &ImagePreviewKey) -> String {
        self.cache
            .entries
            .get(key)
            .map(ImagePreviewEntry::filename)
            .unwrap_or("image")
            .to_owned()
    }
}

fn protocol_window_frame_indices(image: &DecodedMediaImage) -> impl Iterator<Item = usize> + '_ {
    (0..image.frame_count().min(ANIMATION_PROTOCOL_WINDOW_FRAMES))
        .map(|offset| image.frame_index_with_offset(offset))
}

impl ImagePreviewTarget {
    pub(in crate::tui) fn key(&self) -> ImagePreviewKey {
        ImagePreviewKey {
            viewer: self.viewer,
            message_id: self.message_id,
            preview_index: self.preview_index,
            // A thread-card target stores its absolute card row here for rendering.
            // Screen position must not split the decoded-image cache when the
            // same card moves because another thread was inserted or removed.
            preview_y_offset_rows: if self.thread_card {
                0
            } else {
                self.preview_y_offset_rows
            },
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            url: self.url.clone(),
        }
    }

    pub(super) fn protocol_render_spec(&self) -> MediaProtocolRenderSpec {
        MediaProtocolRenderSpec {
            width: self.preview_width,
            height: self.preview_height,
            visible_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            show_play_marker: self.show_play_marker,
            mask_circular: false,
        }
    }

    fn render<'a>(&self, state: ImagePreviewState<'a>) -> ImagePreview<'a> {
        ImagePreview {
            viewer: self.viewer,
            thread_card: self.thread_card,
            message_index: self.message_index,
            body_line_index: self.body_line_index,
            preview_x_offset_columns: self.preview_x_offset_columns,
            preview_y_offset_rows: self.preview_y_offset_rows,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
            visible_preview_height: self.visible_preview_height,
            accent_color: self.accent_color,
            state,
        }
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
}

impl MediaImageCacheEntry for ImagePreviewEntry {
    fn last_used(&self) -> u64 {
        match self {
            Self::Loading { last_used, .. }
            | Self::Decoding { last_used, .. }
            | Self::Ready { last_used, .. }
            | Self::Failed { last_used, .. } => *last_used,
        }
    }

    fn decoded_image(&self) -> Option<&DecodedMediaImage> {
        match self {
            Self::Ready { image, .. } => Some(image),
            Self::Loading { .. } | Self::Decoding { .. } | Self::Failed { .. } => None,
        }
    }

    fn decoded_image_mut(&mut self) -> Option<&mut DecodedMediaImage> {
        match self {
            Self::Ready { image, .. } => Some(image),
            Self::Loading { .. } | Self::Decoding { .. } | Self::Failed { .. } => None,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            ImagePreviewEntry::Loading { last_used, .. }
            | ImagePreviewEntry::Decoding { last_used, .. }
            | ImagePreviewEntry::Ready { last_used, .. }
            | ImagePreviewEntry::Failed { last_used, .. } => *last_used = tick,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(self, ImagePreviewEntry::Loading { .. })
    }

    fn is_failed(&self) -> bool {
        matches!(self, ImagePreviewEntry::Failed { .. })
    }

    fn retained_protocol_bytes(&self) -> u64 {
        match self {
            ImagePreviewEntry::Ready { protocols, .. } => protocols.retained_bytes(),
            _ => 0,
        }
    }

    fn decoding_generation(&self) -> Option<u64> {
        match self {
            ImagePreviewEntry::Decoding { generation, .. } => Some(*generation),
            ImagePreviewEntry::Loading { .. }
            | ImagePreviewEntry::Ready { .. }
            | ImagePreviewEntry::Failed { .. } => None,
        }
    }
}
