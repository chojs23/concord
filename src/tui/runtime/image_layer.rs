//! Skip-only image emission control.
//!
//! Terminal graphics protocols (iTerm2, Kitty, Sixel) are emitted by
//! `ratatui-image` as a single giant escape string stuffed into one buffer
//! cell, with the rest of the image area marked `skip`. ratatui's frame diff
//! decides whether to re-emit a cell using a `symbol().width()` accumulator
//! that was designed for multi-width CJK/emoji text. Because an image cell's
//! computed width is enormous, that accumulator behaves erratically around
//! images: an image re-emits whenever *anything* near it in the buffer changes,
//! not only when the image itself changes. Each iTerm2 re-emit runs an internal
//! erase-then-redraw sequence, which is the flicker.
//!
//! We therefore take ownership of the "should this image be emitted" decision
//! instead of trusting the diff. For every image surface we remember the cell
//! rect it occupied and a hash of its visual content. On the next frame:
//!
//! * unchanged surface (same rect, same content) -> mark every cell in the rect
//!   `skip` and write nothing. The diff leaves those cells untouched, so the
//!   terminal keeps showing the already-drawn image. Zero re-emission, zero
//!   flicker. As a bonus, steady-state frames carry no giant symbols at all, so
//!   the diff accumulator stays clean for the surrounding text too.
//! * new / moved / changed / re-shown surface -> render the real protocol so the
//!   terminal updates it.
//!
//! ## Lifetime and threading
//!
//! The persistent tracker is owned by the run loop (so it survives across
//! frames even if the async task migrates between Tokio worker threads). It is
//! `install`ed into a thread-local only for the duration of one synchronous
//! `terminal.draw` call and `uninstall`ed immediately after. The widget
//! wrappers consult that thread-local while ratatui renders. When no tracker is
//! installed (e.g. unit tests rendering into a `TestBackend`) the wrappers fall
//! back to plain rendering, so behaviour is unchanged there.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Write as _};
use std::hash::Hasher as _;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{StatefulWidget, Widget},
};
use ratatui_image::{
    Image, Resize, StatefulImage,
    protocol::{Protocol, StatefulProtocol},
};

/// An anchor cell still holding an image is either skipped (we left it alone)
/// or carries the protocol escape sequence, which is far longer than any normal
/// text grapheme. A shorter, non-skipped anchor means another widget (a popup,
/// say) drew over the image this frame, so the surface was occluded.
const IMAGE_ANCHOR_MIN_SYMBOL_LEN: usize = 64;

#[derive(Clone, Copy)]
struct Emitted {
    area: Rect,
    content_hash: u64,
}

/// Per-frame record of which image surfaces were emitted and where, used to
/// decide on the next frame whether each surface can be skipped.
#[derive(Default)]
pub(in crate::tui) struct ImageEmissionTracker {
    /// The terminal area of the previous frame. A change means the terminal was
    /// resized (ratatui clears the screen), so every surface must re-render.
    last_area: Option<Rect>,
    /// Surfaces emitted on the previous frame, keyed by their top-left cell.
    emitted: HashMap<(u16, u16), Emitted>,
    /// Surfaces decided on the current frame; promoted to `emitted` at frame end.
    current: HashMap<(u16, u16), Emitted>,
}

enum Decision {
    Render,
    Skip,
}

impl ImageEmissionTracker {
    fn begin_frame(&mut self, area: Rect) {
        if self.last_area != Some(area) {
            // Terminal resized (or first frame): the screen was cleared, so we
            // cannot rely on any retained image. Force every surface to render.
            self.emitted.clear();
            self.last_area = Some(area);
        }
        self.current.clear();
    }

    fn decide(&mut self, area: Rect, content_hash: u64) -> Decision {
        let key = (area.x, area.y);
        let unchanged = self
            .emitted
            .get(&key)
            .is_some_and(|prev| prev.area == area && prev.content_hash == content_hash);
        self.current.insert(key, Emitted { area, content_hash });
        if unchanged {
            Decision::Skip
        } else {
            Decision::Render
        }
    }

    fn end_frame(&mut self, buf: &Buffer) {
        // Drop any surface that a later widget (e.g. a popup) drew over this
        // frame. An intact surface leaves every cell either skipped or, for its
        // anchor, carrying the long protocol escape; an overwritten cell holds
        // ordinary short text. If we kept an occluded surface we would skip
        // re-emitting it once the occluder went away, leaving stale pixels.
        // Forgetting it forces a clean re-render on the next frame.
        self.current.retain(|_, emitted| surface_intact(emitted.area, buf));
        std::mem::swap(&mut self.emitted, &mut self.current);
        // `current` now holds last frame's map; cleared again at next begin_frame.
    }

