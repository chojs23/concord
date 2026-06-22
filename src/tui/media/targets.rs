use std::collections::HashSet;

use crate::{
    config::ImagePreviewQualityPreset,
    discord::{
        InlinePreviewInfo,
        ids::{Id, marker::MessageMarker},
    },
};

use super::super::{
    message::{format::format_message_content_lines, layout::MessageViewportPlan},
    selection,
    state::{ActiveModalPopupKind, DashboardState, MAX_MENTION_PICKER_VISIBLE},
    ui::ImagePreviewLayout,
};

/// Wide-enough wrap width for the prefetch walk. URL emission is
/// wrap-independent. It only needs to avoid slot truncation in reply previews.
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
    pub(super) visible_preview_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) accent_color: Option<u32>,
    pub(super) show_play_marker: bool,
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

#[cfg(test)]
pub(in crate::tui) fn visible_image_preview_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<ImagePreviewTarget> {
    if let Some((message_id, preview_index, preview)) = state.selected_attachment_viewer_preview()
        && state.show_images()
    {
        let quality = state.image_preview_quality();
        let (preview_width, preview_height) = image_preview_size_for_dimensions(
            layout.viewer_preview_width,
            layout.viewer_max_preview_height,
            preview.width,
            preview.height,
            true,
            layout.font_size,
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
            preview_width,
            preview_height,
            visible_preview_height: preview_height,
            top_clip_rows: 0,
            accent_color: preview.accent_color,
            show_play_marker: preview.show_play_marker,
            message_id,
            url: preview_request_url(preview, preview_width, preview_height, quality),
            filename: preview.filename.to_owned(),
        }];
    }

    if !state.show_images() {
        return Vec::new();
    }

    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let plan = MessageViewportPlan::new(
        &messages,
        selected,
        state,
        layout.content_width,
        layout.preview_width,
        layout.max_preview_height,
    );
    visible_image_preview_targets_from_plan(state, layout, &plan)
}

pub(in crate::tui) fn visible_image_preview_targets_from_plan(
    state: &DashboardState,
    layout: ImagePreviewLayout,
    plan: &MessageViewportPlan<'_>,
) -> Vec<ImagePreviewTarget> {
    if let Some((message_id, preview_index, preview)) = state.selected_attachment_viewer_preview()
        && state.show_images()
    {
        let quality = state.image_preview_quality();
        let (preview_width, preview_height) = image_preview_size_for_dimensions(
            layout.viewer_preview_width,
            layout.viewer_max_preview_height,
            preview.width,
            preview.height,
            true,
            layout.font_size,
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
            preview_width,
            preview_height,
            visible_preview_height: preview_height,
            top_clip_rows: 0,
            accent_color: preview.accent_color,
            show_play_marker: preview.show_play_marker,
            message_id,
            url: preview_request_url(preview, preview_width, preview_height, quality),
            filename: preview.filename.to_owned(),
        }];
    }

    if !state.show_images() {
        return Vec::new();
    }

    let mut targets = Vec::new();
    let quality = state.image_preview_quality();

    for (message_index, row) in plan.rows().iter().enumerate() {
        if row.message_top >= layout.list_height as isize {
            break;
        }

        let previews = row.message.inline_previews();
        let album =
            image_preview_album_layout(&previews, layout.preview_width, layout.max_preview_height);
        let preview_top_base = row.body_top + row.metrics.body_rows() as isize;
        let album_accent_color = (previews.len() == 1)
            .then(|| previews.first().and_then(|preview| preview.accent_color))
            .flatten();
        for cell in &album.cells {
            let preview = previews[cell.preview_index];
            let preview_top = preview_top_base + cell.y_offset_rows as isize;
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
                    visible_preview_height: u16::try_from(visible_bottom - visible_top)
                        .unwrap_or(u16::MAX),
                    top_clip_rows: u16::try_from(visible_top - preview_top).unwrap_or(u16::MAX),
                    accent_color: album_accent_color,
                    show_play_marker: preview.show_play_marker,
                    message_id: row.message.id,
                    url: preview_request_url(preview, cell.width, cell.height, quality),
                    filename: preview.filename.to_owned(),
                });
            }
        }
    }

    targets
}

