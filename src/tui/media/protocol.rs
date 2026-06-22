use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use ratatui::layout::Size;
use ratatui_image::{
    Resize,
    picker::{Picker, ProtocolType},
};

use crate::{config::ImageProtocolPreference, logging};

pub(super) const AVATAR_PREVIEW_WIDTH: u16 = 4;
pub(super) const AVATAR_PREVIEW_HEIGHT: u16 = 2;
pub(in crate::tui) const PROFILE_POPUP_AVATAR_WIDTH: u16 = 8;
pub(in crate::tui) const PROFILE_POPUP_AVATAR_HEIGHT: u16 = 4;
const AVATAR_SOURCE_PIXELS_PER_COLUMN: u64 = 10;
const AVATAR_SOURCE_PIXELS_PER_ROW: u64 = AVATAR_SOURCE_PIXELS_PER_COLUMN * 3;
const DISCORD_AVATAR_CDN_PREFIX: &str = "https://cdn.discordapp.com/avatars/";
const DISCORD_AVATAR_MIN_SIZE: u64 = 16;
const DISCORD_AVATAR_MAX_SIZE: u64 = 1024;
pub(super) const EMOJI_REACTION_THUMB_WIDTH: u16 = 2;
pub(super) const EMOJI_REACTION_THUMB_HEIGHT: u16 = 1;

pub(in crate::tui) fn query_image_picker(
    target: &str,
    unavailable_message: &str,
    protocol_preference: ImageProtocolPreference,
) -> Option<Picker> {
    match Picker::from_query_stdio() {
        Ok(mut picker) => {
            apply_protocol_preference(&mut picker, protocol_preference);
            Some(picker)
        }
        Err(error) => {
            logging::error(target, format!("{unavailable_message}: {error}"));
            None
        }
    }
}

fn apply_protocol_preference(picker: &mut Picker, protocol_preference: ImageProtocolPreference) {
    if let Some(protocol_type) =
        protocol_type_for_preference(protocol_preference, is_iterm_terminal())
    {
        picker.set_protocol_type(protocol_type);
    }
}

fn protocol_type_for_preference(
    protocol_preference: ImageProtocolPreference,
    iterm_terminal: bool,
) -> Option<ProtocolType> {
    match protocol_preference {
        // iTerm2 answers ratatui-image's Kitty capability query, but its Kitty
        // implementation does not support the unicode-placeholder mode used by
        // ratatui-image. Prefer the native iTerm2 protocol when auto-detecting
        // inside iTerm so images render instead of selecting a broken Kitty path.
        ImageProtocolPreference::Auto if iterm_terminal => Some(ProtocolType::Iterm2),
        ImageProtocolPreference::Auto => None,
        ImageProtocolPreference::Iterm2 => Some(ProtocolType::Iterm2),
        ImageProtocolPreference::Kitty => Some(ProtocolType::Kitty),
        ImageProtocolPreference::Sixel => Some(ProtocolType::Sixel),
        ImageProtocolPreference::Halfblocks => Some(ProtocolType::Halfblocks),
    }
}

fn is_iterm_terminal() -> bool {
    is_iterm_terminal_values(
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var("LC_TERMINAL").ok().as_deref(),
    )
}

fn is_iterm_terminal_values(term_program: Option<&str>, lc_terminal: Option<&str>) -> bool {
    term_program.is_some_and(|value| value.contains("iTerm"))
        || lc_terminal.is_some_and(|value| value.contains("iTerm"))
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
pub(in crate::tui) struct ImagePreviewRenderInfo {
    pub(super) viewer: bool,
    pub(super) message_index: usize,
    pub(super) preview_x_offset_columns: u16,
    pub(super) preview_y_offset_rows: usize,
    pub(super) preview_width: u16,
    pub(super) preview_height: u16,
    pub(super) visible_preview_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) accent_color: Option<u32>,
    pub(super) show_play_marker: bool,
    pub(super) mask_circular: bool,
}

pub(in crate::tui) fn fixed_image_preview_render_info(
    preview_width: u16,
    preview_height: u16,
) -> ImagePreviewRenderInfo {
    ImagePreviewRenderInfo {
        viewer: false,
        message_index: 0,
        preview_x_offset_columns: 0,
        preview_y_offset_rows: 0,
        preview_width,
        preview_height,
        visible_preview_height: preview_height,
        top_clip_rows: 0,
        accent_color: None,
        show_play_marker: false,
        mask_circular: false,
    }
}

/// `Picker::font_size` returns a `FontSize` struct as of ratatui-image 11; the
/// rest of our pixel math works in `(width, height)` tuples, so convert here.
pub(super) fn picker_font_size(picker: &Picker) -> (u16, u16) {
    let font_size = picker.font_size();
    (font_size.width, font_size.height)
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

    let mut fitted = fit_image_to_canvas(image, full_width, full_height);
    if render_info.show_play_marker {
        apply_video_play_marker(&mut fitted);
    }
    let mut cropped = fitted.crop_imm(0, crop_top, full_width, crop_height);
    if render_info.mask_circular {
        apply_circular_alpha_mask(&mut cropped, full_width, full_height, crop_top);
    }
    Some(cropped)
}

