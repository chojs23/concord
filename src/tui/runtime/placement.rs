//! Per-image placement tracking for the selective image-clear frame.
//!
//! Terminal graphics (kitty/iTerm2/sixel) live on a pixel layer the ratatui cell
//! diff cannot erase on its own, so a moved or removed image leaves a ghost
//! unless its old cells are overpainted first. Rather than clear every image
//! globally (which makes unchanged images flicker), we fingerprint where each
//! overlay image sits on screen this frame and compare against the previous
//! frame. Only images whose fingerprint changed or disappeared need the
//! erase-then-redraw pass; everything else is left untouched and emits no
//! terminal output.
//!
//! A "placement" is the resolved absolute screen geometry of one image. It must
//! include absolute screen position: the same `message_index` lands on a
//! different row after a scroll, so relative target fields alone would miss real
//! movement.

use std::collections::{HashMap, HashSet};

use ratatui::layout::Rect;

use crate::tui::media::ImagePreviewKey;

/// Fingerprint of every overlay image's on-screen geometry for one frame.
///
/// Emoji are intentionally absent: they flow with text, so the cell diff moves
/// them naturally and they are always drawn in both frames (see the run loop).
#[derive(Clone, Default)]
pub(super) struct FramePlacements {
    /// Inline message-pane previews, keyed by their cache key. The value is the
    /// resolved post-clip screen rect (inline) or the centered viewer rect.
    previews: HashMap<ImagePreviewKey, Rect>,
    /// Message-pane avatars, keyed by (url, absolute row). The value is the
    /// vertical fingerprint (row, visible_height, top_clip_rows); avatar x and
    /// width are constant.
    avatars: HashMap<(String, isize), (isize, u16, u16)>,
    /// Profile popup avatar, when shown: (url, circular, area).
    popup_avatar: Option<(String, bool, Rect)>,
}

/// Which images survived unchanged from the previous frame, plus whether any
/// clear pass is needed at all. The clear frame draws only the unchanged
/// overlays so their stale pixels are preserved; changed/removed overlays are
/// omitted so their old cells get overpainted.
#[derive(Default)]
pub(super) struct PlacementDiff {
    pub(super) need_clear: bool,
    pub(super) unchanged_previews: HashSet<ImagePreviewKey>,
    pub(super) unchanged_avatars: HashSet<(String, isize)>,
    pub(super) popup_avatar_unchanged: bool,
}

impl FramePlacements {
    pub(super) fn insert_preview(&mut self, key: ImagePreviewKey, area: Rect) {
        self.previews.insert(key, area);
    }

    pub(super) fn insert_avatar(
        &mut self,
        url: String,
        row: isize,
        fingerprint: (isize, u16, u16),
    ) {
        self.avatars.insert((url, row), fingerprint);
    }

    pub(super) fn set_popup_avatar(&mut self, popup: Option<(String, bool, Rect)>) {
        self.popup_avatar = popup;
    }

    /// Compare this frame's placements against the previous frame's. An overlay
    /// is "unchanged" when it exists in both with an identical fingerprint;
    /// those are the only ones the clear frame keeps drawing. `need_clear` is
    /// set when anything changed, was added in a moved position, or was removed,
    /// so the erase pass runs to overpaint stale pixels.
    pub(super) fn diff(&self, previous: &FramePlacements) -> PlacementDiff {
        let mut diff = PlacementDiff::default();

        for (key, area) in &self.previews {
            if previous.previews.get(key) == Some(area) {
                diff.unchanged_previews.insert(key.clone());
            } else {
                diff.need_clear = true;
            }
        }
        for (key, fingerprint) in &self.avatars {
            if previous.avatars.get(key) == Some(fingerprint) {
                diff.unchanged_avatars.insert(key.clone());
            } else {
                diff.need_clear = true;
            }
        }

        // Anything in the previous frame that is gone now must be cleared.
        if previous
            .previews
            .keys()
            .any(|key| !self.previews.contains_key(key))
            || previous
                .avatars
                .keys()
                .any(|key| !self.avatars.contains_key(key))
        {
            diff.need_clear = true;
        }

        if self.popup_avatar == previous.popup_avatar {
            diff.popup_avatar_unchanged = true;
        } else {
            diff.need_clear = true;
        }

        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::ids::{Id, marker::MessageMarker};
    use crate::tui::media::ImagePreviewTarget;

    fn preview_target(message_id: u64, y_offset: usize) -> ImagePreviewTarget {
        ImagePreviewTarget {
            viewer: false,
            thread_card: false,
            message_index: 0,
            preview_index: 0,
            body_line_index: None,
            preview_x_offset_columns: 0,
            preview_y_offset_rows: y_offset,
            preview_width: 20,
            preview_height: 10,
            visible_preview_height: 10,
            top_clip_rows: 0,
            accent_color: None,
            show_play_marker: false,
            message_id: Id::<MessageMarker>::new(message_id),
            url: "https://cdn.discordapp.com/image.png".to_owned(),
            filename: "image.png".to_owned(),
        }
    }

    #[test]
    fn member_pane_only_change_keeps_message_preview_unchanged() {
        // A message preview at a fixed screen rect, with one avatar that moves.
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(target.key(), Rect::new(10, 5, 20, 10));
        previous.insert_avatar("avatar".to_owned(), 4, (4, 3, 0));

        let mut current = FramePlacements::default();
        current.insert_preview(target.key(), Rect::new(10, 5, 20, 10));
        // The member-pane scroll moved the avatar by one row.
        current.insert_avatar("avatar".to_owned(), 3, (3, 3, 0));

        let diff = current.diff(&previous);
        // The preview never moved, so it stays drawn in the clear frame and never
        // re-emits. The avatar moved, so a clear pass runs.
        assert!(diff.need_clear);
        assert!(diff.unchanged_previews.contains(&target.key()));
        assert!(!diff.unchanged_avatars.contains(&("avatar".to_owned(), 3)));
    }

    #[test]
    fn vertical_scroll_changes_preview_placement() {
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(target.key(), Rect::new(10, 5, 20, 10));

        let mut current = FramePlacements::default();
        // Same image, same key, but a vertical scroll moved its screen rect.
        current.insert_preview(target.key(), Rect::new(10, 3, 20, 10));

        let diff = current.diff(&previous);
        assert!(diff.need_clear);
        assert!(!diff.unchanged_previews.contains(&target.key()));
    }

    #[test]
    fn identical_frame_needs_no_clear() {
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(target.key(), Rect::new(10, 5, 20, 10));
        previous.insert_avatar("avatar".to_owned(), 4, (4, 3, 0));

        let mut current = FramePlacements::default();
        current.insert_preview(target.key(), Rect::new(10, 5, 20, 10));
        current.insert_avatar("avatar".to_owned(), 4, (4, 3, 0));

        let diff = current.diff(&previous);
        assert!(!diff.need_clear);
        assert!(diff.unchanged_previews.contains(&target.key()));
        assert!(diff.unchanged_avatars.contains(&("avatar".to_owned(), 4)));
        assert!(diff.popup_avatar_unchanged);
    }

    #[test]
    fn removed_preview_forces_clear() {
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(target.key(), Rect::new(10, 5, 20, 10));

        let current = FramePlacements::default();
        let diff = current.diff(&previous);
        assert!(diff.need_clear);
        assert!(diff.unchanged_previews.is_empty());
    }
}
