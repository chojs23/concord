use std::{collections::HashSet, time::Instant};

use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent, ProfileAvatarUpload},
    tui::ui::AvatarImage,
};

use super::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, AvatarTarget, MediaProtocolRenderSpec,
    PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH, avatar_preview_url,
    cache::{
        MediaCacheStats, MediaImageCacheCore, MediaImageCacheEntry, MediaImageEntry,
        RenderProtocolCache,
    },
    decode::{DecodedMediaImage, MediaImageDecodeKey, MediaImageDecodeRequest},
    estimated_media_protocol_bytes, picker_font_size,
    protocol_job::{MediaProtocolBuildJob, MediaProtocolBuildResult, MediaProtocolBuildTarget},
    work::{MediaWorkError, MediaWorkResult},
};

/// Avatar images are small on screen but decoded originals can still add up
/// as users scroll through large servers. Keep a generous URL-keyed LRU cap.
pub(super) const MAX_AVATAR_IMAGE_CACHE_ENTRIES: usize = 32;
const AVATAR_IMAGE_CACHE_DECODED_BYTE_BUDGET: u64 = 12 * 1024 * 1024;

pub(in crate::tui) struct AvatarImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<String, AvatarImageEntry>,
    pub(super) active_popup_avatar_url: Option<String>,
    pub(super) protocol_jobs: Vec<MediaProtocolBuildJob>,
}

