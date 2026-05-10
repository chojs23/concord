use std::collections::HashSet;

use crate::{
    config::ImagePreviewQualityPreset,
    discord::{
        InlinePreviewInfo,
        ids::{Id, marker::MessageMarker},
    },
};

use super::super::{
    message_format::format_message_content_lines,
    selection,
    state::{DashboardState, MAX_MENTION_PICKER_VISIBLE},
    ui::{self, ImagePreviewLayout},
};

/// Wide-enough wrap width for the prefetch walk. URL emission is
/// wrap-independent; we just need to defeat slot truncation in reply previews.
const EMOJI_PREFETCH_FORMAT_WIDTH: usize = 10_000;
use super::AVATAR_PREVIEW_HEIGHT;

const EFFICIENT_IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN: u64 = 6;
const IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN: u64 = 10;
const DISCORD_MEDIA_PROXY_PREFIX: &str = "https://media.discordapp.net/";
const DISCORD_IMAGES_EXTERNAL_PREFIX: &str = "https://images-ext-";
const DISCORD_IMAGES_EXTERNAL_HOST_SUFFIX: &str = ".discordapp.net";
const DISCORD_EXTERNAL_PROXY_PATH: &str = "/external/";
const DISCORD_MEDIA_PROXY_PREVIEW_FORMAT: &str = "webp";
const DISCORD_MEDIA_PROXY_LOW_QUALITY: &str = "low";
const DISCORD_MEDIA_PROXY_PREVIEW_QUALITY: &str = "lossless";
const DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION: u64 = 1000;
const YOUTUBE_THUMBNAIL_PREFIXES: [&str; 4] = [
    "https://i.ytimg.com/vi/",
    "https://i.ytimg.com/vi_webp/",
    "https://img.youtube.com/vi/",
    "https://img.youtube.com/vi_webp/",
];
const YOUTUBE_PREVIEW_MEDIUM: &str = "mqdefault";
const YOUTUBE_PREVIEW_HIGH: &str = "hqdefault";
const YOUTUBE_HIGH_PREVIEW_MIN_WIDTH: u64 = 321;
const YOUTUBE_HIGH_PREVIEW_MIN_HEIGHT: u64 = 181;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum YoutubeThumbnailSize {
    Default,
    Medium,
    High,
}

pub(in crate::tui) struct ImagePreviewTarget {
    pub(super) viewer: bool,
    pub(super) message_index: usize,
    pub(super) preview_index: usize,
    pub(super) preview_x_offset_columns: u16,
    pub(super) preview_y_offset_rows: usize,
    pub(super) preview_width: u16,
    pub(super) preview_height: u16,
    pub(super) preview_overflow_count: usize,
    pub(super) visible_preview_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) accent_color: Option<u32>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) url: String,
    pub(super) filename: String,
}

