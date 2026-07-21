//! Shared ASCII loading art for TUI surfaces.
//!
//! Callers provide the animation frame so rendering stays deterministic in tests
//! and each runtime can decide when redraws are worth scheduling.

use std::time::Duration;

use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub(in crate::tui) const LOADING_ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(180);

const LABEL_COLUMN: usize = 15;
// Every frame keeps the same three-row footprint so animated loaders do not
// move surrounding popup content while the character walks.
const CHARACTER_FRAMES: [[&str; 3]; 4] = [
    ["  /\\_/\\", " ( o.o )", " /|_[]_|\\"],
    ["   /\\_/\\", "  ( o.o )", "  \\|_[]_|/"],
    ["    /\\_/\\", "   ( -.- )", "  _/|_[]_|\\_"],
    ["   /\\_/\\", "  ( o.o )", "  \\|_[]_|/"],
];

pub(in crate::tui) struct AsciiLoadingIndicator {
    label: String,
    style: Style,
}

impl AsciiLoadingIndicator {
    pub(in crate::tui) fn new(label: impl Into<String>, style: Style) -> Self {
        Self {
            label: label.into(),
            style,
        }
    }

    pub(in crate::tui) const fn height(&self) -> usize {
        CHARACTER_FRAMES[0].len()
    }

    pub(in crate::tui) fn lines(&self, animation_frame: usize) -> [Line<'static>; 3] {
        let frame = CHARACTER_FRAMES[animation_frame % CHARACTER_FRAMES.len()];
        [
            self.line(frame[0]),
            self.line(format!("{:<LABEL_COLUMN$}{}", frame[1], self.label)),
            self.line(frame[2]),
        ]
    }

    fn line(&self, content: impl Into<String>) -> Line<'static> {
        Line::from(Span::styled(content.into(), self.style))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn loading_indicator_animates_and_cycles_without_changing_height() {
        let indicator = AsciiLoadingIndicator::new("Working...", Style::default());
        let frames = (0..CHARACTER_FRAMES.len())
            .map(|frame| text(&indicator.lines(frame)))
            .collect::<Vec<_>>();

        assert!(frames.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(frames.iter().all(|frame| frame.len() == indicator.height()));
        assert!(frames.iter().all(|frame| frame[1].contains("Working...")));
        assert_eq!(frames[0], text(&indicator.lines(CHARACTER_FRAMES.len())));
    }
}