fn apply_video_play_marker(image: &mut DynamicImage) {
    let mut rgba = image.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let min_dimension = width.min(height);
    if min_dimension < 24 {
        return;
    }

    let cx = width as f32 / 2.0 - 0.5;
    let cy = height as f32 / 2.0 - 0.5;
    let radius = (min_dimension as f32 * 0.14).clamp(10.0, 56.0);
    let radius_sq = radius * radius;

    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let dx = x as f32 - cx;
        let dy = y as f32 - cy;
        if dx * dx + dy * dy <= radius_sq {
            blend_pixel(pixel, Rgba([0, 0, 0, 135]));
        }
    }

    let left = cx - radius * 0.24;
    let right = cx + radius * 0.42;
    let top = cy - radius * 0.42;
    let bottom = cy + radius * 0.42;
    let triangle_min_x = left.floor().max(0.0) as u32;
    let triangle_max_x = right.ceil().min(width.saturating_sub(1) as f32) as u32;
    let triangle_min_y = top.floor().max(0.0) as u32;
    let triangle_max_y = bottom.ceil().min(height.saturating_sub(1) as f32) as u32;

    for y in triangle_min_y..=triangle_max_y {
        let vertical = if y as f32 <= cy {
            ((y as f32 - top) / (cy - top)).clamp(0.0, 1.0)
        } else {
            ((bottom - y as f32) / (bottom - cy)).clamp(0.0, 1.0)
        };
        let row_left = left;
        let row_right = left + (right - left) * vertical;
        for x in triangle_min_x..=triangle_max_x {
            let xf = x as f32;
            if xf >= row_left && xf <= row_right {
                blend_pixel(rgba.get_pixel_mut(x, y), Rgba([245, 247, 250, 230]));
            }
        }
    }

    *image = DynamicImage::ImageRgba8(rgba);
}

fn blend_pixel(pixel: &mut Rgba<u8>, overlay: Rgba<u8>) {
    let alpha = u16::from(overlay.0[3]);
    let inverse_alpha = 255u16.saturating_sub(alpha);
    for channel in 0..3 {
        pixel.0[channel] = ((u16::from(overlay.0[channel]) * alpha
            + u16::from(pixel.0[channel]) * inverse_alpha
            + 127)
            / 255) as u8;
    }
    pixel.0[3] = pixel.0[3].max(overlay.0[3]);
}

/// Zeroes the alpha channel for pixels outside the circle inscribed in the
/// full (uncropped) image bounds. The mask is computed against the full image
/// because vertical clipping (`top_clip_rows`) shifts the crop window, but the
/// circle should stay anchored to the original avatar — otherwise scrolling
/// would deform it.
fn apply_circular_alpha_mask(
    image: &mut DynamicImage,
    full_width: u32,
    full_height: u32,
    crop_top: u32,
) {
    let mut rgba = image.to_rgba8();
    let cx = full_width as f32 / 2.0 - 0.5;
    let cy = full_height as f32 / 2.0 - 0.5;
    let radius = (full_width.min(full_height) as f32 / 2.0) - 0.5;
    let radius_sq = radius * radius;
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        let dx = x as f32 - cx;
        let dy = (y + crop_top) as f32 - cy;
        if dx * dx + dy * dy > radius_sq {
            pixel.0[3] = 0;
        }
    }
    *image = DynamicImage::ImageRgba8(rgba);
}

pub(in crate::tui) fn clipped_preview_protocol(
    picker: &Picker,
    image: &DynamicImage,
    render_info: ImagePreviewRenderInfo,
) -> Option<ratatui_image::protocol::Protocol> {
    let image = clipped_preview_image(image, picker_font_size(picker), render_info)?;
    picker
        .new_protocol(
            image,
            Size::new(
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
    let x_offset = width.saturating_sub(resized.width()) / 2;
    let y_offset = height.saturating_sub(resized.height()) / 2;
    image::imageops::overlay(
        &mut canvas,
        &resized,
        i64::from(x_offset),
        i64::from(y_offset),
    );
    canvas
}

pub(super) fn emoji_protocol(
    picker: &Picker,
    img: DynamicImage,
) -> Option<ratatui_image::protocol::Protocol> {
    let (font_width, font_height) = picker_font_size(picker);
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
            Size::new(EMOJI_REACTION_THUMB_WIDTH, EMOJI_REACTION_THUMB_HEIGHT),
            Resize::Fit(None),
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::{is_iterm_terminal_values, protocol_type_for_preference};
    use crate::config::ImageProtocolPreference;
    use ratatui_image::picker::ProtocolType;

    #[test]
    fn auto_protocol_forces_iterm2_inside_iterm() {
        assert_eq!(
            protocol_type_for_preference(ImageProtocolPreference::Auto, true),
            Some(ProtocolType::Iterm2)
        );
        assert_eq!(
            protocol_type_for_preference(ImageProtocolPreference::Auto, false),
            None
        );
    }

    #[test]
    fn explicit_protocol_preference_overrides_terminal_detection() {
        let cases = [
            (ImageProtocolPreference::Iterm2, ProtocolType::Iterm2),
            (ImageProtocolPreference::Kitty, ProtocolType::Kitty),
            (ImageProtocolPreference::Sixel, ProtocolType::Sixel),
            (
                ImageProtocolPreference::Halfblocks,
                ProtocolType::Halfblocks,
            ),
        ];

        for (preference, expected) in cases {
            assert_eq!(
                protocol_type_for_preference(preference, true),
                Some(expected)
            );
            assert_eq!(
                protocol_type_for_preference(preference, false),
                Some(expected)
            );
        }
    }

    #[test]
    fn iterm_detection_accepts_term_program_or_lc_terminal() {
        assert!(is_iterm_terminal_values(Some("iTerm.app"), None));
        assert!(is_iterm_terminal_values(None, Some("iTerm2")));
        assert!(!is_iterm_terminal_values(Some("WezTerm"), None));
    }
}
