use std::{collections::HashSet, time::Instant};

use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent},
    tui::{text::EmojiImageSize, ui::EmojiImage},
};

use super::{
    EmojiImageTarget,
    cache::{MediaCacheStats, MediaImageCacheCore, MediaImageCacheEntry, RenderProtocolCache},
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

    pub(in crate::tui) fn prepare(&mut self, targets: &[EmojiImageTarget]) {
        self.prune_to_limit(targets);
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
    }

    pub(in crate::tui) fn render_state(&self, targets: &[EmojiImageTarget]) -> Vec<EmojiImage<'_>> {
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

    pub(in crate::tui) fn accepts_decode_request(&self, url: &str, generation: u64) -> bool {
        self.cache
            .decoded_generation_matches(&url.to_owned(), generation)
    }

    pub(in crate::tui) fn defer_loading(&mut self, url: &str) {
        if self
            .cache
            .entries
            .get(url)
            .is_some_and(MediaImageCacheEntry::is_loading)
        {
            self.cache.entries.remove(url);
        }
    }

    pub(in crate::tui) fn retain_source_consumers(&mut self, targets: &[EmojiImageTarget]) {
        let protected = targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .map(|target| target.url.as_str())
            .collect::<HashSet<_>>();
        self.cache.entries.retain(|url, entry| {
            !matches!(
                entry,
                EmojiImageEntry::Loading { .. } | EmojiImageEntry::Decoding { .. }
            ) || protected.contains(url.as_str())
        });
    }

    pub(in crate::tui) fn reuse_cached_sources(
        &mut self,
        targets: &[EmojiImageTarget],
        mut lookup: impl FnMut(&str) -> Option<DecodedMediaImage>,
    ) -> bool {
        if self.picker.is_none() {
            return false;
        }
        let mut reused = false;
        for url in targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .map(|target| target.url.clone())
        {
            if matches!(
                self.cache.entries.get(&url),
                Some(EmojiImageEntry::Ready { .. })
            ) {
                continue;
            }
            let Some(image) = lookup(&url) else {
                continue;
            };
            let generation = self.cache.next_decode_generation();
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                url,
                EmojiImageEntry::Ready {
                    generation,
                    image,
                    protocols: Box::new(EmojiProtocolCaches::new()),
                    last_used,
                },
            );
            reused = true;
        }
        reused
    }

    pub(in crate::tui) fn store_loaded(&mut self, url: &str) -> Option<MediaImageDecodeRequest> {
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

    pub(in crate::tui) fn ready_image_for_url(&self, url: &str) -> Option<DecodedMediaImage> {
        let EmojiImageEntry::Ready { image, .. } = self.cache.entries.get(url)? else {
            return None;
        };
        Some(image.fresh_playback())
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
            Err(MediaWorkError::Busy) => {}
            Err(MediaWorkError::Failed(_)) => {
                self.cache
                    .entries
                    .insert(url, EmojiImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        // A cache hit may have replaced the placeholder while HTTP was in flight.
        if !self
            .cache
            .entries
            .get(url)
            .is_some_and(MediaImageCacheEntry::is_loading)
        {
            return;
        }
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

    pub(in crate::tui) fn diagnostics(&self) -> MediaCacheStats {
        self.cache.diagnostics(
            MAX_EMOJI_IMAGE_CACHE_ENTRIES,
            EMOJI_IMAGE_CACHE_DECODED_BYTE_BUDGET,
        )
    }

    pub(in crate::tui) fn next_retry_deadline(
        &self,
        targets: &[EmojiImageTarget],
    ) -> Option<Instant> {
        self.picker.as_ref()?;
        targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .filter_map(|target| self.cache.retry_deadline(&target.url))
            .min()
    }

    pub(in crate::tui) fn forget_failures(&mut self) {
        self.cache.forget_failures();
        for entry in self.cache.entries.values_mut() {
            if let EmojiImageEntry::Ready { protocols, .. } = entry {
                protocols.compact.forget_failures();
                protocols.standalone.forget_failures();
            }
        }
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
        if let Some(EmojiImageEntry::Ready {
            generation,
            protocols,
            ..
        }) = self.cache.entries.get_mut(&url)
            && *generation == completed.generation
        {
            // Compact and standalone renders fail independently. Neither can
            // invalidate the decoded source or the other size's protocols.
            match image_size {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_draw_does_not_change_cache_recency() {
        let mut cache = EmojiImageCache::new(Some(Picker::halfblocks()));
        let target = EmojiImageTarget {
            url: "emoji".to_owned(),
            image_size: EmojiImageSize::Compact,
        };
        cache.cache.entries.insert(
            target.url.clone(),
            EmojiImageEntry::Loading { last_used: 0 },
        );

        let _ = cache.render_state(std::slice::from_ref(&target));

        assert_eq!(cache.cache.tick, 0);
        assert!(cache.take_protocol_jobs().is_empty());
    }
}
