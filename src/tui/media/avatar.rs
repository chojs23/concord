use std::collections::{HashMap, HashSet};

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::Protocol};

use crate::{
    discord::{AppCommand, AppEvent, ProfileAvatarUpload},
    tui::ui::AvatarImage,
};

use super::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, AvatarTarget, ImagePreviewRenderInfo,
    PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH, avatar_preview_url,
    clipped_preview_protocol, lru, query_image_picker,
};

/// Avatar images are small on screen but decoded originals can still add up
/// as users scroll through large servers. Keep a generous URL-keyed LRU cap.
pub(super) const MAX_AVATAR_IMAGE_CACHE_ENTRIES: usize = 32;

pub(in crate::tui) struct AvatarImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) entries: HashMap<String, AvatarImageEntry>,
    pub(super) active_popup_avatar_url: Option<String>,
    pub(super) tick: u64,
    pub(super) protocol_generation: u64,
}

pub(super) enum AvatarImageEntry {
    Loading {
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
    protocol_generation: u64,
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
            preview_overflow_count: 0,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            accent_color: None,
            show_play_marker: false,
            mask_circular: self.circular,
        }
    }
}

impl AvatarImageEntry {
    fn last_used(&self) -> u64 {
        match self {
            AvatarImageEntry::Loading { last_used }
            | AvatarImageEntry::Ready { last_used, .. }
            | AvatarImageEntry::Failed { last_used } => *last_used,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            AvatarImageEntry::Loading { last_used }
            | AvatarImageEntry::Ready { last_used, .. }
            | AvatarImageEntry::Failed { last_used } => *last_used = tick,
        }
    }
}

impl AvatarImageCache {
    pub(in crate::tui) fn new() -> Self {
        Self {
            picker: query_image_picker("avatar", "avatar image picker unavailable"),
            entries: HashMap::new(),
            active_popup_avatar_url: None,
            tick: 0,
            protocol_generation: 0,
        }
    }

    pub(in crate::tui) fn refresh_protocols(&mut self) {
        self.protocol_generation = self.protocol_generation.saturating_add(1);
    }

    pub(in crate::tui) fn render_state_with_popup(
        &mut self,
        targets: &[AvatarTarget],
        popup_url: Option<&str>,
        circular: bool,
    ) -> (Vec<AvatarImage<'_>>, Option<AvatarImage<'_>>) {
        let touch_tick = lru::next_tick(&mut self.tick);
        for target in targets {
            let url = avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
            if let Some(entry) = self.entries.get_mut(&url) {
                entry.touch(touch_tick);
            }
        }
        let popup_cache_url = popup_url.map(|url| {
            avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT)
        });
        self.active_popup_avatar_url = popup_cache_url.clone();
        if let Some(url) = popup_cache_url.as_deref()
            && let Some(entry) = self.entries.get_mut(url)
        {
            entry.touch(touch_tick);
        }

        {
            let Some(picker) = self.picker.as_ref() else {
                return (Vec::new(), None);
            };
            let protocol_generation = self.protocol_generation;

            for target in targets {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let key = AvatarProtocolKey::message_avatar(target, circular);
                let Some(AvatarImageEntry::Ready {
                    image, protocols, ..
                }) = self.entries.get_mut(&url)
                else {
                    continue;
                };
                if protocols
                    .get(&key)
                    .is_none_or(|entry| entry.protocol_generation != protocol_generation)
                    && let Some(protocol) =
                        clipped_preview_protocol(picker, image, key.render_info())
                {
                    protocols.insert(
                        key,
                        AvatarProtocolEntry {
                            protocol,
                            protocol_generation,
                        },
                    );
                }
            }

            if let Some(url) = popup_cache_url.as_deref()
                && let Some(AvatarImageEntry::Ready {
                    image, protocols, ..
                }) = self.entries.get_mut(url)
            {
                let key = AvatarProtocolKey::profile_popup(circular);
                if protocols
                    .get(&key)
                    .is_none_or(|entry| entry.protocol_generation != protocol_generation)
                    && let Some(protocol) =
                        clipped_preview_protocol(picker, image, key.render_info())
                {
                    protocols.insert(
                        key,
                        AvatarProtocolEntry {
                            protocol,
                            protocol_generation,
                        },
                    );
                }
            }
        }

        let avatars = targets
            .iter()
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let AvatarImageEntry::Ready { protocols, .. } = self.entries.get(&url)? else {
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
            let AvatarImageEntry::Ready { protocols, .. } = self.entries.get(&url)? else {
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
        if self.entries.contains_key(key) {
            return None;
        }
        let upload = upload()?;
        let last_used = lru::next_tick(&mut self.tick);
        self.entries
            .insert(key.to_owned(), AvatarImageEntry::Loading { last_used });
        self.prune_to_limit(&[]);
        Some(AppCommand::LoadProfileAvatarPreview {
            key: key.to_owned(),
            upload,
        })
    }

    fn next_request_for_cache_url(&mut self, url: &str) -> Option<AppCommand> {
        if self.entries.contains_key(url) {
            return None;
        }
        let last_used = lru::next_tick(&mut self.tick);
        self.entries
            .insert(url.to_owned(), AvatarImageEntry::Loading { last_used });
        self.prune_to_limit(&[]);
        Some(AppCommand::LoadAttachmentPreview {
            url: url.to_owned(),
        })
    }

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) {
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
        let last_used = lru::next_tick(&mut self.tick);

        if self.picker.is_none() {
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
            return;
        }

        match image::load_from_memory(bytes) {
            Ok(image) => {
                self.entries.insert(
                    url.to_owned(),
                    AvatarImageEntry::Ready {
                        image,
                        protocols: HashMap::new(),
                        last_used,
                    },
                );
            }
            Err(_) => {
                self.entries
                    .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            let last_used = lru::next_tick(&mut self.tick);
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
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
        lru::prune_to_limit(
            &mut self.entries,
            MAX_AVATAR_IMAGE_CACHE_ENTRIES,
            |url| protected.contains(url.as_str()),
            AvatarImageEntry::last_used,
        );
    }
}
