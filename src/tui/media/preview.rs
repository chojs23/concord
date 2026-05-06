use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::discord::ids::{Id, marker::MessageMarker};
use image::DynamicImage;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use tokio::{sync::mpsc, task};

use crate::{
    discord::{AppCommand, AppEvent},
    tui::ui::{ImagePreview, ImagePreviewState},
};

use super::{
    ImagePreviewRenderInfo, ImagePreviewTarget, clipped_preview_image, query_image_picker,
};

pub(super) const MAX_IMAGE_PREVIEW_CACHE_ENTRIES: usize = 32;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ImagePreviewKey {
    viewer: bool,
    message_id: Id<MessageMarker>,
    preview_index: usize,
    pub(super) url: String,
}

pub(in crate::tui) struct ImagePreviewCache {
    pub(super) picker: Option<Picker>,
    pub(super) entries: HashMap<ImagePreviewKey, ImagePreviewEntry>,
    pub(super) tick: u64,
    pub(super) decode_generation: u64,
}

pub(in crate::tui) struct ImagePreviewDecodeJob {
    pub(super) key: ImagePreviewKey,
    pub(super) generation: u64,
    pub(super) bytes: Arc<[u8]>,
}

pub(in crate::tui) struct ImagePreviewDecodeResult {
    pub(super) key: ImagePreviewKey,
    pub(super) generation: u64,
    pub(super) result: std::result::Result<DynamicImage, String>,
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
    pub(in crate::tui) fn new() -> Self {
        Self {
            picker: query_image_picker("preview", "inline image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        }
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

        for (key, entry) in &mut self.entries {
            let Some((order, render_info)) = target_by_key.get(key).copied() else {
                continue;
            };
            rendered_keys.insert(key.clone());
            tick_entry(entry, &mut self.tick);
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
                    preview_overflow_count: render_info.preview_overflow_count,
                    accent_color: render_info.accent_color,
                    state,
                },
            ));
        }

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
                        preview_overflow_count: target.preview_overflow_count,
                        accent_color: target.accent_color,
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
        let mut commands = Vec::new();
        let mut requested_urls = self
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, ImagePreviewEntry::Loading { .. }))
            .map(|(key, _)| key.url.clone())
            .collect::<HashSet<_>>();
        for target in targets.iter().take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES) {
            let key = target.key();
            if self.entries.contains_key(&key) {
                continue;
            }

            let url = target.url.clone();
            let last_used = self.next_tick();
            self.entries.insert(
                key,
                ImagePreviewEntry::Loading {
                    filename: target.filename.clone(),
                    render_info: target.preview_render_info(),
                    last_used,
                },
            );
            if requested_urls.insert(url.clone()) {
                commands.push(AppCommand::LoadAttachmentPreview { url });
            }
        }
        self.prune_to_limit(targets);
        commands
    }

    pub(in crate::tui) fn record_event(&mut self, event: &AppEvent) -> Vec<ImagePreviewDecodeJob> {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                self.store_failed(url, message.clone());
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn store_loaded(&mut self, url: &str, bytes: &[u8]) -> Vec<ImagePreviewDecodeJob> {
        let keys = self.loading_keys_for_url(url);
        if keys.is_empty() {
            return Vec::new();
        }

        let Some(font_size) = self.picker.as_ref().map(Picker::font_size) else {
            for key in keys {
                let filename = self.filename_for_key(&key);
                let last_used = self.next_tick();
                self.entries.insert(
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

        self.decode_jobs_for_loaded_keys(keys, bytes, font_size)
    }

    pub(super) fn decode_jobs_for_loaded_keys(
        &mut self,
        keys: Vec<ImagePreviewKey>,
        bytes: &[u8],
        _font_size: (u16, u16),
    ) -> Vec<ImagePreviewDecodeJob> {
        let bytes: Arc<[u8]> = Arc::from(bytes.to_vec());
        let mut jobs = Vec::new();
        for key in keys {
            let filename = self.filename_for_key(&key);
            let Some(render_info) = self.render_info_for_key(&key) else {
                let last_used = self.next_tick();
                self.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "preview dimensions unavailable".to_owned(),
                        last_used,
                    },
                );
                continue;
            };
            let last_used = self.next_tick();
            let generation = self.next_decode_generation();
            self.entries.insert(
                key.clone(),
                ImagePreviewEntry::Decoding {
                    filename,
                    generation,
                    render_info,
                    last_used,
                },
            );
            jobs.push(ImagePreviewDecodeJob {
                key,
                generation,
                bytes: bytes.clone(),
            });
        }
        jobs
    }

    pub(in crate::tui) fn store_decoded(&mut self, result: ImagePreviewDecodeResult) {
        let Some((filename, generation, render_info)) =
            self.entries.get(&result.key).and_then(|entry| {
                if let ImagePreviewEntry::Decoding {
                    filename,
                    generation,
                    render_info,
                    ..
                } = entry
                {
                    Some((filename.clone(), *generation, *render_info))
                } else {
                    None
                }
            })
        else {
            return;
        };

        if generation != result.generation {
            return;
        }

        let last_used = self.next_tick();
        match result.result {
            Ok(image) => {
                let Some(picker) = self.picker.as_ref() else {
                    self.entries.insert(
                        result.key,
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
                    self.entries.insert(
                        result.key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: "inline preview dimensions unavailable".to_owned(),
                            last_used,
                        },
                    );
                    return;
                };
                self.entries.insert(
                    result.key,
                    ImagePreviewEntry::Ready {
                        filename,
                        image: image.clone(),
                        protocol_render_info: render_info,
                        protocol,
                        last_used,
                    },
                );
            }
            Err(message) => {
                self.entries.insert(
                    result.key,
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
        match self.entries.get(key)? {
            ImagePreviewEntry::Loading { render_info, .. }
            | ImagePreviewEntry::Decoding { render_info, .. } => Some(*render_info),
            ImagePreviewEntry::Ready { .. } | ImagePreviewEntry::Failed { .. } => None,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn next_decode_generation(&mut self) -> u64 {
        self.decode_generation = self.decode_generation.saturating_add(1);
        self.decode_generation
    }

    fn prune_to_limit(&mut self, targets: &[ImagePreviewTarget]) {
        if self.entries.len() <= MAX_IMAGE_PREVIEW_CACHE_ENTRIES {
            return;
        }

        let protected = targets
            .iter()
            .take(MAX_IMAGE_PREVIEW_CACHE_ENTRIES)
            .map(ImagePreviewTarget::key)
            .collect::<HashSet<_>>();
        let mut removable = self
            .entries
            .iter()
            .filter(|(key, _)| !protected.contains(*key))
            .map(|(key, entry)| (key.clone(), entry.last_used()))
            .collect::<Vec<_>>();
        removable.sort_by_key(|(_, last_used)| *last_used);

        for (key, _) in removable {
            if self.entries.len() <= MAX_IMAGE_PREVIEW_CACHE_ENTRIES {
                break;
            }
            self.entries.remove(&key);
        }
    }

    pub(super) fn store_failed(&mut self, url: &str, message: String) {
        for key in self.loading_keys_for_url(url) {
            let filename = self.filename_for_key(&key);
            let last_used = self.next_tick();
            self.entries.insert(
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
        self.entries
            .iter()
            .filter(|(key, entry)| {
                key.url == url && matches!(entry, ImagePreviewEntry::Loading { .. })
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    fn filename_for_key(&self, key: &ImagePreviewKey) -> String {
        self.entries
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
    let image = clipped_preview_image(image, picker.font_size(), render_info)?;
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
            preview_overflow_count: self.preview_overflow_count,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
            accent_color: self.accent_color,
            viewer: self.viewer,
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

    fn last_used(&self) -> u64 {
        match self {
            Self::Loading { last_used, .. }
            | Self::Decoding { last_used, .. }
            | Self::Ready { last_used, .. }
            | Self::Failed { last_used, .. } => *last_used,
        }
    }
}

fn tick_entry(entry: &mut ImagePreviewEntry, tick: &mut u64) {
    *tick = tick.saturating_add(1);
    let last_used = *tick;
    match entry {
        ImagePreviewEntry::Loading {
            last_used: value, ..
        }
        | ImagePreviewEntry::Decoding {
            last_used: value, ..
        }
        | ImagePreviewEntry::Ready {
            last_used: value, ..
        }
        | ImagePreviewEntry::Failed {
            last_used: value, ..
        } => *value = last_used,
    }
}

pub(in crate::tui) fn spawn_image_preview_decode(
    job: ImagePreviewDecodeJob,
    tx: mpsc::UnboundedSender<ImagePreviewDecodeResult>,
) {
    task::spawn_blocking(move || {
        let result = decode_image_preview(job);
        let _ = tx.send(result);
    });
}

fn decode_image_preview(job: ImagePreviewDecodeJob) -> ImagePreviewDecodeResult {
    let result = decode_original_preview_image(&job.bytes);
    ImagePreviewDecodeResult {
        key: job.key,
        generation: job.generation,
        result,
    }
}

pub(super) fn decode_original_preview_image(
    bytes: &[u8],
) -> std::result::Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|error| format!("decode failed: {error}"))
}
