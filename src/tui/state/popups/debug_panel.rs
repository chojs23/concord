use crate::tui::keybindings::{PaneFilterAction, SelectionAction};
use crate::tui::media::{MediaCacheStats, SharedMediaCacheStats};

use super::super::DashboardState;
use super::super::pane_filter::PaneFilterState;
use super::{ActiveModalPopupKind, ModalPopup};
use crate::logging::LogLine;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct DebugMediaSnapshot {
    pub previews: MediaCacheStats,
    pub avatars: MediaCacheStats,
    pub emojis: MediaCacheStats,
    pub shared: SharedMediaCacheStats,
    pub active_sources: usize,
    pub source_limit: usize,
    pub protocol: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct DebugLogLine {
    pub entry_id: u64,
    pub source_start: usize,
    pub text: String,
}

#[derive(Debug, Default)]
pub(in crate::tui) struct DebugLogPopupState {
    pub(super) scroll: super::ScrollablePopupState,
    lines: Vec<DebugLogLine>,
    log_entries: Vec<LogLine>,
    following: bool,
    media: Option<DebugMediaSnapshot>,
    filter: Option<PaneFilterState>,
}

impl DashboardState {
    pub fn open_debug_log_popup(&mut self) {
        self.popups
            .set_modal(ModalPopup::DebugLog(DebugLogPopupState {
                following: true,
                ..Default::default()
            }));
    }

    pub fn close_debug_log_popup(&mut self) {
        if let Some(popup) = self.popups.debug_log_popup_mut()
            && popup.filter.take().is_some()
        {
            return;
        }
        if self.is_active_modal_popup(ActiveModalPopupKind::DebugLog) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn open_debug_log_filter(&mut self) {
        if let Some(popup) = self.popups.debug_log_popup_mut() {
            popup
                .filter
                .get_or_insert_with(PaneFilterState::new)
                .start_editing();
        }
    }

    pub(in crate::tui) fn debug_log_filter_query(&self) -> Option<&str> {
        Some(self.popups.debug_log_popup()?.filter.as_ref()?.query())
    }

    pub(in crate::tui) fn debug_log_filter_cursor(&self) -> Option<usize> {
        let filter = self.popups.debug_log_popup()?.filter.as_ref()?;
        filter.is_editing().then(|| filter.cursor_byte_index())
    }

    pub(in crate::tui) fn apply_debug_log_filter_action(&mut self, action: PaneFilterAction) {
        let Some(popup) = self.popups.debug_log_popup_mut() else {
            return;
        };
        let Some(filter) = popup.filter.as_mut() else {
            return;
        };
        match action {
            PaneFilterAction::Close => popup.filter = None,
            PaneFilterAction::Confirm => {
                filter.commit();
                if filter.query().is_empty() {
                    popup.filter = None;
                }
            }
            PaneFilterAction::Select(SelectionAction::Next) => {
                self.move_active_popup_down();
            }
            PaneFilterAction::Select(SelectionAction::Previous) => {
                self.move_active_popup_up();
            }
            PaneFilterAction::DeleteChar => filter.pop_char(),
            PaneFilterAction::MoveCursorLeft => filter.cursor_left(),
            PaneFilterAction::MoveCursorRight => filter.cursor_right(),
            PaneFilterAction::MoveCursorHome => filter.cursor_home(),
            PaneFilterAction::MoveCursorEnd => filter.cursor_end(),
            PaneFilterAction::InsertChar(value) => filter.push_char(value),
            PaneFilterAction::Ignore => {}
        }
    }

    pub(in crate::tui) fn debug_log_lines(&self) -> &[DebugLogLine] {
        self.popups
            .debug_log_popup()
            .map_or(&[], |popup| &popup.lines)
    }

    pub(in crate::tui) fn sync_debug_log_lines(&mut self, lines: Vec<DebugLogLine>, height: usize) {
        let Some(popup) = self.popups.debug_log_popup_mut() else {
            return;
        };
        let anchor = popup
            .lines
            .get(popup.scroll.scroll())
            .map(|line| (line.entry_id, line.source_start));
        // Anchor to retained log identity, not distance from the newest line.
        // Appends and eviction of older entries must not move a paused reader.
        let position = anchor
            .and_then(|(id, source_start)| {
                lines
                    .iter()
                    .rposition(|line| line.entry_id == id && line.source_start <= source_start)
            })
            .unwrap_or_default();
        popup.scroll.set_view_height(height);
        popup.scroll.set_total_lines(lines.len());
        if popup.following {
            popup.scroll.scroll_to_bottom();
        } else {
            popup.scroll.set_scroll(position);
        }
        popup.lines = lines;
    }

    pub(in crate::tui) fn debug_log_scroll(&self) -> usize {
        self.popups
            .debug_log_popup()
            .map_or(0, |popup| popup.scroll.scroll())
    }

    pub(in crate::tui) fn debug_log_following(&self) -> bool {
        self.popups
            .debug_log_popup()
            .is_some_and(|popup| popup.following)
    }

    pub(super) fn update_debug_log_following(&mut self) {
        if let Some(popup) = self.popups.debug_log_popup_mut() {
            popup.following = popup.scroll.is_near_bottom(0);
        }
    }

    pub(in crate::tui) fn jump_debug_log(&mut self, bottom: bool) {
        if let Some(popup) = self.popups.debug_log_popup_mut() {
            if bottom {
                popup.scroll.scroll_to_bottom();
            } else {
                popup.scroll.scroll_to_top();
            }
            popup.following = bottom;
        }
    }

    pub(in crate::tui) fn debug_log_entries(&self) -> &[LogLine] {
        self.popups
            .debug_log_popup()
            .map_or(&[], |popup| &popup.log_entries)
    }

    pub(in crate::tui) fn store_debug_log_tail(&mut self, entries: Vec<LogLine>) -> bool {
        let Some(popup) = self.popups.debug_log_popup_mut() else {
            return false;
        };
        if popup.log_entries == entries {
            return false;
        }
        popup.log_entries = entries;
        true
    }

    pub(in crate::tui) fn debug_media_snapshot(&self) -> Option<&DebugMediaSnapshot> {
        self.popups.debug_log_popup()?.media.as_ref()
    }

    pub(in crate::tui) fn set_debug_media_snapshot(
        &mut self,
        snapshot: DebugMediaSnapshot,
    ) -> bool {
        let Some(popup) = self.popups.debug_log_popup_mut() else {
            return false;
        };
        if popup.media.as_ref() == Some(&snapshot) {
            return false;
        }
        popup.media = Some(snapshot);
        true
    }
}
