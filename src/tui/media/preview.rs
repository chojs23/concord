use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::config::ImageProtocolPreference;
use crate::discord::ids::{Id, marker::MessageMarker};
use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

use crate::{
    discord::{AppCommand, AppEvent},
    tui::ui::{ImagePreview, ImagePreviewState},
};

use super::{
    ImagePreviewRenderInfo, ImagePreviewTarget,
    cache::{MediaImageCacheCore, MediaImageCacheEntry},
    clipped_preview_image,
    decode::{MediaImageDecodeJob, MediaImageDecodeKey},
    picker_font_size, query_image_picker,
};

pub(super) const MAX_IMAGE_PREVIEW_CACHE_ENTRIES: usize = 16;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct ImagePreviewKey {
    viewer: bool,
    message_id: Id<MessageMarker>,
    preview_index: usize,
    pub(super) url: String,
}

pub(in crate::tui) struct ImagePreviewCache {
    pub(super) picker: Option<Picker>,
    pub(super) cache: MediaImageCacheCore<ImagePreviewKey, ImagePreviewEntry>,
}

pub(super) enum ImagePreviewEntry {
    Loading {
        filename: String,
        render_info: ImagePreviewRenderInfo,
        last_used: u64,
    },
    Decoding {
        filename: String,
        generation: u64,
        render_info: ImagePreviewRenderInfo,
        last_used: u64,
    },
    Ready {
        filename: String,
        image: DynamicImage,
        protocol_render_info: ImagePreviewRenderInfo,
        protocol: Box<StatefulProtocol>,
        last_used: u64,
    },
    Failed {
        filename: String,
        message: String,
        last_used: u64,
    },
}

impl ImagePreviewCache {
    #[cfg(test)]
    pub(in crate::tui) fn new() -> Self {
        Self::new_with_protocol_preference(ImageProtocolPreference::Auto)
    }

    pub(in crate::tui) fn new_with_protocol_preference(
        protocol_preference: ImageProtocolPreference,
    ) -> Self {
        Self {
            picker: query_image_picker(
                "preview",
                "inline image picker unavailable",
                protocol_preference,
            ),
            cache: MediaImageCacheCore::new(),
        }
    }

