use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::{Resize, picker::Picker};

use crate::logging;

pub(super) const AVATAR_PREVIEW_WIDTH: u16 = 2;
pub(super) const AVATAR_PREVIEW_HEIGHT: u16 = 2;
pub(super) const PROFILE_POPUP_AVATAR_WIDTH: u16 = 8;
pub(super) const PROFILE_POPUP_AVATAR_HEIGHT: u16 = 4;
const AVATAR_SOURCE_PIXELS_PER_COLUMN: u64 = 10;
const AVATAR_SOURCE_PIXELS_PER_ROW: u64 = AVATAR_SOURCE_PIXELS_PER_COLUMN * 3;
const DISCORD_AVATAR_CDN_PREFIX: &str = "https://cdn.discordapp.com/avatars/";
const DISCORD_AVATAR_MIN_SIZE: u64 = 16;
const DISCORD_AVATAR_MAX_SIZE: u64 = 1024;
pub(super) const EMOJI_REACTION_THUMB_WIDTH: u16 = 2;
pub(super) const EMOJI_REACTION_THUMB_HEIGHT: u16 = 1;

pub(super) fn query_image_picker(target: &str, unavailable_message: &str) -> Option<Picker> {
    match Picker::from_query_stdio() {
        Ok(picker) => Some(picker),
        Err(error) => {
            logging::error(target, format!("{unavailable_message}: {error}"));
            None
        }
    }
}

pub(super) fn avatar_preview_url(url: &str, width_columns: u16, height_rows: u16) -> String {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ImagePreviewRenderInfo {
    pub(super) viewer: bool,
    pub(super) message_index: usize,
    pub(super) preview_x_offset_columns: u16,
    pub(super) preview_y_offset_rows: usize,
    pub(super) preview_width: u16,
    pub(super) preview_height: u16,
    pub(super) preview_overflow_count: usize,
    pub(super) visible_preview_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) accent_color: Option<u32>,
}

pub(super) fn clipped_preview_image(
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

pub(super) fn clipped_preview_protocol(
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

pub(super) fn emoji_protocol(
    picker: &Picker,
    img: DynamicImage,
) -> Option<ratatui_image::protocol::Protocol> {
    let (font_width, font_height) = picker.font_size();
    let canvas_w = u32::from(EMOJI_REACTION_THUMB_WIDTH) * u32::from(font_width);
    let canvas_h = u32::from(font_height);

    let max_h = (canvas_h * 3 / 4).max(1);
    let scaled = img.resize(canvas_w, max_h, FilterType::Lanczos3);
    let scaled_rgba = scaled.to_rgba8();

    let x_off = ((canvas_w.saturating_sub(scaled_rgba.width())) / 2) as i64;
    let y_off = ((canvas_h.saturating_sub(scaled_rgba.height())) / 2) as i64;

    let mut canvas = image::RgbaImage::new(canvas_w, canvas_h);
    image::imageops::overlay(&mut canvas, &scaled_rgba, x_off, y_off);

    picker
        .new_protocol(
            DynamicImage::ImageRgba8(canvas),
            Rect::new(
                0,
                0,
                EMOJI_REACTION_THUMB_WIDTH,
                EMOJI_REACTION_THUMB_HEIGHT,
            ),
            Resize::Fit(None),
        )
        .ok()
}
