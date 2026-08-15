use std::{collections::HashSet, time::Instant};

use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent, ProfileAvatarUpload},
    tui::ui::AvatarImage,
};

use super::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, AvatarTarget, MediaProtocolRenderSpec,
    PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH, avatar_preview_url,
    cache::{MediaImageCacheCore, MediaImageCacheEntry, RenderProtocolCache},
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

pub(super) enum AvatarImageEntry {
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
        protocols: Box<RenderProtocolCache<AvatarFrameProtocolKey>>,
        last_used: u64,
    },
    Failed {
        last_used: u64,
    },
}

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

impl MediaImageCacheEntry for AvatarImageEntry {
    fn last_used(&self) -> u64 {
        match self {
            AvatarImageEntry::Loading { last_used }
            | AvatarImageEntry::Decoding { last_used, .. }
            | AvatarImageEntry::Ready { last_used, .. }
            | AvatarImageEntry::Failed { last_used } => *last_used,
        }
    }

    fn decoded_image(&self) -> Option<&DecodedMediaImage> {
        match self {
            AvatarImageEntry::Ready { image, .. } => Some(image),
            AvatarImageEntry::Loading { .. }
            | AvatarImageEntry::Decoding { .. }
            | AvatarImageEntry::Failed { .. } => None,
        }
    }

    fn decoded_image_mut(&mut self) -> Option<&mut DecodedMediaImage> {
        match self {
            AvatarImageEntry::Ready { image, .. } => Some(image),
            AvatarImageEntry::Loading { .. }
            | AvatarImageEntry::Decoding { .. }
            | AvatarImageEntry::Failed { .. } => None,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            AvatarImageEntry::Loading { last_used }
            | AvatarImageEntry::Decoding { last_used, .. }
            | AvatarImageEntry::Ready { last_used, .. }
            | AvatarImageEntry::Failed { last_used } => *last_used = tick,
        }
    }

    fn is_loading(&self) -> bool {
        matches!(self, AvatarImageEntry::Loading { .. })
    }

    fn is_failed(&self) -> bool {
        matches!(self, AvatarImageEntry::Failed { .. })
    }

    fn retained_protocol_bytes(&self) -> u64 {
        match self {
            AvatarImageEntry::Ready { protocols, .. } => protocols.retained_bytes(),
            _ => 0,
        }
    }

    fn decoding_generation(&self) -> Option<u64> {
        match self {
            AvatarImageEntry::Decoding { generation, .. } => Some(*generation),
            AvatarImageEntry::Loading { .. }
            | AvatarImageEntry::Ready { .. }
            | AvatarImageEntry::Failed { .. } => None,
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

    pub(in crate::tui) fn render_state_with_popup(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
        popup_clip: Option<(u16, u16)>,
        circular: bool,
    ) -> (Vec<AvatarImage<'_>>, Option<AvatarImage<'_>>) {
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

        {
            let Some(picker) = self.picker.as_ref() else {
                return (Vec::new(), None);
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
        let intents = targets
            .iter()
            .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                self.next_request_for_cache_url(&url)
            })
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
        if self.cache.entries.contains_key(key) {
            return None;
        }
        let upload = upload()?;
        let last_used = self.cache.next_tick();
        self.cache
            .entries
            .insert(key.to_owned(), AvatarImageEntry::Loading { last_used });
        self.prune_to_limit(&[]);
        Some(AppCommand::LoadProfileAvatarPreview {
            key: key.to_owned(),
            upload,
        })
    }

    fn next_request_for_cache_url(&mut self, url: &str) -> Option<AppCommand> {
        if self
            .cache
            .insert_loading(url.to_owned(), |last_used| AvatarImageEntry::Loading {
                last_used,
            })
        {
            self.prune_to_limit(&[]);
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

    fn store_loaded(&mut self, url: &str) -> Option<MediaImageDecodeRequest> {
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
            Err(MediaWorkError::Busy) => {
                self.cache.entries.remove(&key);
            }
            Err(MediaWorkError::Failed(_)) => {
                self.cache
                    .entries
                    .insert(key, AvatarImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
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
        let MediaProtocolBuildTarget::Avatar { url, key } = completed.target else {
            return;
        };
        let font_size = self.picker.as_ref().map_or((10, 20), picker_font_size);
        let protocol_bytes = estimated_media_protocol_bytes(key.render_spec(), font_size);
        let failed = match self.cache.entries.get_mut(&url) {
            Some(AvatarImageEntry::Ready {
                generation,
                protocols,
                ..
            }) if *generation == completed.generation => protocols
                .store_result(key, completed.result, protocol_bytes)
                .is_err(),
            _ => false,
        };
        if failed {
            let last_used = self.cache.next_tick();
            self.cache
                .entries
                .insert(url, AvatarImageEntry::Failed { last_used });
        }
    }

    pub(super) fn prune_to_limit(&mut self, targets: &[AvatarTarget]) {
        let protected = targets
            .iter()
            .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
            .map(|target| {
                avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT)
            })
            .chain(self.active_popup_avatar_url.iter().cloned())
            .collect::<HashSet<_>>();
        self.cache.prune_to_limits(
            MAX_AVATAR_IMAGE_CACHE_ENTRIES,
            AVATAR_IMAGE_CACHE_DECODED_BYTE_BUDGET,
            |url| protected.contains(url.as_str()),
        );
    }
}