    /// Drop all retained surfaces so the next frame re-renders everything. Used
    /// when something outside our control may have wiped the screen.
    pub(in crate::tui) fn invalidate(&mut self) {
        self.emitted.clear();
        self.last_area = None;
    }
}

/// Hash a value's `Debug` representation into a stable content fingerprint.
///
/// Used to fingerprint an image surface's visual inputs (image identity plus
/// the render parameters that determine its pixels) without having to add
/// `Hash` derives across many unrelated types. The same `Debug` output yields
/// the same hash every frame, so an unchanged surface keeps the same hash.
pub(in crate::tui) fn content_hash<T: fmt::Debug>(value: &T) -> u64 {
    struct DebugHasher(DefaultHasher);
    impl fmt::Write for DebugHasher {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            self.0.write(value.as_bytes());
            Ok(())
        }
    }
    let mut hasher = DebugHasher(DefaultHasher::new());
    write!(hasher, "{value:?}").expect("writing into content hasher cannot fail");
    hasher.0.finish()
}

thread_local! {
    static TRACKER: RefCell<Option<ImageEmissionTracker>> = const { RefCell::new(None) };
}

/// Move the run loop's tracker into the thread-local for the duration of one
/// synchronous draw. Pair with [`uninstall`].
pub(in crate::tui) fn install(tracker: ImageEmissionTracker) {
    TRACKER.with(|slot| *slot.borrow_mut() = Some(tracker));
}

/// Take the tracker back out after the draw so its state lives on the run
/// loop's stack rather than in thread-local storage.
pub(in crate::tui) fn uninstall() -> ImageEmissionTracker {
    TRACKER.with(|slot| slot.borrow_mut().take().unwrap_or_default())
}

/// Start a frame; call once at the top of the draw with the full terminal area.
pub(in crate::tui) fn begin_frame(area: Rect) {
    TRACKER.with(|slot| {
        if let Some(tracker) = slot.borrow_mut().as_mut() {
            tracker.begin_frame(area);
        }
    });
}

/// Finish a frame; call once at the end of the draw with the composed buffer so
/// occluded surfaces can be detected.
pub(in crate::tui) fn end_frame(buf: &Buffer) {
    TRACKER.with(|slot| {
        if let Some(tracker) = slot.borrow_mut().as_mut() {
            tracker.end_frame(buf);
        }
    });
}

/// Returns `true` when the caller should render the real image, `false` when
/// the surface is unchanged (in which case its cells are marked `skip` here so
/// ratatui leaves the already-drawn image alone). With no tracker installed it
/// always returns `true`.
fn should_render(area: Rect, content_hash: u64, buf: &mut Buffer) -> bool {
    TRACKER.with(|slot| match slot.borrow_mut().as_mut() {
        Some(tracker) => match tracker.decide(area, content_hash) {
            Decision::Render => true,
            Decision::Skip => {
                mark_skip(area, buf);
                false
            }
        },
        None => true,
    })
}

fn mark_skip(area: Rect, buf: &mut Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_skip(true);
            }
        }
    }
}

/// True while every cell of `area` is still either skipped or carrying an image
/// escape, i.e. nothing has been drawn over the surface this frame.
fn surface_intact(area: Rect, buf: &Buffer) -> bool {
    (area.top()..area.bottom()).all(|y| {
        (area.left()..area.right()).all(|x| {
            buf.cell((x, y))
                .is_some_and(|cell| cell.skip || cell.symbol().len() >= IMAGE_ANCHOR_MIN_SYMBOL_LEN)
        })
    })
}

/// Drop-in replacement for [`ratatui_image::Image`] that emits the image only
/// when its `(area, content_hash)` differs from the previous frame.
pub(in crate::tui) struct TrackedImage<'a> {
    protocol: &'a Protocol,
    content_hash: u64,
}

impl<'a> TrackedImage<'a> {
    pub(in crate::tui) fn new(protocol: &'a Protocol, content_hash: u64) -> Self {
        Self {
            protocol,
            content_hash,
        }
    }
}

impl Widget for TrackedImage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if should_render(area, self.content_hash, buf) {
            Image::new(self.protocol).render(area, buf);
        }
    }
}

/// Drop-in replacement for [`ratatui_image::StatefulImage`] with `Resize::Fit`
/// that emits (and resize-encodes) only when the surface changed. When skipped
/// it does not touch the protocol state, avoiding a needless re-encode.
pub(in crate::tui) struct TrackedStatefulImage {
    content_hash: u64,
}

impl TrackedStatefulImage {
    pub(in crate::tui) fn new(content_hash: u64) -> Self {
        Self { content_hash }
    }
}

impl StatefulWidget for TrackedStatefulImage {
    type State = StatefulProtocol;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if should_render(area, self.content_hash, buf) {
            StatefulImage::new().resize(Resize::Fit(None)).render(area, buf, state);
        }
    }
}