fn image_preview_size_for_dimensions(
    max_preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
    viewer: bool,
    font_size: Option<(u16, u16)>,
) -> (u16, u16) {
    if max_preview_width == 0 || max_preview_height == 0 {
        return (0, 0);
    }

    let (Some(image_width), Some(image_height)) = (image_width, image_height) else {
        return (max_preview_width, max_preview_height);
    };
    if image_width == 0 || image_height == 0 {
        return (max_preview_width, max_preview_height);
    }

    let (cell_w, cell_h) = cell_aspect_ratio(font_size);
    let width_for_height = (u128::from(max_preview_height) * u128::from(image_width) * cell_h)
        / (u128::from(image_height) * cell_w);
    let mut preview_width = max_preview_width
        .min(u16::try_from(width_for_height.max(1)).unwrap_or(u16::MAX))
        .max(1);
    if !viewer {
        let source_width_columns = image_width.div_ceil(IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN);
        preview_width = preview_width.min(u16::try_from(source_width_columns).unwrap_or(u16::MAX));
    }
    let preview_height = image_preview_height_for_dimensions_inner(
        preview_width,
        max_preview_height,
        Some(image_width),
        Some(image_height),
        viewer,
        font_size,
    );

    (preview_width, preview_height)
}

/// Returns (cell_width_px, cell_height_px) for the terminal's font cell.
/// Falls back to the legacy 1:3 (width:height) ratio when the picker
/// couldn't query the terminal — preserves prior behavior in that case.
fn cell_aspect_ratio(font_size: Option<(u16, u16)>) -> (u128, u128) {
    match font_size {
        Some((w, h)) if w > 0 && h > 0 => (u128::from(w), u128::from(h)),
        _ => (1, 3),
    }
}

