use crate::tui::keybindings::{KeymapBindingSummary, SelectionAction};

use super::super::DashboardState;
use super::{ActiveModalPopupKind, KeymapPopupState, ModalPopup};

impl DashboardState {
    pub fn open_keymap_help_popup(&mut self) {
        self.popups
            .set_modal(ModalPopup::KeymapHelp(KeymapPopupState {
                scroll: Default::default(),
            }));
    }

    pub fn close_keymap_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::KeymapHelp) {
            self.popups.clear_modal();
        }
    }

    pub fn keymap_popup_scroll(&self) -> usize {
        self.popups
            .keymap_popup()
            .map(|popup| popup.scroll.scroll())
            .unwrap_or_default()
    }

    pub fn scroll_keymap_popup(&mut self, action: SelectionAction) {
        let Some(popup) = self.popups.keymap_popup_mut() else {
            return;
        };
        match action {
            SelectionAction::Next => popup.scroll.scroll_down(),
            SelectionAction::Previous => popup.scroll.scroll_up(),
        }
    }

    pub fn set_keymap_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = self.popups.keymap_popup_mut() {
            popup.scroll.set_view_height(height);
        }
    }

    pub fn set_keymap_popup_total_lines(&mut self, total_lines: usize) {
        if let Some(popup) = self.popups.keymap_popup_mut() {
            popup.scroll.set_total_lines(total_lines);
        }
    }

    pub fn keymap_binding_summaries(&self) -> Vec<KeymapBindingSummary> {
        self.options.key_bindings.binding_summaries()
    }
}
