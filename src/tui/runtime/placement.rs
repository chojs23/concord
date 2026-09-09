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

use crate::tui::media::{ImagePreviewFragmentKey, ImagePreviewTarget};

/// Fingerprint of every overlay image's on-screen geometry for one frame.
///
/// Emoji are intentionally absent: they flow with text, so the cell diff moves
/// them naturally and they are always drawn in both frames (see the run loop).
#[derive(Clone, Default)]
pub(super) struct FramePlacements {
    /// Inline message-pane previews, keyed by their cache key. The value is the
    /// resolved post-clip screen rect (inline) or the centered viewer rect.
    previews: HashMap<ImagePreviewFragmentKey, Rect>,
    /// Message-pane avatars, keyed by (url, absolute row). The value is the
    /// render fingerprint (visible_height, top_clip_rows, circular). The row
    /// is already part of the key, and avatar x and width are constant.
    avatars: HashMap<(String, isize), (u16, u16, bool)>,
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
    pub(super) unchanged_previews: HashSet<ImagePreviewFragmentKey>,
    pub(super) unchanged_avatars: HashSet<(String, isize)>,
    pub(super) popup_avatar_unchanged: bool,
}

impl FramePlacements {
    pub(super) fn insert_preview(&mut self, target: &ImagePreviewTarget, area: Rect) {
        self.previews.insert(target.fragment_key(), area);
    }

    pub(super) fn insert_avatar(&mut self, url: String, row: isize, fingerprint: (u16, u16, bool)) {
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
            selected: false,
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
    fn avatar_change_keeps_message_preview_unchanged() {
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(&target, Rect::new(10, 5, 20, 10));
        previous.insert_avatar("avatar".to_owned(), 4, (3, 0, false));

        // Either scrolling or changing the mask requires clearing only the avatar.
        for (row, circular) in [(3, false), (4, true)] {
            let mut current = FramePlacements::default();
            current.insert_preview(&target, Rect::new(10, 5, 20, 10));
            current.insert_avatar("avatar".to_owned(), row, (3, 0, circular));

            let diff = current.diff(&previous);
            assert!(diff.need_clear);
            assert!(diff.unchanged_previews.contains(&target.fragment_key()));
            assert!(!diff.unchanged_avatars.contains(&("avatar".to_owned(), row)));
        }
    }

    #[test]
    fn placement_diff_tracks_preview_lifecycle() {
        let target = preview_target(1, 0);
        let mut previous = FramePlacements::default();
        previous.insert_preview(&target, Rect::new(10, 5, 20, 10));
        previous.insert_avatar("avatar".to_owned(), 4, (3, 0, false));

        for (name, rect, keep_avatar, need_clear, preview_unchanged) in [
            (
                "vertical scroll",
                Some(Rect::new(10, 3, 20, 10)),
                false,
                true,
                false,
            ),
            (
                "identical frame",
                Some(Rect::new(10, 5, 20, 10)),
                true,
                false,
                true,
            ),
            ("removed preview", None, false, true, false),
        ] {
            let mut current = FramePlacements::default();
            if let Some(rect) = rect {
                current.insert_preview(&target, rect);
            }
            if keep_avatar {
                current.insert_avatar("avatar".to_owned(), 4, (3, 0, false));
            }

            let diff = current.diff(&previous);
            assert_eq!(diff.need_clear, need_clear, "{name}");
            assert_eq!(
                diff.unchanged_previews.contains(&target.fragment_key()),
                preview_unchanged,
                "{name}"
            );
            if rect.is_none() {
                assert!(diff.unchanged_previews.is_empty(), "{name}");
            }
            if keep_avatar {
                assert!(
                    diff.unchanged_avatars.contains(&("avatar".to_owned(), 4)),
                    "{name}"
                );
                assert!(diff.popup_avatar_unchanged, "{name}");
            }
        }
    }

    #[test]
    fn split_preview_fragments_are_tracked_independently() {
        let top = ImagePreviewTarget {
            visible_preview_height: 3,
            ..preview_target(1, 0)
        };
        let bottom = ImagePreviewTarget {
            preview_y_offset_rows: 6,
            visible_preview_height: 4,
            top_clip_rows: 6,
            ..preview_target(1, 0)
        };
        assert_eq!(top.key(), bottom.key());
        assert_ne!(top.fragment_key(), bottom.fragment_key());

        let mut previous = FramePlacements::default();
        previous.insert_preview(&top, Rect::new(10, 5, 20, 3));
        previous.insert_preview(&bottom, Rect::new(10, 11, 20, 4));

        let mut current = FramePlacements::default();
        current.insert_preview(&top, Rect::new(10, 5, 20, 3));
        current.insert_preview(&bottom, Rect::new(10, 10, 20, 4));

        let diff = current.diff(&previous);
        assert!(diff.need_clear);
        assert!(diff.unchanged_previews.contains(&top.fragment_key()));
        assert!(!diff.unchanged_previews.contains(&bottom.fragment_key()));
    }
}
