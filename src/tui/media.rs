use std::collections::{HashMap, HashSet};

use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::{Resize, picker::Picker};

mod preview;
mod targets;

pub(super) use preview::{ImagePreviewCache, ImagePreviewDecodeResult, spawn_image_preview_decode};
#[cfg(test)]
use targets::image_preview_height_for_dimensions;
pub(super) use targets::{
    AvatarTarget, EmojiImageTarget, ImagePreviewTarget, image_preview_album_layout,
    visible_avatar_targets, visible_emoji_image_targets, visible_image_preview_targets,
};

#[cfg(test)]
use preview::{ImagePreviewEntry, MAX_IMAGE_PREVIEW_CACHE_ENTRIES, decode_original_preview_image};

use crate::{
    discord::{AppCommand, AppEvent},
    logging,
};

use super::ui::{AvatarImage, EmojiReactionImage};

const AVATAR_PREVIEW_WIDTH: u16 = 2;
const AVATAR_PREVIEW_HEIGHT: u16 = 2;
const PROFILE_POPUP_AVATAR_WIDTH: u16 = 8;
const PROFILE_POPUP_AVATAR_HEIGHT: u16 = 4;
const AVATAR_SOURCE_PIXELS_PER_COLUMN: u64 = 10;
const AVATAR_SOURCE_PIXELS_PER_ROW: u64 = AVATAR_SOURCE_PIXELS_PER_COLUMN * 3;
const DISCORD_AVATAR_CDN_PREFIX: &str = "https://cdn.discordapp.com/avatars/";
const DISCORD_AVATAR_MIN_SIZE: u64 = 16;
const DISCORD_AVATAR_MAX_SIZE: u64 = 1024;
const EMOJI_REACTION_THUMB_WIDTH: u16 = 2;
const EMOJI_REACTION_THUMB_HEIGHT: u16 = 1;

/// Avatar images are small on screen but decoded originals can still add up
/// as users scroll through large servers. Keep a generous URL-keyed LRU cap.
const MAX_AVATAR_IMAGE_CACHE_ENTRIES: usize = 32;

fn query_image_picker(target: &str, unavailable_message: &str) -> Option<Picker> {
    match Picker::from_query_stdio() {
        Ok(picker) => Some(picker),
        Err(error) => {
            logging::error(target, format!("{unavailable_message}: {error}"));
            None
        }
    }
}