    pub(in crate::tui) fn font_size(&self) -> Option<(u16, u16)> {
        self.picker.as_ref().map(picker_font_size)
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
            .map(|(index, target)| (target.key(), (index, target.preview_render_info())))
            .collect::<HashMap<_, _>>();
        let mut rendered_keys = HashSet::new();
        let mut previews = Vec::new();

        let mut tick = self.cache.tick;
        for (key, entry) in &mut self.cache.entries {
            let Some((order, render_info)) = target_by_key.get(key).copied() else {
                continue;
            };
            rendered_keys.insert(key.clone());
            tick = tick.saturating_add(1);
            entry.touch(tick);
            let state = match entry {
                ImagePreviewEntry::Loading { filename, .. }
                | ImagePreviewEntry::Decoding { filename, .. } => ImagePreviewState::Loading {
                    filename: filename.clone(),
                },
                ImagePreviewEntry::Ready {
                    image,
                    protocol,
                    protocol_render_info,
                    ..
                } => {
                    if *protocol_render_info != render_info
                        && let Some(picker) = picker.as_ref()
                        && let Some(updated_protocol) =
                            clipped_preview_stateful_protocol(picker, image, render_info)
                    {
                        *protocol = updated_protocol;
                        *protocol_render_info = render_info;
                    }
                    ImagePreviewState::Ready {
                        protocol: protocol.as_mut(),
                    }
                }
                ImagePreviewEntry::Failed {
                    filename, message, ..
                } => ImagePreviewState::Failed {
                    filename: filename.clone(),
                    message: message.clone(),
                },
            };
            previews.push((
                order,
                ImagePreview {
                    viewer: render_info.viewer,
                    message_index: render_info.message_index,
                    preview_x_offset_columns: render_info.preview_x_offset_columns,
                    preview_y_offset_rows: render_info.preview_y_offset_rows,
                    preview_width: render_info.preview_width,
                    preview_height: render_info.preview_height,
                    accent_color: render_info.accent_color,
                    content_hash: crate::tui::runtime::image_layer::content_hash(&(
                        &key.url,
                        render_info,
                    )),
                    state,
                },
            ));
        }
        self.cache.tick = tick;

        for (order, target) in targets.iter().enumerate() {
            if !rendered_keys.contains(&target.key()) {
                previews.push((
                    order,
                    ImagePreview {
                        viewer: target.viewer,
                        message_index: target.message_index,
                        preview_x_offset_columns: target.preview_x_offset_columns,
                        preview_y_offset_rows: target.preview_y_offset_rows,
                        preview_width: target.preview_width,
                        preview_height: target.preview_height,
                        accent_color: target.accent_color,
                        // Placeholder renders as text, so its fingerprint is unused.
                        content_hash: 0,
                        state: ImagePreviewState::Loading {
                            filename: target.filename.clone(),
                        },
                    },
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
        let mut requested_urls = self
            .cache
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, ImagePreviewEntry::Loading { .. }))
            .map(|(key, _)| key.url.clone())
            .collect::<HashSet<_>>();
        for target in targets.iter().take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES) {
            let key = target.key();
            if self.cache.entries.contains_key(&key) {
                continue;
            }

            let url = target.url.clone();
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                key,
                ImagePreviewEntry::Loading {
                    filename: target.filename.clone(),
                    render_info: target.preview_render_info(),
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

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) -> Vec<MediaImageDecodeJob> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                self.store_failed(url, message.clone());
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn store_loaded(&mut self, url: &str, bytes: &[u8]) -> Vec<MediaImageDecodeJob> {
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

        self.decode_jobs_for_loaded_keys(keys, bytes)
    }

    pub(super) fn decode_jobs_for_loaded_keys(
        &mut self,
        keys: Vec<ImagePreviewKey>,
        bytes: &[u8],
    ) -> Vec<MediaImageDecodeJob> {
        let bytes: Arc<[u8]> = Arc::from(bytes.to_vec());
        let mut jobs = Vec::new();
        for key in keys {
            let filename = self.filename_for_key(&key);
            let Some(render_info) = self.render_info_for_key(&key) else {
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
                    render_info,
                    last_used,
                },
            );
            jobs.push(MediaImageDecodeJob {
                key: MediaImageDecodeKey::Preview(key),
                generation,
                bytes: bytes.clone(),
            });
        }
        jobs
    }

    pub(in crate::tui) fn store_decoded(
        &mut self,
        key: ImagePreviewKey,
        result_generation: u64,
        result: std::result::Result<DynamicImage, String>,
    ) {
        let Some((filename, render_info)) = self.cache.entries.get(&key).and_then(|entry| {
            if let ImagePreviewEntry::Decoding {
                filename,
                render_info,
                ..
            } = entry
            {
                Some((filename.clone(), *render_info))
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
                let Some(picker) = self.picker.as_ref() else {
                    self.cache.entries.insert(
                        key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: "inline preview unavailable in this terminal".to_owned(),
                            last_used,
                        },
                    );
                    return;
                };
                let Some(protocol) = clipped_preview_stateful_protocol(picker, &image, render_info)
                else {
                    self.cache.entries.insert(
                        key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: "inline preview dimensions unavailable".to_owned(),
                            last_used,
                        },
                    );
                    return;
                };
                self.cache.entries.insert(
                    key,
                    ImagePreviewEntry::Ready {
                        filename,
                        image,
                        protocol_render_info: render_info,
                        protocol,
                        last_used,
                    },
                );
            }
            Err(message) => {
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

    fn render_info_for_key(&self, key: &ImagePreviewKey) -> Option<ImagePreviewRenderInfo> {
        match self.cache.entries.get(key)? {
            ImagePreviewEntry::Loading { render_info, .. }
            | ImagePreviewEntry::Decoding { render_info, .. } => Some(*render_info),
            ImagePreviewEntry::Ready { .. } | ImagePreviewEntry::Failed { .. } => None,
        }
    }

    fn prune_to_limit(&mut self, targets: &[ImagePreviewTarget]) {
        let protected = targets
            .iter()
            .take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES)
            .map(ImagePreviewTarget::key)
            .collect::<HashSet<_>>();
        self.cache
            .prune_to_limit(MAX_IMAGE_PREVIEW_CACHE_ENTRIES, |key| {
                protected.contains(key)
            });
    }

    pub(super) fn store_failed(&mut self, url: &str, message: String) {
        for key in self.loading_keys_for_url(url) {
            let filename = self.filename_for_key(&key);
            let last_used = self.cache.next_tick();
            self.cache.entries.insert(
                key,
                ImagePreviewEntry::Failed {
                    filename,
                    message: message.clone(),
                    last_used,
                },
            );
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

fn clipped_preview_stateful_protocol(
    picker: &Picker,
    image: &DynamicImage,
    render_info: ImagePreviewRenderInfo,
) -> Option<Box<StatefulProtocol>> {
    let image = clipped_preview_image(image, picker_font_size(picker), render_info)?;
    Some(Box::new(picker.new_resize_protocol(image)))
}

impl ImagePreviewTarget {
    pub(super) fn key(&self) -> ImagePreviewKey {
        ImagePreviewKey {
            viewer: self.viewer,
            message_id: self.message_id,
            preview_index: self.preview_index,
            url: self.url.clone(),
        }
    }

    pub(super) fn preview_render_info(&self) -> ImagePreviewRenderInfo {
        ImagePreviewRenderInfo {
            message_index: self.message_index,
            preview_x_offset_columns: self.preview_x_offset_columns,
            preview_y_offset_rows: self.preview_y_offset_rows,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            accent_color: self.accent_color,
            show_play_marker: self.show_play_marker,
            viewer: self.viewer,
            mask_circular: false,
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

    fn decoding_generation(&self) -> Option<u64> {
        match self {
            ImagePreviewEntry::Decoding { generation, .. } => Some(*generation),
            ImagePreviewEntry::Loading { .. }
            | ImagePreviewEntry::Ready { .. }
            | ImagePreviewEntry::Failed { .. } => None,
        }
    }
}

#[cfg(test)]
pub(super) fn decode_original_preview_image(
    bytes: &[u8],
) -> std::result::Result<DynamicImage, String> {
    super::decode::decode_image_bytes(bytes)
}
