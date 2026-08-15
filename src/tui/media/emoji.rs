use std::{collections::HashSet, time::Instant};

use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent},
    tui::{text::EmojiImageSize, ui::EmojiImage},
};

use super::{
    EmojiImageTarget,
    cache::{MediaImageCacheCore, MediaImageCacheEntry, RenderProtocolCache},
    decode::{DecodedMediaImage, MediaImageDecodeKey, MediaImageDecodeRequest},
    estimated_media_protocol_bytes, fixed_media_protocol_render_spec, picker_font_size,
    protocol_job::{MediaProtocolBuildJob, MediaProtocolBuildResult, MediaProtocolBuildTarget},
    work::{MediaWorkError, MediaWorkResult},
};

/// Cap on the URL-keyed emoji image cache. Animated entries retain decoded
/// frames, so the cache must stay bounded even though Discord emoji files are
/// small on the wire.
pub(super) const MAX_EMOJI_IMAGE_CACHE_ENTRIES: usize = 128;
const EMOJI_IMAGE_CACHE_DECODED_BYTE_BUDGET: u64 = 24 * 1024 * 1024;

pub(in crate::tui) struct EmojiImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<String, EmojiImageEntry>,
    pub(super) protocol_jobs: Vec<MediaProtocolBuildJob>,
}

pub(super) enum EmojiImageEntry {
    Loading {
        last_used: u64,
    },
    Decoding {
        generation: u64,
        last_used: u64,
    },
    Ready {
        generation: u64,
        image: DecodedMediaImage,
        protocols: Box<EmojiProtocolCaches>,
        last_used: u64,
    },
    Failed {
        last_used: u64,
    },
}

pub(super) struct EmojiProtocolCaches {
    pub(super) compact: RenderProtocolCache<usize>,
    pub(super) standalone: RenderProtocolCache<usize>,
}

impl EmojiProtocolCaches {
    fn new() -> Self {
        Self {
            compact: RenderProtocolCache::new(),
            standalone: RenderProtocolCache::new(),
        }
    }

    fn retained_bytes(&self) -> u64 {
        self.compact
            .retained_bytes()
            .saturating_add(self.standalone.retained_bytes())
    }
}

impl MediaImageCacheEntry for EmojiImageEntry {
    fn last_used(&self) -> u64 {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Decoding { last_used, .. }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used,
        }
    }

    fn decoded_image(&self) -> Option<&DecodedMediaImage> {
        match self {
            EmojiImageEntry::Ready { image, .. } => Some(image),
            EmojiImageEntry::Loading { .. }
            | EmojiImageEntry::Decoding { .. }
            | EmojiImageEntry::Failed { .. } => None,
        }
    }

    fn decoded_image_mut(&mut self) -> Option<&mut DecodedMediaImage> {
        match self {
            EmojiImageEntry::Ready { image, .. } => Some(image),
            EmojiImageEntry::Loading { .. }
            | EmojiImageEntry::Decoding { .. }
            | EmojiImageEntry::Failed { .. } => None,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Decoding { last_used, .. }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used = tick,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(self, EmojiImageEntry::Loading { .. })
    }

    fn is_failed(&self) -> bool {
        matches!(self, EmojiImageEntry::Failed { .. })
    }

    fn retained_protocol_bytes(&self) -> u64 {
        match self {
            EmojiImageEntry::Ready { protocols, .. } => protocols.retained_bytes(),
            _ => 0,
        }
    }

    fn decoding_generation(&self) -> Option<u64> {
        match self {
            EmojiImageEntry::Decoding { generation, .. } => Some(*generation),
            EmojiImageEntry::Loading { .. }
            | EmojiImageEntry::Ready { .. }
            | EmojiImageEntry::Failed { .. } => None,
        }
    }
}

impl EmojiImageCache {
    pub(in crate::tui) fn new(picker: Option<Picker>) -> Self {
        Self {
            picker,
            cache: MediaImageCacheCore::new(),
            protocol_jobs: Vec::new(),
        }
    }

    /// Returns decoded protocols for visible targets and refreshes their
    /// LRU timestamps so they survive the next pruning pass.
    pub(in crate::tui) fn render_state(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<EmojiImage<'_>> {
        for target in targets {
            let touch_tick = self.cache.next_tick();
            if let Some(entry) = self.cache.entries.get_mut(&target.url) {
                entry.touch(touch_tick);
                if let EmojiImageEntry::Ready {
                    generation,
                    image,
                    protocols,
                    ..
                } = entry
                    && let Some(picker) = self.picker.as_ref()
                {
                    let frame_index = image.current_frame_index();
                    if protocols.compact.request_build(&frame_index) {
                        self.protocol_jobs.push(MediaProtocolBuildJob::emoji(
                            target.url.clone(),
                            *generation,
                            frame_index,
                            EmojiImageSize::Compact,
                            picker.clone(),
                            image.current_frame_shared(),
                        ));
                    }
                    if target.image_size == EmojiImageSize::Standalone
                        && protocols.standalone.request_build(&frame_index)
                    {
                        self.protocol_jobs.push(MediaProtocolBuildJob::emoji(
                            target.url.clone(),
                            *generation,
                            frame_index,
                            EmojiImageSize::Standalone,
                            picker.clone(),
                            image.current_frame_shared(),
                        ));
                    }
                }
            }
        }
        targets
            .iter()
            .filter_map(|target| {
                let EmojiImageEntry::Ready {
                    image, protocols, ..
                } = self.cache.entries.get(&target.url)?
                else {
                    return None;
                };
                let frame_index = image.current_frame_index();
                let protocol = protocols.compact.get_or_last(&frame_index)?;
                let standalone_protocol = match target.image_size {
                    EmojiImageSize::Compact => None,
                    EmojiImageSize::Standalone => {
                        Some(protocols.standalone.get_or_last(&frame_index)?)
                    }
                };
                Some(EmojiImage {
                    url: target.url.clone(),
                    protocol,
                    standalone_protocol,
                })
            })
            .collect()
    }