fn preview_request_url(
    preview: InlinePreviewInfo<'_>,
    width_columns: u16,
    height_rows: u16,
    quality: ImagePreviewQualityPreset,
) -> String {
    if quality == ImagePreviewQualityPreset::Original && !preview.proxy_preview_only {
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

#[cfg(test)]
pub(in crate::tui) fn visible_avatar_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<AvatarTarget> {
    if !state.show_avatars() {
        return Vec::new();
    }

    let messages = state.visible_messages();
    let selected = state.focused_message_selection();
    let plan = MessageViewportPlan::new(
        &messages,
        selected,
        state,
        layout.content_width,
        layout.preview_width,
        layout.max_preview_height,
    );
    visible_avatar_targets_from_plan(state, layout, &plan)
}

pub(in crate::tui) fn visible_avatar_targets_from_plan(
    state: &DashboardState,
    layout: ImagePreviewLayout,
    plan: &MessageViewportPlan<'_>,
) -> Vec<AvatarTarget> {
    if !state.show_avatars() {
        return Vec::new();
    }

    let mut targets = Vec::new();

    for row in plan.rows() {
        if row.message_top >= layout.list_height as isize {
            break;
        }

        let avatar_bottom = row.body_top.saturating_add(AVATAR_PREVIEW_HEIGHT as isize);
        let visible_top = row.body_top.max(0);
        let visible_bottom = avatar_bottom.min(layout.list_height as isize);
        if row.show_header
            && let Some(url) = row.message.author_avatar_url.as_ref()
            && visible_top < visible_bottom
        {
            targets.push(AvatarTarget {
                row: visible_top,
                visible_height: u16::try_from(visible_bottom - visible_top).unwrap_or(u16::MAX),
                top_clip_rows: u16::try_from(visible_top - row.body_top).unwrap_or(u16::MAX),
                url: url.clone(),
            });
        }
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
            let visible_items = candidates.len().clamp(1, MAX_MENTION_PICKER_VISIBLE);
            let window_start = state.composer_emoji_window_start(visible_items, candidates.len());
            let window_end = (window_start + visible_items).min(candidates.len());
            for candidate in &candidates[window_start..window_end] {
                if let Some(url) = candidate.custom_image_url.clone()
                    && seen.insert(url.clone())
                {
                    targets.push(EmojiImageTarget { url });
                }
            }
        }
    }

    if state.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker) {
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

    for member in state.flattened_members() {
        for activity in state.user_activities(member.user_id()) {
            if let Some(url) = activity.emoji.as_ref().and_then(|emoji| emoji.image_url())
                && seen.insert(url.clone())
            {
                targets.push(EmojiImageTarget { url });
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
        let (width, height) = image_preview_size_for_dimensions(
            preview_width,
            max_preview_height,
            preview.width,
            preview.height,
            false,
            None,
        );
        if width == 0 || height == 0 {
            return ImagePreviewAlbumLayout::default();
        }
        return ImagePreviewAlbumLayout {
            cells: vec![ImagePreviewAlbumCell {
                preview_index: 0,
                x_offset_columns: 0,
                y_offset_rows: 0,
                width,
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
            let (first_width, first_height) = image_preview_size_for_dimensions(
                left_width,
                max_preview_height,
                previews[0].width,
                previews[0].height,
                false,
                None,
            );
            let (second_width, second_height) = image_preview_size_for_dimensions(
                right_width,
                max_preview_height,
                previews[1].width,
                previews[1].height,
                false,
                None,
            );
            let row_height = first_height.max(second_height);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: first_width,
                        height: first_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: first_width,
                        y_offset_rows: 0,
                        width: second_width,
                        height: second_height,
                    },
                ],
                height: row_height as usize,
                overflow_count,
            }
        }
        3 => {
            let (top_height, bottom_height) = split_cells(max_preview_height);
            let (left_actual_width, left_actual_height) = image_preview_size_for_dimensions(
                left_width,
                max_preview_height,
                previews[0].width,
                previews[0].height,
                false,
                None,
            );
            let right_capacity = preview_width.saturating_sub(left_actual_width).max(1);
            let (top_width, top_actual_height) = image_preview_size_for_dimensions(
                right_capacity,
                top_height,
                previews[1].width,
                previews[1].height,
                false,
                None,
            );
            let (bottom_width, bottom_actual_height) = image_preview_size_for_dimensions(
                right_capacity,
                bottom_height,
                previews[2].width,
                previews[2].height,
                false,
                None,
            );
            let right_stack_height =
                usize::from(top_actual_height).saturating_add(usize::from(bottom_actual_height));
            let height = usize::from(left_actual_height).max(right_stack_height);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: left_actual_width,
                        height: left_actual_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: left_actual_width,
                        y_offset_rows: 0,
                        width: top_width,
                        height: top_actual_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 2,
                        x_offset_columns: left_actual_width,
                        y_offset_rows: top_actual_height as usize,
                        width: bottom_width,
                        height: bottom_actual_height,
                    },
                ],
                height,
                overflow_count,
            }
        }
        _ => {
            let (top_height, bottom_height) = split_cells(max_preview_height);
            let (top_left_width, top_left_height) = image_preview_size_for_dimensions(
                left_width,
                top_height,
                previews[0].width,
                previews[0].height,
                false,
                None,
            );
            let (top_right_width, top_right_height) = image_preview_size_for_dimensions(
                right_width,
                top_height,
                previews[1].width,
                previews[1].height,
                false,
                None,
            );
            let (bottom_left_width, bottom_left_height) = image_preview_size_for_dimensions(
                left_width,
                bottom_height,
                previews[2].width,
                previews[2].height,
                false,
                None,
            );
            let (bottom_right_width, bottom_right_height) = image_preview_size_for_dimensions(
                right_width,
                bottom_height,
                previews[3].width,
                previews[3].height,
                false,
                None,
            );
            let top_row_height = top_left_height.max(top_right_height);
            let bottom_row_height = bottom_left_height.max(bottom_right_height);
            ImagePreviewAlbumLayout {
                cells: vec![
                    ImagePreviewAlbumCell {
                        preview_index: 0,
                        x_offset_columns: 0,
                        y_offset_rows: 0,
                        width: top_left_width,
                        height: top_left_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 1,
                        x_offset_columns: top_left_width,
                        y_offset_rows: 0,
                        width: top_right_width,
                        height: top_right_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 2,
                        x_offset_columns: 0,
                        y_offset_rows: top_row_height as usize,
                        width: bottom_left_width,
                        height: bottom_left_height,
                    },
                    ImagePreviewAlbumCell {
                        preview_index: 3,
                        x_offset_columns: bottom_left_width,
                        y_offset_rows: top_row_height as usize,
                        width: bottom_right_width,
                        height: bottom_right_height,
                    },
                ],
                height: usize::from(top_row_height).saturating_add(usize::from(bottom_row_height)),
                overflow_count,
            }
        }
    }
}

fn split_cells(value: u16) -> (u16, u16) {
    let first = value.div_ceil(2);
    (first, value.saturating_sub(first))
}

#[cfg(test)]
pub(in crate::tui) fn image_preview_height_for_dimensions(
    preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
) -> u16 {
    image_preview_height_for_dimensions_inner(
        preview_width,
        max_preview_height,
        image_width,
        image_height,
        false,
        None,
    )
}

fn image_preview_height_for_dimensions_inner(
    preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
    viewer: bool,
    font_size: Option<(u16, u16)>,
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

    let preview_width = if viewer {
        preview_width
    } else {
        let source_width_columns = image_width.div_ceil(IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN);
        preview_width.min(u16::try_from(source_width_columns).unwrap_or(u16::MAX))
    };

    let (cell_w, cell_h) = cell_aspect_ratio(font_size);
    let rows = (u128::from(preview_width) * u128::from(image_height) * cell_w)
        .div_ceil(u128::from(image_width) * cell_h);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);

    rows.clamp(3.min(max_preview_height), max_preview_height)
}
