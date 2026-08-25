//! Plans dashboard redraw transactions around terminal pixel media.
//!
//! Placement cleanup and animation frame replacement have the same display
//! requirement: the terminal must not present the partially updated pixel layer.
//! Keeping that policy here avoids terminal-, protocol-, and media-format checks in
//! the event loop.

use crossterm::{
    execute,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};
use ratatui::{DefaultTerminal, layout::Rect};
use tokio::time::{Duration, Instant};

use crate::tui::state::DashboardState;

use super::media_runtime::{
    DashboardMediaRuntime, clear_image_surfaces_frame, draw_dashboard_frame,
};

pub(super) const INPUT_REDRAW_INTERVAL: Duration = Duration::from_millis(12);

pub(super) fn schedule_redraw(deadline: &mut Option<Instant>, now: Instant, delay: Duration) {
    let candidate = now + delay;
    if deadline.is_none_or(|current| candidate < current) {
        *deadline = Some(candidate);
    }
}

pub(super) fn redraw_is_due(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_none_or(|deadline| deadline <= now)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RedrawPlan {
    pub(super) clear_stale_media: bool,
    pub(super) synchronized: bool,
}

#[derive(Default)]
pub(super) struct DashboardRedrawState {
    media_animation_pending: bool,
}

impl DashboardRedrawState {
    pub(super) fn request_media_animation(&mut self) {
        self.media_animation_pending = true;
    }

    pub(super) fn take_plan(&mut self, clear_stale_media: bool) -> RedrawPlan {
        let media_animation_pending = std::mem::take(&mut self.media_animation_pending);
        RedrawPlan {
            clear_stale_media,
            synchronized: clear_stale_media || media_animation_pending,
        }
    }
}

pub(super) fn draw_dashboard_transaction(
    terminal: &mut DefaultTerminal,
    state: &mut DashboardState,
    media_runtime: &mut DashboardMediaRuntime,
    last_frame_area: &mut Rect,
    plan: RedrawPlan,
) -> std::io::Result<()> {
    if plan.synchronized {
        let _ = execute!(terminal.backend_mut(), BeginSynchronizedUpdate);
    }

    let draw_result = (|| -> std::io::Result<()> {
        if plan.clear_stale_media {
            terminal.draw(|frame| {
                *last_frame_area = clear_image_surfaces_frame(frame, state, media_runtime);
            })?;
        }
        terminal.draw(|frame| {
            *last_frame_area = draw_dashboard_frame(frame, state, media_runtime);
        })?;
        Ok(())
    })();

    // Ending the transaction is best-effort, like beginning it. It must still be
    // attempted after a draw error so a supporting terminal does not keep showing
    // the previous frame indefinitely.
    if plan.synchronized {
        let _ = execute!(terminal.backend_mut(), EndSynchronizedUpdate);
    }

    draw_result
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::{DashboardRedrawState, RedrawPlan, redraw_is_due, schedule_redraw};

    #[test]
    fn input_redraw_shortens_background_deadline_without_replacing_it_later() {
        let now = Instant::now();
        let mut deadline = None;
        schedule_redraw(&mut deadline, now, Duration::from_millis(80));
        let background_deadline = deadline;

        schedule_redraw(&mut deadline, now, Duration::from_millis(12));
        assert!(deadline < background_deadline);

        schedule_redraw(&mut deadline, now, Duration::from_millis(80));
        assert_eq!(deadline, Some(now + Duration::from_millis(12)));
    }

    #[test]
    fn redraw_deadline_coalesces_events_until_the_due_frame() {
        let now = Instant::now();
        let mut deadline = None;
        schedule_redraw(&mut deadline, now, Duration::from_millis(12));
        schedule_redraw(
            &mut deadline,
            now + Duration::from_millis(4),
            Duration::from_millis(12),
        );

        assert!(!redraw_is_due(deadline, now + Duration::from_millis(11)));
        assert!(redraw_is_due(deadline, now + Duration::from_millis(12)));
    }

    #[test]
    fn redraw_plan_synchronizes_media_animation_and_placement_cleanup() {
        let cases = [
            (
                "static redraw",
                false,
                false,
                RedrawPlan {
                    clear_stale_media: false,
                    synchronized: false,
                },
            ),
            (
                "placement cleanup",
                true,
                false,
                RedrawPlan {
                    clear_stale_media: true,
                    synchronized: true,
                },
            ),
            (
                "media animation",
                false,
                true,
                RedrawPlan {
                    clear_stale_media: false,
                    synchronized: true,
                },
            ),
            (
                "animation with placement cleanup",
                true,
                true,
                RedrawPlan {
                    clear_stale_media: true,
                    synchronized: true,
                },
            ),
        ];

        for (label, clear_stale_media, animated_media_advanced, expected) in cases {
            let mut state = DashboardRedrawState::default();
            if animated_media_advanced {
                state.request_media_animation();
            }

            assert_eq!(state.take_plan(clear_stale_media), expected, "{label}");
            assert_eq!(
                state.take_plan(false),
                RedrawPlan {
                    clear_stale_media: false,
                    synchronized: false,
                },
                "{label} should consume the pending animation redraw"
            );
        }
    }
}
