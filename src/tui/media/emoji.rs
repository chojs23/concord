use std::collections::{HashMap, HashSet};

use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::Protocol};

use crate::{
    discord::{AppCommand, AppEvent},
    tui::ui::EmojiImage,
};

use super::{EmojiImageTarget, emoji_protocol, lru, query_image_picker};

/// Cap on the URL-keyed emoji image cache. Each entry is a small terminal
/// protocol payload, so 256 or 128 fits realistic loads and bounds worst-case
/// memory if many unique emoji ids arrive.
pub(super) const MAX_EMOJI_IMAGE_CACHE_ENTRIES: usize = 128;

pub(in crate::tui) struct EmojiImageCache {
    pub(super) picker: Option<Picker>,
    pub(super) entries: HashMap<String, EmojiImageEntry>,
    pub(super) tick: u64,
    pub(super) protocol_generation: u64,
}

pub(super) enum EmojiImageEntry {
    Loading {
        last_used: u64,
    },
    Ready {
        image: DynamicImage,
        protocol: Protocol,
        protocol_generation: u64,
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

impl EmojiImageCache {
    pub(in crate::tui) fn new() -> Self {
        Self {
            picker: query_image_picker("emoji", "emoji image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
            protocol_generation: 0,
        }
    }

    pub(in crate::tui) fn refresh_protocols(&mut self) {
        self.protocol_generation = self.protocol_generation.saturating_add(1);
    }

    /// Returns decoded protocols for visible targets and refreshes their
    /// LRU timestamps so they survive the next pruning pass.
    pub(in crate::tui) fn render_state(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<EmojiImage<'_>> {
        let touch_tick = lru::next_tick(&mut self.tick);
        let picker = self.picker.clone();
        let protocol_generation = self.protocol_generation;
        for target in targets {
            if let Some(entry) = self.entries.get_mut(&target.url) {
                entry.touch(touch_tick);
                if let EmojiImageEntry::Ready {
                    image,
                    protocol,
                    protocol_generation: entry_protocol_generation,
                    ..
                } = entry
                    && *entry_protocol_generation != protocol_generation
                    && let Some(picker) = picker.as_ref()
                    && let Some(updated_protocol) = emoji_protocol(picker, image.clone())
                {
                    *protocol = updated_protocol;
                    *entry_protocol_generation = protocol_generation;
                }
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

        let mut intents = Vec::new();
        for target in targets.iter().take(MAX_EMOJI_IMAGE_CACHE_ENTRIES) {
            if self.entries.contains_key(&target.url) {
                continue;
            }

            let last_used = lru::next_tick(&mut self.tick);
            self.entries
                .insert(target.url.clone(), EmojiImageEntry::Loading { last_used });
            intents.push(AppCommand::LoadAttachmentPreview {
                url: target.url.clone(),
            });
        }
        self.prune_to_limit(targets);
        intents
    }

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, .. } => self.store_failed(url),
            _ => {}
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
        lru::prune_to_limit(
            &mut self.entries,
            MAX_EMOJI_IMAGE_CACHE_ENTRIES,
            |url| protected.contains(url.as_str()),
            EmojiImageEntry::last_used,
        );
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) {
        if !self.entries.contains_key(url) {
            return;
        }
        let last_used = lru::next_tick(&mut self.tick);

        let Some(picker) = self.picker.as_ref() else {
            self.entries
                .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
            return;
        };

        match image::load_from_memory(bytes) {
            Ok(img) => match emoji_protocol(picker, img.clone()) {
                Some(protocol) => {
                    self.entries.insert(
                        url.to_owned(),
                        EmojiImageEntry::Ready {
                            image: img,
                            protocol,
                            protocol_generation: self.protocol_generation,
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
            let last_used = lru::next_tick(&mut self.tick);
            self.entries
                .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
        }
    }
}
