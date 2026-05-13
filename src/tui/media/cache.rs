use std::collections::{HashMap, HashSet};

use image::DynamicImage;
use ratatui_image::picker::Picker;

use crate::{
    discord::{AppCommand, AppEvent},
    tui::ui::{AvatarImage, EmojiImage},
};

use super::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, AvatarTarget, EmojiImageTarget,
    ImagePreviewRenderInfo, PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH,
    avatar_preview_url, clipped_preview_protocol, emoji_protocol, query_image_picker,
};

/// Avatar images are small on screen but decoded originals can still add up
/// as users scroll through large servers. Keep a generous URL-keyed LRU cap.
pub(super) const MAX_AVATAR_IMAGE_CACHE_ENTRIES: usize = 32;

pub(in crate::tui) struct AvatarImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) entries: HashMap<String, AvatarImageEntry>,
    pub(super) tick: u64,
}

pub(super) enum AvatarImageEntry {
    Loading { last_used: u64 },
    Ready { image: DynamicImage, last_used: u64 },
    Failed { last_used: u64 },
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

/// Cap on the URL-keyed emoji image cache. Each entry is a small terminal
/// protocol payload, so 256 or 128 fits realistic loads and bounds worst-case
/// memory if many unique emoji ids arrive.
pub(super) const MAX_EMOJI_IMAGE_CACHE_ENTRIES: usize = 128;

pub(in crate::tui) struct EmojiImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) entries: HashMap<String, EmojiImageEntry>,
    pub(super) tick: u64,
}

pub(super) enum EmojiImageEntry {
    Loading {
        last_used: u64,
    },
    Ready {
        protocol: ratatui_image::protocol::Protocol,
        last_used: u64,
    },
    Failed {
        last_used: u64,
    },
}

impl EmojiImageEntry {
    fn last_used(&self) -> u64 {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used,
        }
    }

    fn touch(&mut self, tick: u64) {
        match self {
            EmojiImageEntry::Loading { last_used }
            | EmojiImageEntry::Ready { last_used, .. }
            | EmojiImageEntry::Failed { last_used } => *last_used = tick,
        }
    }
}