#[derive(Clone)]
pub(in crate::tui) struct AvatarTarget {
    pub(super) row: isize,
    pub(super) visible_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct EmojiImageTarget {
    pub(super) url: String,
}

const MAX_ALBUM_PREVIEW_TILES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ImagePreviewAlbumCell {
    pub(in crate::tui) preview_index: usize,
    pub(in crate::tui) x_offset_columns: u16,
    pub(in crate::tui) y_offset_rows: usize,
    pub(in crate::tui) width: u16,
    pub(in crate::tui) height: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct ImagePreviewAlbumLayout {
    pub(in crate::tui) cells: Vec<ImagePreviewAlbumCell>,
    pub(in crate::tui) height: usize,
    pub(in crate::tui) overflow_count: usize,
}

pub(in crate::tui) fn visible_image_preview_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<ImagePreviewTarget> {
    if !state.show_images() {
        return Vec::new();
    }

    if let Some((message_id, preview_index, preview)) = state.selected_image_viewer_preview() {
        let quality = state.image_preview_quality();
        let preview_height = image_preview_height_for_dimensions(
            layout.viewer_preview_width,
            layout.viewer_max_preview_height,
            preview.width,
            preview.height,
        );
        if preview_height == 0 {
            return Vec::new();
        }
        return vec![ImagePreviewTarget {
            viewer: true,
            message_index: 0,
            preview_index,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: 0,
            preview_width: layout.viewer_preview_width,
            preview_height,
            preview_overflow_count: 0,
            visible_preview_height: preview_height,
            top_clip_rows: 0,
            accent_color: preview.accent_color,
            message_id,
            url: preview_request_url(
                preview,
                layout.viewer_preview_width,
                preview_height,
                quality,
            ),
            filename: preview.filename.to_owned(),
        }];
    }

    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();
    let quality = state.image_preview_quality();

    for (message_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        let line_offset = usize::from(message_index == 0) * state.message_line_scroll();
        let global_index = state.message_scroll().saturating_add(message_index);
        let separator_lines = state.message_extra_top_lines(global_index);
        let body_rows = state.message_body_line_count_for_width(message, layout.content_width);
        let base_rows = state.message_base_line_count_for_width(message, layout.content_width);
        let preview_rows_before_message = body_rows.saturating_add(separator_lines);
        let block_rows = base_rows.saturating_add(separator_lines);

        let previews = message.inline_previews();
        let album =
            image_preview_album_layout(&previews, layout.preview_width, layout.max_preview_height);
        let album_accent_color = (previews.len() == 1)
            .then(|| previews.first().and_then(|preview| preview.accent_color))
            .flatten();
        for cell in &album.cells {
            let preview = previews[cell.preview_index];
            let preview_overflow_count = if cell.preview_index + 1 == MAX_ALBUM_PREVIEW_TILES {
                previews.len().saturating_sub(MAX_ALBUM_PREVIEW_TILES)
            } else {
                0
            };
            let preview_top = rendered_rows as isize
                + preview_rows_before_message as isize
                + cell.y_offset_rows as isize
                - line_offset as isize;
            let preview_bottom = preview_top.saturating_add(cell.height as isize);
            let visible_top = preview_top.max(0);
            let visible_bottom = preview_bottom.min(layout.list_height as isize);
            if cell.width > 0 && cell.height > 0 && visible_top < visible_bottom {
                targets.push(ImagePreviewTarget {
                    viewer: false,
                    message_index,
                    preview_index: cell.preview_index,
                    preview_x_offset_columns: cell.x_offset_columns,
                    preview_y_offset_rows: cell.y_offset_rows,
                    preview_width: cell.width,
                    preview_height: cell.height,
                    preview_overflow_count,
                    visible_preview_height: u16::try_from(visible_bottom - visible_top)
                        .unwrap_or(u16::MAX),
                    top_clip_rows: u16::try_from(visible_top - preview_top).unwrap_or(u16::MAX),
                    accent_color: album_accent_color,
                    message_id: message.id,
                    url: preview_request_url(preview, cell.width, cell.height, quality),
                    filename: preview.filename.to_owned(),
                });
            }
        }

        rendered_rows = rendered_rows.saturating_add(
            block_rows
                .saturating_add(album.height)
                .saturating_add(usize::from(album.overflow_count > 0))
                .saturating_add(ui::MESSAGE_ROW_GAP)
                .saturating_sub(line_offset),
        );
    }

    targets
}

fn preview_request_url(
    preview: InlinePreviewInfo<'_>,
    width_columns: u16,
    height_rows: u16,
    quality: ImagePreviewQualityPreset,
) -> String {
    if quality == ImagePreviewQualityPreset::Original {
        return preview.url.to_owned();
    }

    if let Some(proxy_url) = preview.proxy_url
        && discord_media_proxy_supports_preview_resize(proxy_url)
    {
        return discord_media_proxy_preview_url(
            proxy_url,
            width_columns,
            height_rows,
            preview.width,
            preview.height,
            quality,
        );
    }

    youtube_thumbnail_preview_url(preview.url, width_columns, height_rows, quality)
        .unwrap_or_else(|| preview.url.to_owned())
}

fn youtube_thumbnail_preview_url(
    url: &str,
    width_columns: u16,
    height_rows: u16,
    quality: ImagePreviewQualityPreset,
) -> Option<String> {
    let prefix = YOUTUBE_THUMBNAIL_PREFIXES
        .iter()
        .find(|prefix| url.starts_with(**prefix))?;
    let tail = &url[prefix.len()..];
    let slash_index = tail.find('/')?;
    let (video_id, file_and_query) = tail.split_at(slash_index);
    if video_id.is_empty() {
        return None;
    }

    let file_and_query = &file_and_query[1..];
    let (file, query) = file_and_query
        .split_once('?')
        .unwrap_or((file_and_query, ""));
    let (variant, extension) = file.rsplit_once('.')?;
    let source_size = youtube_thumbnail_size(variant)?;

    let (pixels_per_column, pixels_per_row) = preview_source_pixels_per_cell(quality);
    let width = preview_dimension_pixels(u64::from(width_columns), pixels_per_column);
    let height = preview_dimension_pixels(u64::from(height_rows), pixels_per_row);
    let requested_size =
        if width >= YOUTUBE_HIGH_PREVIEW_MIN_WIDTH || height >= YOUTUBE_HIGH_PREVIEW_MIN_HEIGHT {
            YoutubeThumbnailSize::High
        } else {
            YoutubeThumbnailSize::Medium
        };
    let target_variant = match source_size.min(requested_size) {
        YoutubeThumbnailSize::Default => "default",
        YoutubeThumbnailSize::Medium => YOUTUBE_PREVIEW_MEDIUM,
        YoutubeThumbnailSize::High => YOUTUBE_PREVIEW_HIGH,
    };

    let mut rewritten = format!("{prefix}{video_id}/{target_variant}.{extension}");
    if !query.is_empty() {
        rewritten.push('?');
        rewritten.push_str(query);
    }
    Some(rewritten)
}

fn youtube_thumbnail_size(variant: &str) -> Option<YoutubeThumbnailSize> {
    match variant {
        "default" => Some(YoutubeThumbnailSize::Default),
        "mqdefault" => Some(YoutubeThumbnailSize::Medium),
        "maxresdefault" | "sddefault" | "hqdefault" | "hq720" => Some(YoutubeThumbnailSize::High),
        _ => None,
    }
}

fn discord_media_proxy_preview_url(
    proxy_url: &str,
    width_columns: u16,
    height_rows: u16,
    source_width: Option<u64>,
    source_height: Option<u64>,
    quality: ImagePreviewQualityPreset,
) -> String {
    let (width, height) = discord_media_proxy_preview_dimensions(
        width_columns,
        height_rows,
        source_width,
        source_height,
        quality,
    );
    let (base, query) = proxy_url.split_once('?').unwrap_or((proxy_url, ""));
    let mut params = query
        .split('&')
        .filter(|param| !param.is_empty())
        .filter(|param| {
            let key = param.split_once('=').map_or(*param, |(key, _)| key);
            !matches!(key, "format" | "quality" | "width" | "height")
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    params.push(format!("format={DISCORD_MEDIA_PROXY_PREVIEW_FORMAT}"));
    match quality {
        ImagePreviewQualityPreset::Efficient => {
            params.push(format!("quality={DISCORD_MEDIA_PROXY_LOW_QUALITY}"));
        }
        ImagePreviewQualityPreset::High => {
            params.push(format!("quality={DISCORD_MEDIA_PROXY_PREVIEW_QUALITY}"));
        }
        ImagePreviewQualityPreset::Balanced | ImagePreviewQualityPreset::Original => {}
    }
    params.push(format!("width={width}"));
    params.push(format!("height={height}"));

    format!("{base}?{}", params.join("&"))
}

fn discord_media_proxy_preview_dimensions(
    width_columns: u16,
    height_rows: u16,
    source_width: Option<u64>,
    source_height: Option<u64>,
    quality: ImagePreviewQualityPreset,
) -> (u64, u64) {
    if let (Some(source_width), Some(source_height)) = (source_width, source_height)
        && source_width > 0
        && source_height > 0
    {
        let (scale_numerator, scale_denominator) = preview_source_scale(quality);
        return scaled_preview_dimensions(
            source_width,
            source_height,
            scale_numerator,
            scale_denominator,
        );
    }

    let (pixels_per_column, pixels_per_row) = preview_source_pixels_per_cell(quality);
    (
        preview_dimension_pixels(u64::from(width_columns), pixels_per_column),
        preview_dimension_pixels(u64::from(height_rows), pixels_per_row),
    )
}

fn preview_source_scale(quality: ImagePreviewQualityPreset) -> (u64, u64) {
    match quality {
        ImagePreviewQualityPreset::Efficient => (3, 10),
        ImagePreviewQualityPreset::Balanced => (1, 2),
        ImagePreviewQualityPreset::High | ImagePreviewQualityPreset::Original => (1, 1),
    }
}

fn scaled_preview_dimensions(
    source_width: u64,
    source_height: u64,
    scale_numerator: u64,
    scale_denominator: u64,
) -> (u64, u64) {
    let scaled_width = scaled_dimension(source_width, scale_numerator, scale_denominator);
    let scaled_height = scaled_dimension(source_height, scale_numerator, scale_denominator);
    cap_preview_dimensions_preserving_aspect(scaled_width, scaled_height)
}

fn scaled_dimension(value: u64, numerator: u64, denominator: u64) -> u64 {
    if denominator == 0 {
        return 1;
    }

    let scaled = (u128::from(value) * u128::from(numerator)).div_ceil(u128::from(denominator));
    u64::try_from(scaled).unwrap_or(u64::MAX).max(1)
}

fn cap_preview_dimensions_preserving_aspect(width: u64, height: u64) -> (u64, u64) {
    let max_dimension = width.max(height);
    if max_dimension <= DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION {
        return (width.max(1), height.max(1));
    }

    let capped_width = (u128::from(width) * u128::from(DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION))
        .div_ceil(u128::from(max_dimension));
    let capped_height = (u128::from(height)
        * u128::from(DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION))
    .div_ceil(u128::from(max_dimension));

    (
        u64::try_from(capped_width)
            .unwrap_or(DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION)
            .max(1),
        u64::try_from(capped_height)
            .unwrap_or(DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION)
            .max(1),
    )
}

fn preview_source_pixels_per_cell(quality: ImagePreviewQualityPreset) -> (u64, u64) {
    let pixels_per_column = match quality {
        ImagePreviewQualityPreset::Efficient => EFFICIENT_IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN,
        ImagePreviewQualityPreset::Balanced
        | ImagePreviewQualityPreset::High
        | ImagePreviewQualityPreset::Original => IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN,
    };
    (pixels_per_column, pixels_per_column * 3)
}

fn discord_media_proxy_supports_preview_resize(proxy_url: &str) -> bool {
    if let Some(path) = proxy_url.strip_prefix(DISCORD_MEDIA_PROXY_PREFIX) {
        return path.starts_with("attachments/")
            || path.starts_with("ephemeral-attachments/")
            || path.starts_with("external/");
    }

    let Some(rest) = proxy_url.strip_prefix(DISCORD_IMAGES_EXTERNAL_PREFIX) else {
        return false;
    };
    let Some((host_tail, path)) = rest.split_once('/') else {
        return false;
    };

    host_tail.ends_with(DISCORD_IMAGES_EXTERNAL_HOST_SUFFIX)
        && path.starts_with(&DISCORD_EXTERNAL_PROXY_PATH[1..])
}

fn preview_dimension_pixels(cells: u64, pixels_per_cell: u64) -> u64 {
    cells
        .saturating_mul(pixels_per_cell)
        .clamp(1, DISCORD_MEDIA_PROXY_MAX_PREVIEW_DIMENSION)
}

pub(in crate::tui) fn visible_avatar_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<AvatarTarget> {
    if !state.show_avatars() {
        return Vec::new();
    }

    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();

    for (local_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        let line_offset = usize::from(rendered_rows == 0) * state.message_line_scroll();
        let global_index = state.message_scroll().saturating_add(local_index);
        let separator_lines = state.message_extra_top_lines(global_index);
        let body_base_rows = state.message_base_line_count_for_width(message, layout.content_width);
        let block_rows = body_base_rows + separator_lines;
        let message_block_top = rendered_rows as isize - line_offset as isize;
        let body_top = message_block_top + separator_lines as isize;
        let avatar_bottom = body_top.saturating_add(AVATAR_PREVIEW_HEIGHT as isize);
        let visible_top = body_top.max(0);
        let visible_bottom = avatar_bottom.min(layout.list_height as isize);
        if let Some(url) = message.author_avatar_url.as_ref()
            && visible_top < visible_bottom
        {
            targets.push(AvatarTarget {
                row: visible_top,
                visible_height: u16::try_from(visible_bottom - visible_top).unwrap_or(u16::MAX),
                top_clip_rows: u16::try_from(visible_top - body_top).unwrap_or(u16::MAX),
                url: url.clone(),
            });
        }

        let previews = message.inline_previews();
        let album =
            image_preview_album_layout(&previews, layout.preview_width, layout.max_preview_height);
        let preview_height = album
            .height
            .saturating_add(usize::from(album.overflow_count > 0));
        rendered_rows = rendered_rows.saturating_add(
            block_rows
                .saturating_add(preview_height)
                .saturating_add(ui::MESSAGE_ROW_GAP)
                .saturating_sub(line_offset),
        );
    }

    targets
}

pub(in crate::tui) fn visible_emoji_image_targets(state: &DashboardState) -> Vec<EmojiImageTarget> {
    if !state.show_custom_emoji() {
        return Vec::new();
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut targets: Vec<EmojiImageTarget> = Vec::new();

    if state.is_composing() {
        for completion in state.composer_emoji_image_completions() {
            if seen.insert(completion.url.clone()) {
                targets.push(EmojiImageTarget {
                    url: completion.url,
                });
            }
        }
    }

    if state.composer_emoji_query().is_some() {
        let candidates = state.composer_emoji_candidates();
        if !candidates.is_empty() {
            let selected = state
                .composer_emoji_selected()
                .min(candidates.len().saturating_sub(1));
            let visible_items = candidates.len().clamp(1, MAX_MENTION_PICKER_VISIBLE);
            let visible_range =
                selection::visible_item_range(candidates.len(), selected, visible_items);
            for candidate in &candidates[visible_range] {
                if let Some(url) = candidate.custom_image_url.clone()
                    && seen.insert(url.clone())
                {
                    targets.push(EmojiImageTarget { url });
                }
            }
        }
    }

    // Picker emojis (existing behaviour).
    if state.is_emoji_reaction_picker_open() {
        let reactions = state.filtered_emoji_reaction_items_slice().unwrap_or(&[]);
        if !reactions.is_empty() {
            let selected = state
                .selected_emoji_reaction_index_for_len(reactions.len())
                .unwrap_or(0)
                .min(reactions.len().saturating_sub(1));
            let visible_items = reactions
                .len()
                .clamp(1, selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
            let visible_range =
                selection::visible_item_range(reactions.len(), selected, visible_items);
            for reaction in &reactions[visible_range] {
                if let Some(url) = reaction.custom_image_url()
                    && seen.insert(url.clone())
                {
                    targets.push(EmojiImageTarget { url });
                }
            }
        }
    }

    // Reactions + every body slot the renderer will draw. Walking the same
    // formatter as `message_viewport_lines` keeps prefetch and render in lockstep.
    for message in state.visible_messages() {
        for reaction in &message.reactions {
            if reaction.count == 0 {
                continue;
            }
            if let Some(url) = reaction.emoji.custom_image_url()
                && seen.insert(url.clone())
            {
                targets.push(EmojiImageTarget { url });
            }
        }
        for line in format_message_content_lines(message, state, EMOJI_PREFETCH_FORMAT_WIDTH) {
            for slot in &line.image_slots {
                if seen.insert(slot.url.clone()) {
                    targets.push(EmojiImageTarget {
                        url: slot.url.clone(),
                    });
                }
            }
        }
    }

    // Forum post cards render preview-message reactions through a separate
    // card pipeline, so they do not appear in `visible_messages()` while a
    // forum channel is selected. Collect those URLs here so the shared emoji
    // image cache can still load and render them.
    for post in state.visible_forum_post_items() {
        for reaction in &post.preview_reactions {
            if reaction.count == 0 {
                continue;
            }
            if let Some(url) = reaction.emoji.custom_image_url()
                && seen.insert(url.clone())
            {
                targets.push(EmojiImageTarget { url });
            }
        }
    }

    // Activity emojis for visible member list members.
    for member in state.flattened_members() {
        for activity in state.user_activities(member.user_id()) {
            if let Some(emoji) = &activity.emoji {
                if let Some(id) = emoji.id {
                    let ext = if emoji.animated { "gif" } else { "png" };
                    let url = format!("https://cdn.discordapp.com/emojis/{}.{}", id.get(), ext);
                    if seen.insert(url.clone()) {
                        targets.push(EmojiImageTarget { url });
                    }
                }
            }
        }
    }

    targets
}

pub(in crate::tui) fn image_preview_album_layout(
    previews: &[InlinePreviewInfo<'_>],
    preview_width: u16,
    max_preview_height: u16,
) -> ImagePreviewAlbumLayout {
    if previews.is_empty() || preview_width == 0 || max_preview_height == 0 {
        return ImagePreviewAlbumLayout::default();
    }

    if previews.len() == 1 {
        let preview = previews[0];
        let height = image_preview_height_for_dimensions(
            preview_width,
            max_preview_height,
            preview.width,
            preview.height,
        );
        if height == 0 {
            return ImagePreviewAlbumLayout::default();
        }
        return ImagePreviewAlbumLayout {
            cells: vec![ImagePreviewAlbumCell {
                preview_index: 0,
                x_offset_columns: 0,
                y_offset_rows: 0,
                width: preview_width,
                height,
            }],
            height: height as usize,
            overflow_count: 0,
        };
    }

    let (left_width, right_width) = split_cells(preview_width);
    let overflow_count = previews.len().saturating_sub(MAX_ALBUM_PREVIEW_TILES);
    match previews.len().min(MAX_ALBUM_PREVIEW_TILES) {
        2 => {
            let height = previews
                .iter()
                .take(2)
                .zip([left_width, right_width])
                .map(|(preview, width)| {
                    image_preview_height_for_dimensions(
                        width,
                        max_preview_height,
                        preview.width,
                        preview.height,
                    )
                })
                .max()
                .unwrap_or(0);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: left_width,
                        height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: left_width,
                        y_offset_rows: 0,
                        width: right_width,
                        height,
                    },
                ],
                height: height as usize,
                overflow_count,
            }
        }
        3 => {
            let (top_height, bottom_height) = split_cells(max_preview_height);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: left_width,
                        height: max_preview_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: left_width,
                        y_offset_rows: 0,
                        width: right_width,
                        height: top_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 2,
                        x_offset_columns: left_width,
                        y_offset_rows: top_height as usize,
                        width: right_width,
                        height: bottom_height,
                    },
                ],
                height: max_preview_height as usize,
                overflow_count,
            }
        }
        _ => {
            let (top_height, bottom_height) = split_cells(max_preview_height);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: left_width,
                        height: top_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: left_width,
                        y_offset_rows: 0,
                        width: right_width,
                        height: top_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 2,
                        x_offset_columns: 0,
                        y_offset_rows: top_height as usize,
                        width: left_width,
                        height: bottom_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 3,
                        x_offset_columns: left_width,
                        y_offset_rows: top_height as usize,
                        width: right_width,
                        height: bottom_height,
                    },
                ],
                height: max_preview_height as usize,
                overflow_count,
            }
        }
    }
}

fn split_cells(value: u16) -> (u16, u16) {
    let first = value.div_ceil(2);
    (first, value.saturating_sub(first))
}

pub(in crate::tui) fn image_preview_height_for_dimensions(
    preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
) -> u16 {
    if preview_width == 0 || max_preview_height == 0 {
        return 0;
    }

    let (Some(image_width), Some(image_height)) = (image_width, image_height) else {
        return max_preview_height;
    };
    if image_width == 0 || image_height == 0 {
        return max_preview_height;
    }

    let source_width_columns = image_width.div_ceil(IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN);
    let preview_width = preview_width.min(u16::try_from(source_width_columns).unwrap_or(u16::MAX));

    let rows = (u128::from(preview_width) * u128::from(image_height))
        .div_ceil(u128::from(image_width) * 3);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);

    rows.clamp(3.min(max_preview_height), max_preview_height)
}
