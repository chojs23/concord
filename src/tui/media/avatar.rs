use std::collections::{HashMap, HashSet};

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::Protocol};

use crate::{
    config::ImageProtocolPreference,
    discord::{AppCommand, AppEvent, ProfileAvatarUpload},
    tui::ui::AvatarImage,
};

use super::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, AvatarTarget, ImagePreviewRenderInfo,
    PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH, avatar_preview_url,
    cache::{MediaImageCacheCore, MediaImageCacheEntry},
    clipped_preview_protocol,
    decode::{MediaImageDecodeJob, MediaImageDecodeKey},
    query_image_picker,
};

/// Avatar images are small on screen but decoded originals can still add up
/// as users scroll through large servers. Keep a generous URL-keyed LRU cap.
pub(super) const MAX_AVATAR_IMAGE_CACHE_ENTRIES: usize = 32;

pub(in crate::tui) struct AvatarImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<String, AvatarImageEntry>,
    pub(super) active_popup_avatar_url: Option<String>,
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
        image: DynamicImage,
        protocols: HashMap<AvatarProtocolKey, AvatarProtocolEntry>,
        last_used: u64,
    },
    Failed {
        last_used: u64,
    },
}

pub(super) struct AvatarProtocolEntry {
    protocol: Protocol,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct AvatarProtocolKey {
    preview_width: u16,
    preview_height: u16,
    visible_preview_height: u16,
    top_clip_rows: u16,
    circular: bool,
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

    pub(super) fn profile_popup(circular: bool) -> Self {
        Self {
            preview_width: PROFILE_POPUP_AVATAR_WIDTH,
            preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            visible_preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            top_clip_rows: 0,
            circular,
        }
    }

    fn render_info(self) -> ImagePreviewRenderInfo {
        ImagePreviewRenderInfo {
            viewer: false,
            message_index: 0,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            accent_color: None,
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
    #[cfg(test)]
    pub(in crate::tui) fn new() -> Self {
        Self::new_with_protocol_preference(ImageProtocolPreference::Auto)
    }

    pub(in crate::tui) fn new_with_protocol_preference(
        protocol_preference: ImageProtocolPreference,
    ) -> Self {
        Self {
            picker: query_image_picker(
                "avatar",
                "avatar image picker unavailable",
                protocol_preference,
            ),
            cache: MediaImageCacheCore::new(),
            active_popup_avatar_url: None,
        }
    }

    pub(in crate::tui) fn render_state_with_popup(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
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
                    image, protocols, ..
                }) = self.cache.entries.get_mut(&url)
                else {
                    continue;
                };
                if !protocols.contains_key(&key)
                    && let Some(protocol) =
                        clipped_preview_protocol(picker, image, key.render_info())
                {
                    protocols.insert(key, AvatarProtocolEntry { protocol });
                }
            }

            if let Some(url) = popup_cache_url.as_deref()
                && let Some(AvatarImageEntry::Ready {
                    image, protocols, ..
                }) = self.cache.entries.get_mut(url)
            {
                let key = AvatarProtocolKey::profile_popup(circular);
                if !protocols.contains_key(&key)
                    && let Some(protocol) =
                        clipped_preview_protocol(picker, image, key.render_info())
                {
                    protocols.insert(key, AvatarProtocolEntry { protocol });
                }
            }
        }

        let avatars = targets
            .iter()
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let AvatarImageEntry::Ready { protocols, .. } = self.cache.entries.get(&url)?
                else {
                    return None;
                };
                let key = AvatarProtocolKey::message_avatar(target, circular);
                protocols.get(&key).map(|entry| AvatarImage {
                    row: target.row,
                    visible_height: target.visible_height,
                    protocol: &entry.protocol,
                })
            })
            .collect();
        let popup_avatar = popup_cache_url.and_then(|url| {
            let AvatarImageEntry::Ready { protocols, .. } = self.cache.entries.get(&url)? else {
                return None;
            };
            let key = AvatarProtocolKey::profile_popup(circular);
            protocols.get(&key).map(|entry| AvatarImage {
                row: 0,
                visible_height: PROFILE_POPUP_AVATAR_HEIGHT,
                protocol: &entry.protocol,
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

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) -> Option<MediaImageDecodeJob> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => {
                self.store_failed(url);
                None
            }
            _ => None,
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) -> Option<MediaImageDecodeJob> {
        self.cache.start_decode_job(
            url.to_owned(),
            std::sync::Arc::from(bytes.to_vec()),
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
        result: std::result::Result<DynamicImage, String>,
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
                        image,
                        protocols: HashMap::new(),
                        last_used,
                    },
                );
            }
            Err(_) => {
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

    pub(super) fn prune_to_limit(&mut self, targets: &[AvatarTarget]) {
        let protected = targets
            .iter()
            .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
            .map(|target| {
                avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT)
            })
            .chain(self.active_popup_avatar_url.iter().cloned())
            .collect::<HashSet<_>>();
        self.cache
            .prune_to_limit(MAX_AVATAR_IMAGE_CACHE_ENTRIES, |url| {
                protected.contains(url.as_str())
            });
    }
}