pub(super) type AvatarImageEntry = MediaImageEntry<RenderProtocolCache<AvatarFrameProtocolKey>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct AvatarProtocolKey {
    preview_width: u16,
    preview_height: u16,
    visible_preview_height: u16,
    top_clip_rows: u16,
    circular: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct AvatarFrameProtocolKey {
    layout: AvatarProtocolKey,
    frame_index: usize,
}

impl AvatarFrameProtocolKey {
    pub(super) fn render_spec(self) -> MediaProtocolRenderSpec {
        self.layout.render_spec()
    }
}

impl AvatarProtocolKey {
    pub(super) fn message_avatar(target: &AvatarTarget, circular: bool) -> Self {
        Self {
            preview_width: AVATAR_PREVIEW_WIDTH,
            preview_height: AVATAR_PREVIEW_HEIGHT,
            visible_preview_height: target.visible_height,
            top_clip_rows: target.top_clip_rows,
            circular,
        }
    }

    pub(super) fn profile_popup(
        visible_preview_height: u16,
        top_clip_rows: u16,
        circular: bool,
    ) -> Self {
        Self {
            preview_width: PROFILE_POPUP_AVATAR_WIDTH,
            preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            visible_preview_height: visible_preview_height.min(PROFILE_POPUP_AVATAR_HEIGHT),
            top_clip_rows: top_clip_rows.min(PROFILE_POPUP_AVATAR_HEIGHT),
            circular,
        }
    }

    pub(super) fn render_spec(self) -> MediaProtocolRenderSpec {
        MediaProtocolRenderSpec {
            width: self.preview_width,
            height: self.preview_height,
            visible_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            show_play_marker: false,
            mask_circular: self.circular,
        }
    }
}

impl AvatarImageCache {
    pub(in crate::tui) fn new(picker: Option<Picker>) -> Self {
        Self {
            picker,
            cache: MediaImageCacheCore::new(),
            active_popup_avatar_url: None,
            protocol_jobs: Vec::new(),
        }
    }

    /// Protect the final frame's avatars and queue work before either draw pass.
    pub(in crate::tui) fn prepare(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
        popup_clip: Option<(u16, u16)>,
        circular: bool,
    ) {
        for target in targets {
            let url = avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
            self.cache.touch(&url);
        }
        let popup_cache_url = popup_url.map(|url| {
            avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
        });
        self.active_popup_avatar_url = popup_cache_url.clone();
        if let Some(url) = popup_cache_url.as_deref() {
            self.cache.touch(&url.to_owned());
        }
        self.prune_to_limit(targets);

        {
            let Some(picker) = self.picker.as_ref() else {
                return;
            };

            for target in targets {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let key = AvatarProtocolKey::message_avatar(target, circular);
                let Some(AvatarImageEntry::Ready {
                    generation,
                    image,
                    protocols,
                    ..
                }) = self.cache.entries.get_mut(&url)
                else {
                    continue;
                };
                let frame_key = AvatarFrameProtocolKey {
                    layout: key,
                    frame_index: image.current_frame_index(),
                };
                if protocols.request_build(&frame_key) {
                    self.protocol_jobs.push(MediaProtocolBuildJob::avatar(
                        url,
                        *generation,
                        frame_key,
                        picker.clone(),
                        image.current_frame_shared(),
                    ));
                }
            }

            if let (Some(url), Some((visible_height, top_clip_rows))) =
                (popup_cache_url.as_deref(), popup_clip)
                && let Some(AvatarImageEntry::Ready {
                    generation,
                    image,
                    protocols,
                    ..
                }) = self.cache.entries.get_mut(url)
            {
                let key = AvatarProtocolKey::profile_popup(visible_height, top_clip_rows, circular);
                let frame_key = AvatarFrameProtocolKey {
                    layout: key,
                    frame_index: image.current_frame_index(),
                };
                if protocols.request_build(&frame_key) {
                    self.protocol_jobs.push(MediaProtocolBuildJob::avatar(
                        url.to_owned(),
                        *generation,
                        frame_key,
                        picker.clone(),
                        image.current_frame_shared(),
                    ));
                }
            }
        }
    }

    pub(in crate::tui) fn render_state_with_popup(
        &self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
        popup_clip: Option<(u16, u16)>,
        circular: bool,
    ) -> (Vec<AvatarImage<'_>>, Option<AvatarImage<'_>>) {
        let popup_cache_url = popup_url.map(|url| {
            avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
        });
        let avatars = targets
            .iter()
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let AvatarImageEntry::Ready {
                    image, protocols, ..
                } = self.cache.entries.get(&url)?
                else {
                    return None;
                };
                let key = AvatarProtocolKey::message_avatar(target, circular);
                let frame_key = AvatarFrameProtocolKey {
                    layout: key,
                    frame_index: image.current_frame_index(),
                };
                protocols
                    .get_or_last_matching(&frame_key, |candidate| candidate.layout == key)
                    .map(|protocol| AvatarImage {
                        row: target.row,
                        visible_height: target.visible_height,
                        protocol,
                    })
            })
            .collect();
        let popup_avatar = popup_cache_url.and_then(|url| {
            let (visible_height, top_clip_rows) = popup_clip?;
            let AvatarImageEntry::Ready {
                image, protocols, ..
            } = self.cache.entries.get(&url)?
            else {
                return None;
            };
            let key = AvatarProtocolKey::profile_popup(visible_height, top_clip_rows, circular);
            let frame_key = AvatarFrameProtocolKey {
                layout: key,
                frame_index: image.current_frame_index(),
            };
            protocols
                .get_or_last_matching(&frame_key, |candidate| candidate.layout == key)
                .map(|protocol| AvatarImage {
                    row: 0,
                    visible_height,
                    protocol,
                })
        });

        (avatars, popup_avatar)
    }

    pub(in crate::tui) fn next_requests(&mut self, targets: &[AvatarTarget]) -> Vec<AppCommand> {
        let intents = admitted_avatar_urls(targets)
            .into_iter()
            .filter_map(|url| self.next_request_for_cache_url(&url))
            .collect();
        self.prune_to_limit(targets);
        intents
    }

    /// Schedules an out-of-band avatar fetch (used by the profile popup,
    /// whose URL does not appear in the message-pane avatar targets).
    pub(in crate::tui) fn next_request_for_url(&mut self, url: &str) -> Option<AppCommand> {
        let url = avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT);
        self.next_request_for_cache_url(&url)
    }

    pub(in crate::tui) fn next_request_for_profile_upload(
        &mut self,
        key: &str,
        upload: impl FnOnce() -> Option<ProfileAvatarUpload>,
    ) -> Option<AppCommand> {
        let key = key.to_owned();
        if !self.cache.can_insert_loading(&key, Instant::now()) {
            return None;
        }
        let upload = upload()?;
        if !self
            .cache
            .insert_loading(key.clone(), |last_used| AvatarImageEntry::Loading {
                last_used,
            })
        {
            return None;
        }
        self.prune_to_limit(&[]);
        Some(AppCommand::LoadProfileAvatarPreview { key, upload })
    }

    fn next_request_for_cache_url(&mut self, url: &str) -> Option<AppCommand> {
        if self
            .cache
            .insert_loading(url.to_owned(), |last_used| AvatarImageEntry::Loading {
                last_used,
            })
        {
            return Some(AppCommand::LoadAttachmentPreview {
                url: url.to_owned(),
            });
        }
        None
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

    pub(in crate::tui) fn visible_source_urls(&self, targets: &[AvatarTarget]) -> Vec<String> {
        targets
            .iter()
            .map(|target| {
                avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT)
            })
            .chain(self.active_popup_avatar_url.iter().cloned())
            .collect()
    }

    pub(in crate::tui) fn retain_source_consumers(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
    ) {
        let protected = admitted_avatar_urls(targets)
            .into_iter()
            .chain(popup_url.map(|url| {
                avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
            }))
            .collect::<HashSet<_>>();
        self.cache.entries.retain(|url, entry| {
            !matches!(
                entry,
                AvatarImageEntry::Loading { .. } | AvatarImageEntry::Decoding { .. }
            ) || protected.contains(url)
        });
    }

    pub(in crate::tui) fn reuse_cached_sources(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
        mut lookup: impl FnMut(&str) -> Option<DecodedMediaImage>,
    ) -> bool {
        let mut reused = false;
        let urls = admitted_avatar_urls(targets)
            .into_iter()
            .chain(popup_url.map(|url| {
                avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
            }));
        for url in urls {
            if matches!(
                self.cache.entries.get(&url),
                Some(AvatarImageEntry::Ready { .. })
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
                AvatarImageEntry::Ready {
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

    pub(in crate::tui) fn store_loaded(&mut self, url: &str) -> Option<MediaImageDecodeRequest> {
        self.cache.start_decode_request(
            url.to_owned(),
            self.picker.is_some(),
            |generation, last_used| AvatarImageEntry::Decoding {
                generation,
                last_used,
            },
            |last_used| AvatarImageEntry::Failed { last_used },
            MediaImageDecodeKey::Avatar,
        )
    }

    pub(in crate::tui) fn ready_image_for_url(&self, url: &str) -> Option<DecodedMediaImage> {
        let AvatarImageEntry::Ready { image, .. } = self.cache.entries.get(url)? else {
            return None;
        };
        Some(image.fresh_playback())
    }

    pub(in crate::tui) fn store_decoded(
        &mut self,
        key: String,
        result_generation: u64,
        result: MediaWorkResult<DecodedMediaImage>,
    ) {
        if !self
            .cache
            .decoded_generation_matches(&key, result_generation)
        {
            return;
        }

        let last_used = self.cache.next_tick();
        match result {
            Ok(image) => {
                self.cache.entries.insert(
                    key,
                    AvatarImageEntry::Ready {
                        generation: result_generation,
                        image,
                        protocols: Box::new(RenderProtocolCache::new()),
                        last_used,
                    },
                );
            }
            Err(MediaWorkError::Busy) => {}
            Err(MediaWorkError::Failed(_)) => {
                self.cache
                    .entries
                    .insert(key, AvatarImageEntry::Failed { last_used });
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
            .store_failed_if_present(url.to_owned(), |last_used| AvatarImageEntry::Failed {
                last_used,
            });
    }

    pub(in crate::tui) fn sync_animation_visibility(
        &mut self,
        targets: &[AvatarTarget],
        now: Instant,
    ) {
        let visible = targets
            .iter()
            .map(|target| {
                avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT)
            })
            .chain(self.active_popup_avatar_url.iter().cloned())
            .collect::<HashSet<_>>();
        for (url, entry) in &mut self.cache.entries {
            let AvatarImageEntry::Ready {
                image, protocols, ..
            } = entry
            else {
                continue;
            };
            if visible.contains(url) && !protocols.is_empty() {
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
            MAX_AVATAR_IMAGE_CACHE_ENTRIES,
            AVATAR_IMAGE_CACHE_DECODED_BYTE_BUDGET,
        )
    }

    pub(in crate::tui) fn next_retry_deadline(
        &self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
    ) -> Option<Instant> {
        self.picker.as_ref()?;
        admitted_avatar_urls(targets)
            .into_iter()
            .chain(popup_url.map(|url| {
                avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
            }))
            .filter_map(|url| self.cache.retry_deadline(&url))
            .min()
    }

    pub(in crate::tui) fn forget_failures(&mut self) {
        self.cache.forget_failures();
        for entry in self.cache.entries.values_mut() {
            if let AvatarImageEntry::Ready { protocols, .. } = entry {
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

    pub(in crate::tui) fn advance_animations(&mut self, now: Instant) -> bool {
        self.cache.advance_animations(now)
    }

    pub(in crate::tui) fn take_protocol_jobs(&mut self) -> Vec<MediaProtocolBuildJob> {
        std::mem::take(&mut self.protocol_jobs)
    }

    pub(in crate::tui) fn store_protocol(&mut self, completed: MediaProtocolBuildResult) {
        let MediaProtocolBuildTarget::Avatar { url, key } = completed.target else {
            return;
        };
        let font_size = self.picker.as_ref().map_or((10, 20), picker_font_size);
        let protocol_bytes = estimated_media_protocol_bytes(key.render_spec(), font_size);
        if let Some(AvatarImageEntry::Ready {
            generation,
            protocols,
            ..
        }) = self.cache.entries.get_mut(&url)
            && *generation == completed.generation
        {
            // A render failure belongs to this exact layout and frame. The
            // decoded source and protocols for other layouts remain valid.
            protocols.store_result(key, completed.result, protocol_bytes);
        }
    }

    pub(super) fn prune_to_limit(&mut self, targets: &[AvatarTarget]) {
        let protected = admitted_avatar_urls(targets)
            .into_iter()
            .chain(self.active_popup_avatar_url.iter().cloned())
            .collect::<HashSet<_>>();
        self.cache.prune_to_limits(
            MAX_AVATAR_IMAGE_CACHE_ENTRIES,
            AVATAR_IMAGE_CACHE_DECODED_BYTE_BUDGET,
            |url| protected.contains(url.as_str()),
        );
    }
}

fn admitted_avatar_urls(targets: &[AvatarTarget]) -> Vec<String> {
    let mut seen = HashSet::new();
    targets
        .iter()
        .map(|target| avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT))
        .filter(|url| seen.insert(url.clone()))
        .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_draw_does_not_change_final_frame_cache_protection() {
        let mut cache = AvatarImageCache::new(Some(Picker::halfblocks()));
        cache.active_popup_avatar_url = Some("popup-avatar".to_owned());
        let tick = cache.cache.tick;

        let _ = cache.render_state_with_popup(&[], None, None, false);

        assert_eq!(
            cache.active_popup_avatar_url.as_deref(),
            Some("popup-avatar")
        );
        assert_eq!(cache.cache.tick, tick);
        assert!(cache.take_protocol_jobs().is_empty());
    }
}