    pub(in crate::tui) fn next_requests(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<AppCommand> {
        if self.picker.is_none() {
            return Vec::new();
        }

        let mut intents = Vec::new();
        for target in targets.iter().take(MAX_EMOJI_IMAGE_CACHE_ENTRIES) {
            if self
                .cache
                .insert_loading(target.url.clone(), |last_used| EmojiImageEntry::Loading {
                    last_used,
                })
            {
                intents.push(AppCommand::LoadAttachmentPreview {
                    url: target.url.clone(),
                });
            }
        }
        self.prune_to_limit(targets);
        intents
    }

    pub(in crate::tui) fn record_event(
        &mut self,
        event: &AppEvent,
    ) -> Option<MediaImageDecodeRequest> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, .. } => self.store_loaded(url),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => {
                self.store_failed(url);
                None
            }
            _ => None,
        }
    }

    /// Drops LRU entries while protecting URLs in the current frame's
    /// targets so a flood of unique ids can never evict what is on screen.
    pub(super) fn prune_to_limit(&mut self, targets: &[EmojiImageTarget]) {
        let protected: HashSet<&str> = targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .map(|target| target.url.as_str())
            .collect();
        self.cache.prune_to_limits(
            MAX_EMOJI_IMAGE_CACHE_ENTRIES,
            EMOJI_IMAGE_CACHE_DECODED_BYTE_BUDGET,
            |url| protected.contains(url.as_str()),
        );
    }

    fn store_loaded(&mut self, url: &str) -> Option<MediaImageDecodeRequest> {
        self.cache.start_decode_request(
            url.to_owned(),
            self.picker.is_some(),
            |generation, last_used| EmojiImageEntry::Decoding {
                generation,
                last_used,
            },
            |last_used| EmojiImageEntry::Failed { last_used },
            MediaImageDecodeKey::Emoji,
        )
    }

    pub(in crate::tui) fn store_decoded(
        &mut self,
        url: String,
        result_generation: u64,
        result: MediaWorkResult<DecodedMediaImage>,
    ) {
        if !self
            .cache
            .decoded_generation_matches(&url, result_generation)
        {
            return;
        }

        let last_used = self.cache.next_tick();
        match result {
            Ok(image) => {
                if self.picker.is_none() {
                    self.cache
                        .entries
                        .insert(url, EmojiImageEntry::Failed { last_used });
                    return;
                }
                self.cache.entries.insert(
                    url,
                    EmojiImageEntry::Ready {
                        generation: result_generation,
                        image,
                        protocols: Box::new(EmojiProtocolCaches::new()),
                        last_used,
                    },
                );
            }
            Err(MediaWorkError::Busy) => {
                self.cache.entries.remove(&url);
            }
            Err(MediaWorkError::Failed(_)) => {
                self.cache
                    .entries
                    .insert(url, EmojiImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        self.cache
            .store_failed_if_present(url.to_owned(), |last_used| EmojiImageEntry::Failed {
                last_used,
            });
    }

    pub(in crate::tui) fn sync_animation_visibility(
        &mut self,
        targets: &[EmojiImageTarget],
        now: Instant,
    ) {
        let visible = targets
            .iter()
            .map(|target| target.url.as_str())
            .collect::<HashSet<_>>();
        for (url, entry) in &mut self.cache.entries {
            let EmojiImageEntry::Ready {
                image, protocols, ..
            } = entry
            else {
                continue;
            };
            if visible.contains(url.as_str()) && !protocols.compact.is_empty() {
                image.start_animation(now);
            } else {
                image.pause_animation();
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

    pub(in crate::tui) fn store_protocol(&mut self, completed: MediaProtocolBuildResult) {
        let MediaProtocolBuildTarget::Emoji {
            url,
            frame_index,
            image_size,
        } = completed.target
        else {
            return;
        };
        // The cell box the protocol was rendered into; a few KB per protocol.
        let font_size = self.picker.as_ref().map_or((10, 20), picker_font_size);
        let protocol_bytes = estimated_media_protocol_bytes(
            fixed_media_protocol_render_spec(image_size.width(), image_size.height()),
            font_size,
        );
        let failed = match self.cache.entries.get_mut(&url) {
            Some(EmojiImageEntry::Ready {
                generation,
                protocols,
                ..
            }) if *generation == completed.generation => {
                let result = match image_size {
                    EmojiImageSize::Compact => {
                        protocols
                            .compact
                            .store_result(frame_index, completed.result, protocol_bytes)
                    }
                    EmojiImageSize::Standalone => {
                        protocols
                            .standalone
                            .store_result(frame_index, completed.result, protocol_bytes)
                    }
                };
                image_size == EmojiImageSize::Compact && result.is_err()
            }
            _ => false,
        };
        if failed {
            let last_used = self.cache.next_tick();
            self.cache
                .entries
                .insert(url, EmojiImageEntry::Failed { last_used });
        }
    }
}
