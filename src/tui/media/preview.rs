use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use crate::discord::ids::{Id, marker::MessageMarker};
use ratatui_image::picker::Picker;

use crate::{
    config::AnimatePreviews,
    discord::{AppCommand, AppEvent},
    tui::ui::{ImagePreview, ImagePreviewState},
};

use super::{
    ImagePreviewTarget, MediaProtocolRenderSpec,
    cache::{MediaImageCacheCore, MediaImageCacheEntry, RenderProtocolCache},
    decode::{
        DecodedMediaImage, MediaImageDecodeCache, MediaImageDecodeKey, MediaImageDecodeRequest,
    },
    estimated_media_protocol_bytes, picker_font_size,
    protocol_job::{MediaProtocolBuildJob, MediaProtocolBuildResult},
    work::{MediaWorkError, MediaWorkResult},
};

pub(super) const MAX_IMAGE_PREVIEW_CACHE_ENTRIES: usize = 16;
const IMAGE_PREVIEW_CACHE_DECODED_BYTE_BUDGET: u64 = 64 * 1024 * 1024;
const ANIMATION_PROTOCOL_WINDOW_FRAMES: usize = 2;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct ImagePreviewKey {
    viewer: bool,
    message_id: Id<MessageMarker>,
    preview_index: usize,
    pub(super) url: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct ImagePreviewFragmentKey {
    preview: ImagePreviewKey,
    render_spec: MediaProtocolRenderSpec,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct PreviewFrameProtocolKey {
    render_spec: MediaProtocolRenderSpec,
    frame_index: usize,
}

pub(in crate::tui) struct ImagePreviewCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<ImagePreviewKey, ImagePreviewEntry>,
    pub(super) protocol_jobs: Vec<MediaProtocolBuildJob>,
    prepared_specs: HashMap<ImagePreviewKey, HashSet<MediaProtocolRenderSpec>>,
    protocol_failures: HashMap<(ImagePreviewKey, MediaProtocolRenderSpec), String>,
}

pub(super) enum ImagePreviewEntry {
    Loading {
        filename: String,
        last_used: u64,
    },
    Decoding {
        filename: String,
        generation: u64,
        last_used: u64,
    },
    Ready {
        filename: String,
        generation: u64,
        image: DecodedMediaImage,
        protocols: Box<RenderProtocolCache<PreviewFrameProtocolKey>>,
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
            prepared_specs: HashMap::new(),
            protocol_failures: HashMap::new(),
        }
    }

    pub(in crate::tui) fn prepare(&mut self, targets: &[ImagePreviewTarget]) {
        let picker = self.picker.clone();
        let font_size = picker.as_ref().map_or((10, 20), picker_font_size);
        self.prepared_specs.clear();
        let admitted = admitted_preview_keys(targets);
        for target in targets
            .iter()
            .filter(|target| admitted.contains(&target.key()))
        {
            let key = target.key();
            let render_spec = target.protocol_render_spec();
            self.prepared_specs
                .entry(key.clone())
                .or_default()
                .insert(render_spec);
            self.cache.touch(&key);
        }
        self.protocol_failures.retain(|(key, render_spec), _| {
            self.prepared_specs
                .get(key)
                .is_some_and(|specs| specs.contains(render_spec))
        });
        for (key, entry) in &mut self.cache.entries {
            let ImagePreviewEntry::Ready { protocols, .. } = entry else {
                continue;
            };
            let prepared_specs = self.prepared_specs.get(key);
            protocols.retain_failures(|protocol_key| {
                prepared_specs.is_some_and(|specs| specs.contains(&protocol_key.render_spec))
            });
        }

        let prepared_keys = self.prepared_specs.keys().cloned().collect::<Vec<_>>();
        for key in prepared_keys {
            let Some(ImagePreviewEntry::Ready {
                generation,
                image,
                protocols,
                ..
            }) = self.cache.entries.get_mut(&key)
            else {
                continue;
            };
            let Some(picker) = picker.as_ref() else {
                continue;
            };
            let specs = self
                .prepared_specs
                .get(&key)
                .expect("prepared preview key has render specs");
            let current_frame_index = image.current_frame_index();
            let mut current_missing = false;
            for render_spec in specs {
                let protocol_key = PreviewFrameProtocolKey {
                    render_spec: *render_spec,
                    frame_index: current_frame_index,
                };
                if protocols.get(&protocol_key).is_some()
                    || protocols.is_terminally_failed(&protocol_key)
                {
                    continue;
                }
                current_missing = true;
                if protocols.request_build(&protocol_key) {
                    self.protocol_jobs.push(MediaProtocolBuildJob::preview(
                        key.clone(),
                        *generation,
                        *render_spec,
                        current_frame_index,
                        picker.clone(),
                        image.frame_shared(current_frame_index),
                    ));
                }
                break;
            }
            if current_missing || image.frame_count() < ANIMATION_PROTOCOL_WINDOW_FRAMES {
                continue;
            }

            let window_bytes = specs.iter().fold(0u64, |bytes, render_spec| {
                bytes.saturating_add(
                    estimated_preview_protocol_bytes(*render_spec, font_size)
                        .saturating_mul(ANIMATION_PROTOCOL_WINDOW_FRAMES as u64),
                )
            });
            if specs.len().saturating_mul(ANIMATION_PROTOCOL_WINDOW_FRAMES) > 2
                && window_bytes > super::cache::RENDER_PROTOCOL_BYTE_BUDGET_PER_MEDIA_ENTRY
            {
                // Every visible crop must remain available together. Prefetching
                // an oversized second-frame window would evict those current
                // crops and rebuild them forever, so hold only this oversized
                // split animation still until a smaller window becomes visible.
                image.pause_animation();
                continue;
            }

            let next_frame_index = image.frame_index_with_offset(1);
            for render_spec in specs {
                let protocol_key = PreviewFrameProtocolKey {
                    render_spec: *render_spec,
                    frame_index: next_frame_index,
                };
                if protocols.get(&protocol_key).is_some()
                    || protocols.is_terminally_failed(&protocol_key)
                {
                    continue;
                }
                if protocols.request_build(&protocol_key) {
                    self.protocol_jobs.push(MediaProtocolBuildJob::preview(
                        key.clone(),
                        *generation,
                        *render_spec,
                        next_frame_index,
                        picker.clone(),
                        image.frame_shared(next_frame_index),
                    ));
                }
                break;
            }
        }
        self.prune_to_limit(targets);
    }

    pub(in crate::tui) fn reuse_cached_sources(
        &mut self,
        targets: &[ImagePreviewTarget],
        shared: &mut MediaImageDecodeCache,
    ) -> bool {
        let mut reused = false;
        let admitted = admitted_preview_keys(targets);
        for target in targets
            .iter()
            .filter(|target| admitted.contains(&target.key()))
        {
            let key = target.key();
            if self.cache.entries.contains_key(&key) {
                continue;
            }
            let image = shared
                .get(&target.url)
                .or_else(|| self.ready_image_for_url(&target.url));
            let Some(image) = image else {
                continue;
            };
            let last_used = self.cache.next_tick();
            let generation = self.cache.next_decode_generation();
            self.cache.entries.insert(
                key,
                ImagePreviewEntry::Ready {
                    filename: target.filename.clone(),
                    generation,
                    image,
                    protocols: Box::new(RenderProtocolCache::new()),
                    last_used,
                },
            );
            reused = true;
        }
        reused
    }

    pub(in crate::tui) fn render_state(
        &self,
        targets: &[ImagePreviewTarget],
    ) -> Vec<ImagePreview<'_>> {
        targets
            .iter()
            .map(|target| {
                let state = match self.cache.entries.get(&target.key()) {
                    Some(ImagePreviewEntry::Loading { filename, .. })
                    | Some(ImagePreviewEntry::Decoding { filename, .. }) => {
                        ImagePreviewState::Loading {
                            filename: filename.clone(),
                        }
                    }
                    Some(ImagePreviewEntry::Ready {
                        filename,
                        image,
                        protocols,
                        ..
                    }) => {
                        let current = PreviewFrameProtocolKey {
                            render_spec: target.protocol_render_spec(),
                            frame_index: image.current_frame_index(),
                        };
                        match protocols.get_or_last_matching(&current, |candidate| {
                            candidate.render_spec == current.render_spec
                        }) {
                            Some(protocol) => ImagePreviewState::Ready { protocol },
                            None => match self
                                .protocol_failures
                                .get(&(target.key(), current.render_spec))
                            {
                                Some(message) => ImagePreviewState::Failed {
                                    filename: filename.clone(),
                                    message: message.clone(),
                                },
                                None => ImagePreviewState::Loading {
                                    filename: filename.clone(),
                                },
                            },
                        }
                    }
                    Some(ImagePreviewEntry::Failed {
                        filename, message, ..
                    }) => ImagePreviewState::Failed {
                        filename: filename.clone(),
                        message: message.clone(),
                    },
                    None => ImagePreviewState::Loading {
                        filename: target.filename.clone(),
                    },
                };
                target.render(state)
            })
            .collect()
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
        let admitted = admitted_preview_keys(targets);
        let mut seen = HashSet::new();
        for target in targets.iter().filter(|target| {
            let key = target.key();
            admitted.contains(&key) && seen.insert(key)
        }) {
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
                    last_used,
                },
            );
            if requested_urls.insert(url.clone()) {
                intents.push(AppCommand::LoadAttachmentPreview { url });
            }
        }
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

    pub(in crate::tui) fn store_loaded(&mut self, url: &str) -> Vec<MediaImageDecodeRequest> {
        let keys = self.loading_keys_for_url(url);
        if keys.is_empty() {
            return Vec::new();
        }

        let Some(_) = self.picker.as_ref() else {
            for key in keys {
                let filename = self.filename_for_key(&key);
                let last_used = self.cache.next_tick();
                self.cache.entries.insert(
                    key.clone(),
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "inline preview unavailable in this terminal".to_owned(),
                        last_used,
                    },
                );
                self.protocol_failures
                    .retain(|(failed_key, _), _| failed_key != &key);
            }
            return Vec::new();
        };

        self.decode_requests_for_loaded_keys(keys)
    }

    pub(in crate::tui) fn ready_image_for_url(&self, url: &str) -> Option<DecodedMediaImage> {
        self.cache.entries.iter().find_map(|(key, entry)| {
            if key.url != url {
                return None;
            }
            match entry {
                ImagePreviewEntry::Ready { image, .. } => Some(image.fresh_playback()),
                _ => None,
            }
        })
    }

    pub(in crate::tui) fn defer_loading(&mut self, url: &str) {
        self.cache.entries.retain(|key, entry| {
            key.url != url || !matches!(entry, ImagePreviewEntry::Loading { .. })
        });
    }

    pub(in crate::tui) fn accepts_decode_request(
        &self,
        key: &ImagePreviewKey,
        generation: u64,
    ) -> bool {
        self.cache.decoded_generation_matches(key, generation)
    }

    fn decode_requests_for_loaded_keys(
        &mut self,
        keys: Vec<ImagePreviewKey>,
    ) -> Vec<MediaImageDecodeRequest> {
        let mut requests = Vec::new();
        for key in keys {
            let filename = self.filename_for_key(&key);
            let last_used = self.cache.next_tick();
            let generation = self.cache.next_decode_generation();
            self.cache.entries.insert(
                key.clone(),
                ImagePreviewEntry::Decoding {
                    filename,
                    generation,
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
        let Some(filename) = self.cache.entries.get(&key).and_then(|entry| {
            if let ImagePreviewEntry::Decoding { filename, .. } = entry {
                Some(filename.clone())
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
        self.protocol_failures
            .retain(|(failed_key, _), _| failed_key != &key);
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
                        protocols: Box::new(RenderProtocolCache::new()),
                        last_used,
                    },
                );
            }
            Err(MediaWorkError::Busy) => {
                // The shared decode cache keeps the downloaded bytes and retries
                // decoding, so keep this consumer attached to its generation.
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

    pub(in crate::tui) fn sync_animation_visibility(
        &mut self,
        targets: &[ImagePreviewTarget],
        now: Instant,
        animate: AnimatePreviews,
    ) {
        // Every animated frame costs a fresh protocol build, so the set that
        // keeps moving is the set the reader is actually looking at. Anything
        // left out holds the frame it stopped on.
        let visible = targets
            .iter()
            .filter(|target| match animate {
                AnimatePreviews::Always => true,
                AnimatePreviews::Selected => target.viewer || target.selected,
                AnimatePreviews::Never => false,
            })
            .map(|target| target.key())
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
                    self.prepared_specs.get(key).is_some_and(|specs| {
                        specs.iter().all(|render_spec| {
                            let protocol_key = PreviewFrameProtocolKey {
                                render_spec: *render_spec,
                                frame_index,
                            };
                            protocols.get(&protocol_key).is_some()
                                || protocols.is_terminally_failed(&protocol_key)
                        })
                    })
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
        self.protocol_failures.clear();
        for entry in self.cache.entries.values_mut() {
            if let ImagePreviewEntry::Ready { protocols, .. } = entry {
                protocols.forget_failures();
            }
        }
    }

    pub(in crate::tui) fn pause_animations(&mut self) {
        self.cache.pause_animations();
    }

    pub(in crate::tui) fn next_animation_deadline(&self) -> Option<Instant> {
        self.cache.next_animation_deadline()
    }

    pub(in crate::tui) fn next_retry_deadline(
        &self,
        targets: &[ImagePreviewTarget],
    ) -> Option<Instant> {
        self.picker.as_ref()?;
        admitted_preview_keys(targets)
            .iter()
            .filter_map(|key| self.cache.retry_deadline(key))
            .min()
    }

    pub(in crate::tui) fn advance_animations(&mut self, now: Instant) -> bool {
        let mut advanced = false;
        for (key, entry) in &mut self.cache.entries {
            let ImagePreviewEntry::Ready {
                image, protocols, ..
            } = entry
            else {
                continue;
            };
            let next_frame_index = image.frame_index_with_offset(1);
            let next_frame_ready = self.prepared_specs.get(key).is_none_or(|specs| {
                specs.iter().all(|render_spec| {
                    let protocol_key = PreviewFrameProtocolKey {
                        render_spec: *render_spec,
                        frame_index: next_frame_index,
                    };
                    protocols.get(&protocol_key).is_some()
                        || protocols.is_terminally_failed(&protocol_key)
                })
            });
            if image
                .next_frame_deadline()
                .is_some_and(|deadline| deadline <= now)
                && !next_frame_ready
            {
                // Protocol encoding can be slower than the source frame delay
                // for a large preview. Hold the current image until the worker
                // finishes instead of advancing into missing frames and
                // repeatedly drawing the last protocol as a fallback.
                image.pause_animation();
                continue;
            }
            advanced |= image.advance_frame(now);
        }
        advanced
    }

    pub(in crate::tui) fn take_protocol_jobs(&mut self) -> Vec<MediaProtocolBuildJob> {
        std::mem::take(&mut self.protocol_jobs)
    }

    fn estimated_protocol_bytes(&self, render_spec: MediaProtocolRenderSpec) -> u64 {
        let font_size = self.picker.as_ref().map_or((10, 20), picker_font_size);
        estimated_preview_protocol_bytes(render_spec, font_size)
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
        let protocol_key = PreviewFrameProtocolKey {
            render_spec,
            frame_index,
        };
        let failed_message = match &completed.result {
            Err(MediaWorkError::Failed(message)) => Some(message.clone()),
            Ok(_) | Err(MediaWorkError::Busy) => None,
        };
        let stored = match self.cache.entries.get_mut(&key) {
            Some(ImagePreviewEntry::Ready {
                generation,
                protocols,
                ..
            }) if *generation == completed.generation => {
                protocols.store_result(protocol_key, completed.result, protocol_bytes);
                Some((
                    protocols
                        .get_or_last_matching(&protocol_key, |candidate| {
                            candidate.render_spec == render_spec
                        })
                        .is_some(),
                    protocols.is_terminally_failed(&protocol_key),
                ))
            }
            _ => None,
        };
        let Some((has_matching_protocol, terminally_failed)) = stored else {
            return;
        };
        if has_matching_protocol {
            self.protocol_failures.remove(&(key, render_spec));
        } else if terminally_failed && let Some(message) = failed_message {
            self.protocol_failures.insert((key, render_spec), message);
        }
    }

    fn prune_to_limit(&mut self, targets: &[ImagePreviewTarget]) {
        let protected = admitted_preview_keys(targets);
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

fn admitted_preview_keys(targets: &[ImagePreviewTarget]) -> HashSet<ImagePreviewKey> {
    let mut admitted = HashSet::new();
    for target in targets {
        if admitted.len() >= MAX_IMAGE_PREVIEW_CACHE_ENTRIES {
            break;
        }
        admitted.insert(target.key());
    }
    admitted
}

fn estimated_preview_protocol_bytes(
    render_spec: MediaProtocolRenderSpec,
    font_size: (u16, u16),
) -> u64 {
    let visible_height = render_spec
        .visible_height
        .min(render_spec.height.saturating_sub(render_spec.top_clip_rows));
    estimated_media_protocol_bytes(
        MediaProtocolRenderSpec {
            height: visible_height,
            visible_height,
            top_clip_rows: 0,
            ..render_spec
        },
        font_size,
    )
}

impl ImagePreviewTarget {
    pub(in crate::tui) fn key(&self) -> ImagePreviewKey {
        ImagePreviewKey {
            viewer: self.viewer,
            message_id: self.message_id,
            preview_index: self.preview_index,
            url: self.url.clone(),
        }
    }

    pub(in crate::tui) fn fragment_key(&self) -> ImagePreviewFragmentKey {
        ImagePreviewFragmentKey {
            preview: self.key(),
            render_spec: self.protocol_render_spec(),
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