impl AvatarImageCache {
    pub(in crate::tui) fn new() -> Self {
        Self {
            picker: query_image_picker("avatar", "avatar image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
        }
    }

    pub(in crate::tui) fn render_state(&mut self, targets: &[AvatarTarget]) -> Vec<AvatarImage> {
        let touch_tick = self.next_tick();
        for target in targets {
            let url = avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
            if let Some(entry) = self.entries.get_mut(&url) {
                entry.touch(touch_tick);
            }
        }
        let Some(picker) = self.picker.as_ref() else {
            return Vec::new();
        };

        targets
            .iter()
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                let AvatarImageEntry::Ready { image, .. } = self.entries.get(&url)? else {
                    return None;
                };
                let render_info = ImagePreviewRenderInfo {
                    viewer: false,
                    message_index: 0,
                    preview_x_offset_columns: 0,
                    preview_y_offset_rows: 0,
                    preview_width: AVATAR_PREVIEW_WIDTH,
                    preview_height: AVATAR_PREVIEW_HEIGHT,
                    preview_overflow_count: 0,
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

    pub(in crate::tui) fn next_requests(&mut self, targets: &[AvatarTarget]) -> Vec<AppCommand> {
        let commands = targets
            .iter()
            .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
            .filter_map(|target| {
                let url =
                    avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
                self.next_request_for_cache_url(&url)
            })
            .collect();
        self.prune_to_limit(targets);
        commands
    }

    /// Schedules an out-of-band avatar fetch (used by the profile popup,
    /// whose URL does not appear in the message-pane avatar targets).
    pub(in crate::tui) fn next_request_for_url(&mut self, url: &str) -> Option<AppCommand> {
        let url = avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT);
        self.next_request_for_cache_url(&url)
    }

    fn next_request_for_cache_url(&mut self, url: &str) -> Option<AppCommand> {
        if self.entries.contains_key(url) {
            return None;
        }
        let last_used = self.next_tick();
        self.entries
            .insert(url.to_owned(), AvatarImageEntry::Loading { last_used });
        self.prune_to_limit(&[]);
        Some(AppCommand::LoadAttachmentPreview {
            url: url.to_owned(),
        })
    }

    /// Renders a freshly sized protocol for the profile popup. Profile avatars
    /// use a larger CDN `size` than message-pane avatars, so they get a
    /// separate cache entry when the same user is opened in the popup.
    pub(in crate::tui) fn popup_avatar_image(&mut self, url: &str) -> Option<AvatarImage> {
        let url = avatar_preview_url(url, PROFILE_POPUP_AVATAR_WIDTH, PROFILE_POPUP_AVATAR_HEIGHT);
        let touch_tick = self.next_tick();
        self.entries.get_mut(&url)?.touch(touch_tick);
        let picker = self.picker.as_ref()?;
        let AvatarImageEntry::Ready { image, .. } = self.entries.get(&url)? else {
            return None;
        };
        let render_info = ImagePreviewRenderInfo {
            viewer: false,
            message_index: 0,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: PROFILE_POPUP_AVATAR_WIDTH,
            preview_height: PROFILE_POPUP_AVATAR_HEIGHT,
            preview_overflow_count: 0,
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
        let last_used = self.next_tick();

        if self.picker.is_none() {
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
            return;
        }

        match image::load_from_memory(bytes) {
            Ok(image) => {
                self.entries
                    .insert(url.to_owned(), AvatarImageEntry::Ready { image, last_used });
            }
            Err(_) => {
                self.entries
                    .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            let last_used = self.next_tick();
            self.entries
                .insert(url.to_owned(), AvatarImageEntry::Failed { last_used });
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    pub(super) fn prune_to_limit(&mut self, targets: &[AvatarTarget]) {
        if self.entries.len() <= MAX_AVATAR_IMAGE_CACHE_ENTRIES {
            return;
        }

        let protected = targets
            .iter()
            .take(MAX_AVATAR_IMAGE_CACHE_ENTRIES)
            .map(|target| {
                avatar_preview_url(&target.url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT)
            })
            .collect::<HashSet<_>>();
        let mut removable = self
            .entries
            .iter()
            .filter(|(url, _)| !protected.contains(url.as_str()))
            .map(|(url, entry)| (url.clone(), entry.last_used()))
            .collect::<Vec<_>>();
        removable.sort_by_key(|(_, last_used)| *last_used);

        for (url, _) in removable {
            if self.entries.len() <= MAX_AVATAR_IMAGE_CACHE_ENTRIES {
                break;
            }
            self.entries.remove(&url);
        }
    }
}

impl EmojiImageCache {
    pub(in crate::tui) fn new() -> Self {
        Self {
            picker: query_image_picker("emoji", "emoji image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
        }
    }

    /// Returns decoded protocols for visible targets and refreshes their
    /// LRU timestamps so they survive the next pruning pass.
    pub(in crate::tui) fn render_state(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<EmojiImage<'_>> {
        let touch_tick = self.next_tick();
        for target in targets {
            if let Some(entry) = self.entries.get_mut(&target.url) {
                entry.touch(touch_tick);
            }
        }
        targets
            .iter()
            .filter_map(|target| {
                let EmojiImageEntry::Ready { protocol, .. } = self.entries.get(&target.url)? else {
                    return None;
                };
                Some(EmojiImage {
                    url: target.url.clone(),
                    protocol,
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

        let mut commands = Vec::new();
        for target in targets.iter().take(MAX_EMOJI_IMAGE_CACHE_ENTRIES) {
            if self.entries.contains_key(&target.url) {
                continue;
            }

            let last_used = self.next_tick();
            self.entries
                .insert(target.url.clone(), EmojiImageEntry::Loading { last_used });
            commands.push(AppCommand::LoadAttachmentPreview {
                url: target.url.clone(),
            });
        }
        self.prune_to_limit(targets);
        commands
    }

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => self.store_failed(url),
            _ => {}
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// Drops LRU entries while protecting URLs in the current frame's
    /// targets so a flood of unique ids can never evict what is on screen.
    pub(super) fn prune_to_limit(&mut self, targets: &[EmojiImageTarget]) {
        if self.entries.len() <= MAX_EMOJI_IMAGE_CACHE_ENTRIES {
            return;
        }
        let protected: HashSet<&str> = targets
            .iter()
            .take(MAX_EMOJI_IMAGE_CACHE_ENTRIES)
            .map(|target| target.url.as_str())
            .collect();
        let mut removable: Vec<(String, u64)> = self
            .entries
            .iter()
            .filter(|(url, _)| !protected.contains(url.as_str()))
            .map(|(url, entry)| (url.clone(), entry.last_used()))
            .collect();
        removable.sort_by_key(|(_, last_used)| *last_used);
        for (url, _) in removable {
            if self.entries.len() <= MAX_EMOJI_IMAGE_CACHE_ENTRIES {
                break;
            }
            self.entries.remove(&url);
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) {
        if !self.entries.contains_key(url) {
            return;
        }
        let last_used = self.next_tick();

        let Some(picker) = self.picker.as_ref() else {
            self.entries
                .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
            return;
        };

        match image::load_from_memory(bytes) {
            Ok(img) => match emoji_protocol(picker, img) {
                Some(protocol) => {
                    self.entries.insert(
                        url.to_owned(),
                        EmojiImageEntry::Ready {
                            protocol,
                            last_used,
                        },
                    );
                }
                None => {
                    self.entries
                        .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
                }
            },
            Err(_) => {
                self.entries
                    .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
            }
        }
    }

    fn store_failed(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            let last_used = self.next_tick();
            self.entries
                .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
        }
    }
}