fn avatar_preview_url(url: &str, width_columns: u16, height_rows: u16) -> String {
    if !is_discord_avatar_url(url) {
        return url.to_owned();
    }

    let size = avatar_preview_size(width_columns, height_rows);
    let (base, query) = url.split_once('?').unwrap_or((url, ""));
    let mut params = query
        .split('&')
        .filter(|param| !param.is_empty())
        .filter(|param| {
            let key = param.split_once('=').map_or(*param, |(key, _)| key);
            key != "size"
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    params.push(format!("size={size}"));

    format!("{base}?{}", params.join("&"))
}

fn is_discord_avatar_url(url: &str) -> bool {
    url.starts_with(DISCORD_AVATAR_CDN_PREFIX)
}

fn avatar_preview_size(width_columns: u16, height_rows: u16) -> u64 {
    let width = u64::from(width_columns).saturating_mul(AVATAR_SOURCE_PIXELS_PER_COLUMN);
    let height = u64::from(height_rows).saturating_mul(AVATAR_SOURCE_PIXELS_PER_ROW);
    let needed = width.max(height).max(1);
    needed
        .clamp(DISCORD_AVATAR_MIN_SIZE, DISCORD_AVATAR_MAX_SIZE)
        .next_power_of_two()
        .min(DISCORD_AVATAR_MAX_SIZE)
}

pub(super) struct AvatarImageCache {
    picker: Option<Picker>,
    entries: HashMap<String, AvatarImageEntry>,
    tick: u64,
}

enum AvatarImageEntry {
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
const MAX_EMOJI_IMAGE_CACHE_ENTRIES: usize = 128;

pub(super) struct EmojiImageCache {
    picker: Option<Picker>,
    entries: HashMap<String, EmojiImageEntry>,
    tick: u64,
}

enum EmojiImageEntry {
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
    pub(super) fn new() -> Self {
        Self {
            picker: query_image_picker("avatar", "avatar image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
        }
    }

    pub(super) fn render_state(&mut self, targets: &[AvatarTarget]) -> Vec<AvatarImage> {
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

    pub(super) fn next_requests(&mut self, targets: &[AvatarTarget]) -> Vec<AppCommand> {
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
    pub(super) fn next_request_for_url(&mut self, url: &str) -> Option<AppCommand> {
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
    pub(super) fn popup_avatar_image(&mut self, url: &str) -> Option<AvatarImage> {
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

    pub(super) fn record_event(&mut self, event: &AppEvent) {
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

    fn prune_to_limit(&mut self, targets: &[AvatarTarget]) {
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
    pub(super) fn new() -> Self {
        Self {
            picker: query_image_picker("emoji", "emoji image picker unavailable"),
            entries: HashMap::new(),
            tick: 0,
        }
    }

    /// Returns decoded protocols for visible targets and refreshes their
    /// LRU timestamps so they survive the next pruning pass.
    pub(super) fn render_state(
        &mut self,
        targets: &[EmojiImageTarget],
    ) -> Vec<EmojiReactionImage<'_>> {
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
                Some(EmojiReactionImage {
                    url: target.url.clone(),
                    protocol,
                })
            })
            .collect()
    }

    pub(super) fn next_requests(&mut self, targets: &[EmojiImageTarget]) -> Vec<AppCommand> {
        if self.picker.is_none() {
            return Vec::new();
        }

        let mut commands = Vec::new();
        let mut requested_urls = HashSet::new();
        for target in targets.iter().take(MAX_EMOJI_IMAGE_CACHE_ENTRIES) {
            if self.entries.contains_key(&target.url) {
                continue;
            }

            let last_used = self.next_tick();
            self.entries
                .insert(target.url.clone(), EmojiImageEntry::Loading { last_used });
            if requested_urls.insert(target.url.clone()) {
                commands.push(AppCommand::LoadAttachmentPreview {
                    url: target.url.clone(),
                });
            }
        }
        self.prune_to_limit(targets);
        commands
    }

    pub(super) fn record_event(&mut self, event: &AppEvent) {
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
    fn prune_to_limit(&mut self, targets: &[EmojiImageTarget]) {
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
            Ok(image) => {
                let render_info = ImagePreviewRenderInfo {
                    viewer: false,
                    message_index: 0,
                    preview_x_offset_columns: 0,
                    preview_y_offset_rows: 0,
                    preview_width: EMOJI_REACTION_THUMB_WIDTH,
                    preview_height: EMOJI_REACTION_THUMB_HEIGHT,
                    preview_overflow_count: 0,
                    visible_preview_height: EMOJI_REACTION_THUMB_HEIGHT,
                    top_clip_rows: 0,
                    accent_color: None,
                };
                if let Some(protocol) = clipped_preview_protocol(picker, &image, render_info) {
                    self.entries.insert(
                        url.to_owned(),
                        EmojiImageEntry::Ready {
                            protocol,
                            last_used,
                        },
                    );
                } else {
                    self.entries
                        .insert(url.to_owned(), EmojiImageEntry::Failed { last_used });
                }
            }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImagePreviewRenderInfo {
    viewer: bool,
    message_index: usize,
    preview_x_offset_columns: u16,
    preview_y_offset_rows: usize,
    preview_width: u16,
    preview_height: u16,
    preview_overflow_count: usize,
    visible_preview_height: u16,
    top_clip_rows: u16,
    accent_color: Option<u32>,
}

fn clipped_preview_image(
    image: &DynamicImage,
    font_size: (u16, u16),
    render_info: ImagePreviewRenderInfo,
) -> Option<DynamicImage> {
    if render_info.preview_width == 0
        || render_info.preview_height == 0
        || render_info.visible_preview_height == 0
    {
        return None;
    }

    let (font_width, font_height) = font_size;
    let full_width = u32::from(render_info.preview_width).checked_mul(u32::from(font_width))?;
    let full_height = u32::from(render_info.preview_height).checked_mul(u32::from(font_height))?;
    let crop_top = u32::from(render_info.top_clip_rows).checked_mul(u32::from(font_height))?;
    let crop_height = u32::from(render_info.visible_preview_height)
        .checked_mul(u32::from(font_height))?
        .min(full_height.saturating_sub(crop_top));
    if full_width == 0 || crop_height == 0 {
        return None;
    }

    let fitted = fit_image_to_canvas(image, full_width, full_height);
    Some(fitted.crop_imm(0, crop_top, full_width, crop_height))
}

fn clipped_preview_protocol(
    picker: &Picker,
    image: &DynamicImage,
    render_info: ImagePreviewRenderInfo,
) -> Option<ratatui_image::protocol::Protocol> {
    let image = clipped_preview_image(image, picker.font_size(), render_info)?;
    picker
        .new_protocol(
            image,
            Rect::new(
                0,
                0,
                render_info.preview_width,
                render_info.visible_preview_height,
            ),
            Resize::Fit(None),
        )
        .ok()
}

fn fit_image_to_canvas(image: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    let resized = image.resize(width, height, FilterType::Nearest);
    if resized.width() == width && resized.height() == height {
        return resized;
    }

    let mut canvas =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    image::imageops::overlay(&mut canvas, &resized, 0, 0);
    canvas
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::discord::ids::{Id, marker::MessageMarker};
    use image::{DynamicImage, ImageBuffer, Rgba};

    use crate::{
        discord::{
            AppCommand, AppEvent, AttachmentInfo, ChannelInfo, CustomEmojiInfo, EmbedInfo,
            MessageInfo, MessageSnapshotInfo, ReactionEmoji, ReactionInfo,
        },
        tui::{
            state::{DashboardState, FocusPane},
            ui::ImagePreviewLayout,
        },
    };

    use super::*;

    fn layout(list_height: usize) -> ImagePreviewLayout {
        ImagePreviewLayout {
            list_height,
            content_width: 200,
            preview_width: 16,
            max_preview_height: 3,
            viewer_preview_width: 76,
            viewer_max_preview_height: 13,
        }
    }

    #[test]
    fn image_preview_targets_stop_at_rendered_row_budget() {
        let mut state = state_with_image_messages(6, &[1, 3, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_include_preview_that_would_be_clipped() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_include_multiple_attachments_from_one_message() {
        let mut state = state_with_image_messages(0, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("album".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1), image_attachment(2)],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(12));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1), Id::new(1)]);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://cdn.discordapp.com/image-1.png",
                "https://cdn.discordapp.com/image-2.png",
            ]
        );
        assert_eq!(
            targets
                .iter()
                .map(|target| (
                    target.preview_x_offset_columns,
                    target.preview_y_offset_rows,
                    target.preview_width,
                    target.preview_height,
                ))
                .collect::<Vec<_>>(),
            vec![(0, 0, 8, 3), (8, 0, 8, 3)]
        );
    }

    #[test]
    fn image_preview_targets_use_resized_discord_media_proxy_url() {
        let mut state = state_with_image_messages(0, &[]);
        let mut attachment = image_attachment(1);
        attachment.proxy_url = concat!(
            "https://media.discordapp.net/attachments/691/150/photo.png",
            "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
        )
        .to_owned();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("photo".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![attachment],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("image attachment should produce preview target");

        assert_eq!(
            target.url,
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=webp&quality=lossless&width=160&height=90"
            )
        );
    }

    #[test]
    fn image_preview_targets_layout_three_images_as_large_left_tile() {
        let mut state = state_with_image_messages(0, &[]);
        push_album_message(&mut state, 1, 3);

        let targets = visible_image_preview_targets(&state, layout(12));

        assert_eq!(targets.len(), 3);
        assert_eq!(
            targets
                .iter()
                .map(|target| (
                    target.preview_index,
                    target.preview_x_offset_columns,
                    target.preview_y_offset_rows,
                    target.preview_width,
                    target.preview_height,
                ))
                .collect::<Vec<_>>(),
            vec![(0, 0, 0, 8, 3), (1, 8, 0, 8, 2), (2, 8, 2, 8, 1)]
        );
    }

    #[test]
    fn image_preview_targets_layout_four_images_as_bounded_two_by_two_grid() {
        let mut state = state_with_image_messages(0, &[]);
        push_album_message(&mut state, 1, 4);

        let targets = visible_image_preview_targets(&state, layout(12));

        assert_eq!(targets.len(), 4);
        assert_eq!(
            targets
                .iter()
                .map(|target| (
                    target.preview_index,
                    target.preview_x_offset_columns,
                    target.preview_y_offset_rows,
                    target.preview_width,
                    target.preview_height,
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, 0, 0, 8, 2),
                (1, 8, 0, 8, 2),
                (2, 0, 2, 8, 1),
                (3, 8, 2, 8, 1)
            ]
        );
    }

    #[test]
    fn image_preview_targets_layout_five_images_with_overflow_marker_on_fourth_tile() {
        let mut state = state_with_image_messages(0, &[]);
        push_album_message(&mut state, 1, 5);

        let targets = visible_image_preview_targets(&state, layout(12));

        assert_eq!(targets.len(), 4);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.preview_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert!(
            targets
                .iter()
                .all(|target| target.preview_y_offset_rows < 3)
        );
        assert_eq!(
            targets
                .iter()
                .map(|target| target.preview_overflow_count)
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 1]
        );
    }

    #[test]
    fn image_viewer_target_uses_viewer_layout_dimensions() {
        let mut state = state_with_image_messages(1, &[1]);
        state.focus_pane(FocusPane::Messages);
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("viewer should create one image target");

        assert!(target.viewer);
        assert_eq!(target.preview_width, 76);
        assert_eq!(target.preview_height, 13);
        assert_eq!(target.visible_preview_height, 13);
    }

    #[test]
    fn image_preview_targets_account_for_first_message_line_offset() {
        let mut state = state_with_image_messages(1, &[1]);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(&state, layout(2));

        assert_eq!(
            target_message_ids(&targets),
            Vec::<Id<MessageMarker>>::new()
        );
    }

    #[test]
    fn avatar_targets_include_visible_author_avatar() {
        let state = state_with_avatar_messages(1);

        let targets = visible_avatar_targets(&state, layout(2));

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].row, 1);
        assert_eq!(targets[0].visible_height, 1);
        assert_eq!(targets[0].top_clip_rows, 0);
        assert_eq!(targets[0].url, "https://cdn.discordapp.com/avatar-1.png");
    }

    #[test]
    fn avatar_preview_url_adds_power_of_two_size_for_user_avatar() {
        assert_eq!(
            avatar_preview_url("https://cdn.discordapp.com/avatars/1/hash.png", 2, 2),
            "https://cdn.discordapp.com/avatars/1/hash.png?size=64"
        );
        assert_eq!(
            avatar_preview_url(
                "https://cdn.discordapp.com/avatars/1/hash.png?size=1024&foo=bar",
                8,
                4
            ),
            "https://cdn.discordapp.com/avatars/1/hash.png?foo=bar&size=128"
        );
    }

    #[test]
    fn avatar_preview_url_leaves_default_avatar_unchanged() {
        assert_eq!(
            avatar_preview_url("https://cdn.discordapp.com/embed/avatars/0.png", 8, 4),
            "https://cdn.discordapp.com/embed/avatars/0.png"
        );
    }

    #[test]
    fn avatar_targets_clip_first_message_avatar_after_line_scroll() {
        let mut state = state_with_avatar_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_avatar_targets(&state, layout(1));

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].row, 0);
        assert_eq!(targets[0].visible_height, 1);
        assert_eq!(targets[0].top_clip_rows, 0);
    }

    #[test]
    fn avatar_image_cache_evicts_least_recently_used_when_over_capacity() {
        let mut cache = AvatarImageCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
        };
        for id in 0..MAX_AVATAR_IMAGE_CACHE_ENTRIES {
            let url = avatar_preview_url(
                &format!("https://cdn.discordapp.com/avatars/{id}.png"),
                AVATAR_PREVIEW_WIDTH,
                AVATAR_PREVIEW_HEIGHT,
            );
            cache.entries.insert(
                url,
                AvatarImageEntry::Failed {
                    last_used: id as u64,
                },
            );
        }
        cache.tick = MAX_AVATAR_IMAGE_CACHE_ENTRIES as u64;
        cache.entries.insert(
            "https://cdn.discordapp.com/avatars/oldest.png".to_owned(),
            AvatarImageEntry::Failed { last_used: 0 },
        );

        let visible_url = "https://cdn.discordapp.com/avatars/0.png".to_owned();
        let visible_cache_url =
            avatar_preview_url(&visible_url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
        let targets = vec![AvatarTarget {
            row: 0,
            visible_height: 1,
            top_clip_rows: 0,
            url: visible_url.clone(),
        }];
        cache.prune_to_limit(&targets);

        assert_eq!(cache.entries.len(), MAX_AVATAR_IMAGE_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&visible_cache_url));
        assert!(
            !cache
                .entries
                .contains_key("https://cdn.discordapp.com/avatars/oldest.png")
        );
    }

    #[test]
    fn avatar_popup_request_prunes_cache_to_limit() {
        let mut cache = AvatarImageCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
        };
        for id in 0..MAX_AVATAR_IMAGE_CACHE_ENTRIES {
            cache.entries.insert(
                format!("https://cdn.discordapp.com/avatars/{id}.png"),
                AvatarImageEntry::Failed {
                    last_used: id as u64,
                },
            );
        }

        let request = cache.next_request_for_url("https://cdn.discordapp.com/avatars/new.png");

        assert_eq!(
            request,
            Some(AppCommand::LoadAttachmentPreview {
                url: "https://cdn.discordapp.com/avatars/new.png?size=128".to_owned(),
            })
        );
        assert_eq!(cache.entries.len(), MAX_AVATAR_IMAGE_CACHE_ENTRIES);
        assert!(
            cache
                .entries
                .contains_key("https://cdn.discordapp.com/avatars/new.png?size=128")
        );
    }

    #[test]
    fn image_preview_targets_include_top_clipped_preview_rows() {
        let mut state = state_with_image_messages(1, &[1]);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        for _ in 0..4 {
            state.scroll_message_viewport_down();
            state.clamp_message_viewport_for_image_previews(200, 16, 3);
        }

        let targets = visible_image_preview_targets(&state, layout(2));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
        assert_eq!(targets[0].visible_preview_height, 2);
        assert_eq!(targets[0].top_clip_rows, 0);
    }

    #[test]
    fn image_preview_targets_clip_album_bottom_row_after_line_scroll() {
        let mut state = state_with_image_messages(0, &[]);
        push_album_message(&mut state, 1, 4);
        state.focus_pane(FocusPane::Messages);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        for _ in 0..16 {
            state.scroll_message_viewport_down();
            let targets = visible_image_preview_targets(&state, layout(2));
            if targets
                .first()
                .is_some_and(|target| target.preview_index == 2)
            {
                break;
            }
        }

        let targets = visible_image_preview_targets(&state, layout(2));

        assert_eq!(
            targets
                .iter()
                .map(|target| (
                    target.preview_index,
                    target.preview_y_offset_rows,
                    target.visible_preview_height,
                    target.top_clip_rows,
                ))
                .collect::<Vec<_>>(),
            vec![(2, 2, 1, 0), (3, 2, 1, 0)]
        );
    }

    #[test]
    fn image_preview_targets_skip_preview_when_no_preview_row_is_visible() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(5);

        let targets = visible_image_preview_targets(&state, layout(5));

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_account_for_date_separator_rows() {
        let mut state = state_with_cross_day_image_message();
        state.set_message_view_height(4);

        let targets = visible_image_preview_targets(&state, layout(4));

        assert!(targets.is_empty());
    }

    #[test]
    fn video_attachment_does_not_request_original_as_image_preview() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("clip".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![video_attachment(2)],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(6));

        assert!(targets.is_empty());
    }

    #[test]
    fn image_preview_targets_include_embed_thumbnail() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: vec![youtube_embed()],
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(8));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(
            targets[0].url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        );
        assert_eq!(targets[0].filename, "embed-thumbnail");
    }

    #[test]
    fn image_preview_targets_downscale_youtube_embed_image_url() {
        let mut embed = youtube_embed();
        embed.thumbnail_url = None;
        embed.thumbnail_width = None;
        embed.thumbnail_height = None;
        embed.image_url =
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg?token=abc".to_owned());
        embed.image_width = Some(1280);
        embed.image_height = Some(720);
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: vec![embed],
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(8));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(
            targets[0].url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg?token=abc"
        );
        assert_eq!(targets[0].filename, "embed-image");
    }

    #[test]
    fn image_viewer_target_caps_large_youtube_embed_image_url() {
        let mut embed = youtube_embed();
        embed.thumbnail_url = None;
        embed.thumbnail_width = None;
        embed.thumbnail_height = None;
        embed.image_url = Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg".to_owned());
        embed.image_width = Some(1280);
        embed.image_height = Some(720);
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: vec![embed],
            forwarded_snapshots: Vec::new(),
        });
        state.focus_pane(FocusPane::Messages);
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("viewer should create one image target");

        assert!(target.viewer);
        assert_eq!(
            target.url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        );
        assert_eq!(target.filename, "embed-image");
    }

    #[test]
    fn image_preview_targets_keep_small_youtube_thumbnail_url() {
        let mut embed = youtube_embed();
        embed.thumbnail_url = Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg".to_owned());
        embed.thumbnail_width = Some(120);
        embed.thumbnail_height = Some(90);
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: vec![embed],
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(&state, layout(8));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(
            targets[0].url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg"
        );
        assert_eq!(targets[0].filename, "embed-thumbnail");
    }

    #[test]
    fn image_preview_targets_include_forwarded_image_attachments() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(2)],
        });

        let targets = visible_image_preview_targets(&state, layout(6));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(targets[0].url, "https://cdn.discordapp.com/image-2.png");
    }

    #[test]
    fn image_preview_targets_follow_the_scrolled_message_window() {
        let mut state = state_with_image_messages(8, &[1, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(&state, layout(7));

        assert!(target_message_ids(&targets).is_empty());
    }

    #[test]
    fn image_preview_targets_include_image_messages_in_scrolloff_context() {
        let mut state = state_with_image_messages(8, &[5, 6, 7]);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(14);
        while state.selected_message() > 3 {
            state.move_up();
        }
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(&state, layout(14));

        assert_eq!(target_message_ids(&targets), vec![Id::new(5)]);
    }

    #[test]
    fn image_preview_request_is_created_for_draw_target() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);

        assert!(cache.entries.is_empty());
        assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
        assert!(cache.entries.is_empty());

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert_eq!(
            requests,
            vec![AppCommand::LoadAttachmentPreview {
                url: target.url.clone()
            }]
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn image_preview_render_state_preserves_target_order() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let first = image_preview_target(1);
        let second = ImagePreviewTarget {
            message_id: Id::new(1),
            preview_index: 1,
            preview_x_offset_columns: 8,
            ..image_preview_target(2)
        };
        cache.entries.insert(
            second.key(),
            ImagePreviewEntry::Loading {
                filename: second.filename.clone(),
                render_info: second.preview_render_info(),
                last_used: 1,
            },
        );
        cache.entries.insert(
            first.key(),
            ImagePreviewEntry::Loading {
                filename: first.filename.clone(),
                render_info: first.preview_render_info(),
                last_used: 2,
            },
        );

        let previews = cache.render_state(&[first, second]);

        assert_eq!(
            previews
                .into_iter()
                .map(|preview| match preview.state {
                    super::super::ui::ImagePreviewState::Loading { filename } => filename,
                    _ => "unexpected state".to_owned(),
                })
                .collect::<Vec<_>>(),
            vec!["image-1.png", "image-2.png"]
        );
    }

    #[test]
    fn image_preview_cache_keeps_duplicate_urls_as_separate_preview_instances() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let first = image_preview_target(1);
        let second = ImagePreviewTarget {
            preview_index: 1,
            preview_x_offset_columns: 8,
            ..image_preview_target(1)
        };

        let requests = cache.next_requests(&[first, second]);

        assert_eq!(requests.len(), 1);
        assert_eq!(cache.entries.len(), 2);
        let previews = cache.render_state(&[
            image_preview_target(1),
            ImagePreviewTarget {
                preview_index: 1,
                preview_x_offset_columns: 8,
                ..image_preview_target(1)
            },
        ]);

        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].preview_x_offset_columns, 0);
        assert_eq!(previews[1].preview_x_offset_columns, 8);
    }

    #[test]
    fn image_preview_cache_deduplicates_url_already_loading_from_previous_frame() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let first = image_preview_target(1);
        cache.next_requests(std::slice::from_ref(&first));
        let second = ImagePreviewTarget {
            preview_index: 1,
            preview_x_offset_columns: 8,
            ..image_preview_target(1)
        };

        let requests = cache.next_requests(std::slice::from_ref(&second));

        assert!(requests.is_empty());
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn image_preview_cache_keeps_viewer_and_inline_entries_separate() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let inline = image_preview_target(1);
        let viewer = ImagePreviewTarget {
            viewer: true,
            preview_width: 76,
            preview_height: 13,
            visible_preview_height: 13,
            ..image_preview_target(1)
        };

        let inline_requests = cache.next_requests(std::slice::from_ref(&inline));
        let viewer_requests = cache.next_requests(std::slice::from_ref(&viewer));

        assert_eq!(inline_requests.len(), 1);
        assert!(viewer_requests.is_empty());
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key(&inline.key()));
        assert!(cache.entries.contains_key(&viewer.key()));
    }

    #[test]
    fn image_preview_cache_evicts_least_recently_used_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing_targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64)
            .map(image_preview_target)
            .collect::<Vec<_>>();
        cache.next_requests(&existing_targets);
        cache.render_state(std::slice::from_ref(&existing_targets[0]));

        let new_target = image_preview_target(999);
        cache.next_requests(std::slice::from_ref(&new_target));

        assert_eq!(cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&existing_targets[0].key()));
        assert!(!cache.entries.contains_key(&existing_targets[1].key()));
        assert!(cache.entries.contains_key(&new_target.key()));
    }

    #[test]
    fn image_preview_cache_limits_visible_requests() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64 + 2)
            .map(image_preview_target)
            .collect::<Vec<_>>();

        let requests = cache.next_requests(&targets);

        assert_eq!(cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert_eq!(requests.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&targets[0].key()));
        assert!(
            !cache
                .entries
                .contains_key(&targets[MAX_IMAGE_PREVIEW_CACHE_ENTRIES].key())
        );
    }

    #[test]
    fn image_preview_store_loaded_preserves_existing_non_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing = image_preview_target(1).key();
        let loading = ImagePreviewTarget {
            message_id: Id::new(2),
            ..image_preview_target(1)
        }
        .key();
        cache.entries.insert(
            existing.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );
        cache.entries.insert(
            loading.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info: image_preview_target(1).preview_render_info(),
                last_used: 2,
            },
        );

        cache.store_loaded(&existing.url, &[]);

        assert!(matches!(
            cache.entries.get(&existing),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "existing failure"
        ));
        assert!(matches!(
            cache.entries.get(&loading),
            Some(ImagePreviewEntry::Failed { message, .. })
                if message == "inline preview unavailable in this terminal"
        ));
    }

    #[test]
    fn image_preview_loaded_bytes_start_decode_jobs_for_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);
        let key = target.key();
        let render_info = target.preview_render_info();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info,
                last_used: 1,
            },
        );

        let jobs = cache.decode_jobs_for_loaded_keys(vec![key.clone()], b"image bytes", (10, 20));

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].key, key);
        assert_eq!(jobs[0].generation, 1);
        assert_eq!(jobs[0].bytes.as_ref(), b"image bytes");
        assert!(matches!(
            cache.entries.get(&jobs[0].key),
            Some(ImagePreviewEntry::Decoding { filename, generation, .. })
                if filename == "loading.png" && *generation == 1
        ));
    }

    #[test]
    fn image_preview_store_decoded_records_decode_failure() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);
        let key = target.key();
        let render_info = target.preview_render_info();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Decoding {
                filename: "loading.png".to_owned(),
                generation: 1,
                render_info,
                last_used: 1,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: invalid image".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Failed { filename, message, .. })
                if filename == "loading.png" && message == "decode failed: invalid image"
        ));
    }

    #[test]
    fn image_preview_store_decoded_ignores_stale_results() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let key = image_preview_target(1).key();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: stale".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Failed { filename, message, .. })
                if filename == "existing.png" && message == "existing failure"
        ));
    }

    #[test]
    fn image_preview_store_decoded_ignores_replaced_decoding_generation() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let target = image_preview_target(1);
        let key = target.key();
        let render_info = target.preview_render_info();
        cache.entries.insert(
            key.clone(),
            ImagePreviewEntry::Decoding {
                filename: "newer.png".to_owned(),
                generation: 2,
                render_info,
                last_used: 2,
            },
        );

        cache.store_decoded(ImagePreviewDecodeResult {
            key: key.clone(),
            generation: 1,
            result: Err("decode failed: old generation".to_owned()),
        });

        assert!(matches!(
            cache.entries.get(&key),
            Some(ImagePreviewEntry::Decoding { filename, generation, .. })
                if filename == "newer.png" && *generation == 2
        ));
    }

    #[test]
    fn decode_original_preview_image_reports_invalid_bytes() {
        let error = decode_original_preview_image(b"not an image")
            .expect_err("invalid bytes should fail to decode");

        assert!(error.starts_with("decode failed:"));
    }

    #[test]
    fn image_preview_store_failed_preserves_existing_non_loading_entries() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        };
        let existing = image_preview_target(1).key();
        let loading = ImagePreviewTarget {
            message_id: Id::new(2),
            ..image_preview_target(1)
        }
        .key();
        cache.entries.insert(
            existing.clone(),
            ImagePreviewEntry::Failed {
                filename: "existing.png".to_owned(),
                message: "existing failure".to_owned(),
                last_used: 1,
            },
        );
        cache.entries.insert(
            loading.clone(),
            ImagePreviewEntry::Loading {
                filename: "loading.png".to_owned(),
                render_info: image_preview_target(1).preview_render_info(),
                last_used: 2,
            },
        );

        cache.store_failed(&existing.url, "new failure".to_owned());

        assert!(matches!(
            cache.entries.get(&existing),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "existing failure"
        ));
        assert!(matches!(
            cache.entries.get(&loading),
            Some(ImagePreviewEntry::Failed { message, .. }) if message == "new failure"
        ));
    }

    #[test]
    fn clipped_preview_image_stays_within_preview_pixel_bounds() {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(400, 400, Rgba([0, 0, 0, 255])));
        let render_info = ImagePreviewRenderInfo {
            viewer: false,
            message_index: 0,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: 16,
            preview_height: 3,
            preview_overflow_count: 0,
            visible_preview_height: 3,
            top_clip_rows: 0,
            accent_color: None,
        };

        let resized = clipped_preview_image(&image, (10, 20), render_info)
            .expect("preview dimensions should produce resized image");

        assert!(resized.width() <= 160);
        assert!(resized.height() <= 60);
        assert!(resized.width() < image.width());
        assert!(resized.height() < image.height());
    }

    #[test]
    fn emoji_image_targets_include_visible_custom_reactions() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::GuildEmojisUpdate {
            guild_id: Id::new(1),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
        });
        state.focus_pane(FocusPane::Messages);
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        let targets = visible_emoji_image_targets(&state);

        assert_eq!(
            targets,
            vec![EmojiImageTarget {
                url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            }]
        );
    }

    #[test]
    fn emoji_image_targets_include_visible_forum_preview_custom_reactions() {
        let guild_id = Id::new(1);
        let forum_id = Id::new(20);
        let thread_id = Id::new(30);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: forum_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "forum".to_owned(),
                kind: "GuildForum".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::ForumPostsLoaded {
            channel_id: forum_id,
            archive_state: crate::discord::ForumPostArchiveState::Active,
            offset: 0,
            next_offset: 1,
            posts: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                parent_id: Some(forum_id),
                position: None,
                last_message_id: Some(Id::new(300)),
                name: "welcome".to_owned(),
                kind: "GuildPublicThread".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_archived: Some(false),
                thread_locked: Some(false),
                thread_pinned: Some(false),
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            preview_messages: vec![MessageInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                message_id: Id::new(300),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                pinned: false,
                reactions: vec![ReactionInfo {
                    emoji: ReactionEmoji::Custom {
                        id: Id::new(50),
                        name: Some("party".to_owned()),
                        animated: false,
                    },
                    count: 1,
                    me: false,
                }],
                content: Some("first post".to_owned()),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
                ..MessageInfo::default()
            }],
            has_more: false,
        });

        let targets = visible_emoji_image_targets(&state);

        assert_eq!(
            targets,
            vec![EmojiImageTarget {
                url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            }]
        );
    }

    #[test]
    fn emoji_image_request_is_created_for_visible_target() {
        let mut cache = EmojiImageCache::new();
        let target = EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        };

        if cache.picker.is_none() {
            return;
        }

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert_eq!(
            requests,
            vec![AppCommand::LoadAttachmentPreview {
                url: target.url.clone(),
            }]
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn emoji_image_cache_skips_requests_without_image_protocol() {
        let mut cache = EmojiImageCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
        };
        let target = EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        };

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert!(requests.is_empty());
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn emoji_image_cache_evicts_least_recently_used_when_over_capacity() {
        let mut cache = EmojiImageCache {
            picker: None,
            entries: HashMap::new(),
            tick: 0,
        };
        for id in 0..MAX_EMOJI_IMAGE_CACHE_ENTRIES {
            cache.entries.insert(
                format!("https://cdn.discordapp.com/emojis/{id}.png"),
                EmojiImageEntry::Failed {
                    last_used: id as u64,
                },
            );
        }
        cache.tick = MAX_EMOJI_IMAGE_CACHE_ENTRIES as u64;
        cache.entries.insert(
            "https://cdn.discordapp.com/emojis/oldest.png".to_owned(),
            EmojiImageEntry::Failed { last_used: 0 },
        );

        let visible_url = "https://cdn.discordapp.com/emojis/0.png".to_owned();
        let targets = vec![EmojiImageTarget {
            url: visible_url.clone(),
        }];
        cache.prune_to_limit(&targets);

        assert_eq!(cache.entries.len(), MAX_EMOJI_IMAGE_CACHE_ENTRIES);
        assert!(cache.entries.contains_key(&visible_url));
    }

    #[test]
    fn wide_image_preview_height_is_shorter_than_square_image() {
        let wide = image_preview_height_for_dimensions(60, 10, Some(2400), Some(600));
        let square = image_preview_height_for_dimensions(60, 10, Some(800), Some(800));

        assert!(wide < square);
        assert_eq!(wide, 5);
        assert_eq!(square, 10);
    }

    #[test]
    fn screenshot_like_wide_image_uses_compact_preview_height() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(481), Some(160)),
            6
        );
    }

    #[test]
    fn small_image_preview_height_does_not_upscale_to_full_width() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(100)),
            4
        );
    }

    #[test]
    fn tiny_image_preview_height_stays_compact_but_visible() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(32), Some(32)),
            3
        );
    }

    #[test]
    fn small_wide_image_preview_height_stays_compact() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(40)),
            3
        );
    }

    #[test]
    fn medium_small_square_image_preview_height_stays_below_max() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(128), Some(128)),
            5
        );
    }

    #[test]
    fn image_preview_height_falls_back_to_max_without_dimensions() {
        assert_eq!(image_preview_height_for_dimensions(60, 10, None, None), 10);
    }

    #[test]
    fn image_preview_height_falls_back_to_max_with_zero_dimensions() {
        assert_eq!(
            image_preview_height_for_dimensions(60, 10, Some(0), Some(100)),
            10
        );
    }

    fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: image_message_ids
                    .contains(&id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn state_with_avatar_messages(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: Some(format!("https://cdn.discordapp.com/avatar-{id}.png")),
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn state_with_cross_day_image_message() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        let day_one = snowflake_for_unix_ms(1_743_465_600_000);
        let day_two = snowflake_for_unix_ms(1_743_465_600_000 + 24 * 60 * 60 * 1000);
        for (message_id, attachments) in
            [(day_one, Vec::new()), (day_two, vec![image_attachment(2)])]
        {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id,
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some("msg".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments,
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn target_message_ids(targets: &[ImagePreviewTarget]) -> Vec<Id<MessageMarker>> {
        targets.iter().map(|target| target.message_id).collect()
    }

    fn push_album_message(state: &mut DashboardState, message_id: u64, attachment_count: u64) {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("album".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: (1..=attachment_count).map(image_attachment).collect(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }

    fn image_preview_target(id: u64) -> ImagePreviewTarget {
        ImagePreviewTarget {
            viewer: false,
            message_index: 0,
            preview_index: 0,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: 16,
            preview_height: 3,
            preview_overflow_count: 0,
            visible_preview_height: 3,
            top_clip_rows: 0,
            accent_color: None,
            message_id: Id::new(id),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            filename: format!("image-{id}.png"),
        }
    }

    fn snowflake_for_unix_ms(unix_ms: u64) -> Id<MessageMarker> {
        const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
        const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
        let raw = (unix_ms - DISCORD_EPOCH_MILLIS) << SNOWFLAKE_TIMESTAMP_SHIFT;
        Id::new(raw.max(1))
    }

    fn image_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("image-{id}.png"),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }

    fn video_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("clip-{id}.mp4"),
            url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
            proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }

    fn youtube_embed() -> EmbedInfo {
        EmbedInfo {
            color: Some(0xff0000),
            provider_name: Some("YouTube".to_owned()),
            author_name: None,
            title: Some("Example Video".to_owned()),
            description: Some("A video description".to_owned()),
            fields: Vec::new(),
            footer_text: None,
            url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
            thumbnail_width: Some(480),
            thumbnail_height: Some(360),
            image_url: None,
            image_width: None,
            image_height: None,
            video_url: Some("https://www.youtube.com/embed/dQw4w9WgXcQ".to_owned()),
        }
    }

    fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(format!("forwarded {id}")),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![image_attachment(id)],
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }
}
