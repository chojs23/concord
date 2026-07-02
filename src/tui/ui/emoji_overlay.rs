use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui_image::Image as RatatuiImage;

use crate::tui::message::format::EMOJI_REACTION_IMAGE_WIDTH;

use super::types::EmojiImage;

/// Overlays custom-emoji thumbnails on a single-emoji-per-row list. `row_urls`
/// yields each visible row's url top to bottom, `None` for rows with no image.
/// `x_offset` is the column within `area` where the image sits.
pub(in crate::tui::ui) fn overlay_emoji_column<S: AsRef<str>>(
    frame: &mut Frame,
    area: Rect,
    x_offset: u16,
    row_urls: impl Iterator<Item = Option<S>>,
    emoji_images: &[EmojiImage<'_>],
) {
    if area.width <= x_offset || area.height == 0 {
        return;
    }
    let width = EMOJI_REACTION_IMAGE_WIDTH.min(area.width.saturating_sub(x_offset));
    if width == 0 {
        return;
    }
    for (offset, url) in row_urls.enumerate() {
        let Some(url) = url else {
            continue;
        };
        let Some(image) = emoji_images
            .iter()
            .find(|image| image.url.as_str() == url.as_ref())
        else {
            continue;
        };
        let y = area
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
        if y >= area.y.saturating_add(area.height) {
            continue;
        }
        frame.render_widget(
            RatatuiImage::new(image.protocol),
            Rect {
                x: area.x.saturating_add(x_offset),
                y,
                width,
                height: 1,
            },
        );
    }
}

/// A custom-emoji image to place at a (row, col) on a message-style list.
pub(in crate::tui::ui) struct EmojiSlot {
    /// Row within the list, relative to its top.
    pub(in crate::tui::ui) row_in_list: isize,
    /// Absolute column of the image's left edge.
    pub(in crate::tui::ui) col: isize,
    /// Upper bound on the width before the list-edge clamp. `u16::MAX` for no
    /// extra bound, or the remaining card width where content is narrower.
    pub(in crate::tui::ui) max_width: u16,
    pub(in crate::tui::ui) url: String,
}

/// Overlays custom-emoji thumbnails placed by (row, col) within `list`, skipping
/// any that fall outside it or intersect `occlusion_areas`.
pub(in crate::tui::ui) fn overlay_emoji_slots(
    frame: &mut Frame,
    list: Rect,
    emoji_images: &[EmojiImage<'_>],
    occlusion_areas: &[Rect],
    slots: impl Iterator<Item = EmojiSlot>,
) {
    if emoji_images.is_empty() || list.height == 0 {
        return;
    }
    let list_right = list.x as isize + list.width as isize;
    for slot in slots {
        if slot.row_in_list < 0
            || slot.row_in_list >= list.height as isize
            || slot.col < 0
            || slot.col >= list_right
        {
            continue;
        }
        let Some(image) = emoji_images.iter().find(|image| image.url == slot.url) else {
            continue;
        };
        let remaining = (list_right - slot.col).max(0) as u16;
        let width = EMOJI_REACTION_IMAGE_WIDTH
            .min(slot.max_width)
            .min(remaining);
        if width == 0 {
            continue;
        }
        let image_area = Rect {
            x: slot.col as u16,
            y: (list.y as isize + slot.row_in_list) as u16,
            width,
            height: 1,
        };
        if intersects_any(image_area, occlusion_areas) {
            continue;
        }
        frame.render_widget(RatatuiImage::new(image.protocol), image_area);
    }
}

pub(in crate::tui::ui) fn intersects_any(area: Rect, occlusion_areas: &[Rect]) -> bool {
    occlusion_areas
        .iter()
        .any(|occlusion| rects_intersect(area, *occlusion))
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && a.x < b.x.saturating_add(b.width)
        && b.x < a.x.saturating_add(a.width)
        && a.y < b.y.saturating_add(b.height)
        && b.y < a.y.saturating_add(a.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_any_boundaries() {
        let a = Rect::new(2, 2, 2, 1);
        // Edge-adjacent rects touch but must not count as occluding.
        let right = Rect::new(4, 2, 2, 1);
        let below = Rect::new(2, 3, 2, 1);
        assert!(!intersects_any(a, &[right, below]));
        // An overlapping rect does occlude.
        assert!(intersects_any(a, &[Rect::new(3, 2, 2, 1)]));
        // Empty rects never occlude, even inside the range.
        assert!(!intersects_any(a, &[Rect::new(2, 2, 0, 1)]));
    }
}
