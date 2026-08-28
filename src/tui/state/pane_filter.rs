use crate::discord::AppCommand;
use crate::tui::text_input::TextInputState;

use super::{DashboardState, FocusPane};

#[derive(Debug, Default)]
pub(super) struct PaneFilterState {
    pub(super) query: TextInputState,
    editing: bool,
}

impl DashboardState {
    pub fn has_active_pane_filter(&self) -> bool {
        self.is_pane_filter_active(FocusPane::Guilds)
            || self.is_pane_filter_active(FocusPane::Channels)
    }

    pub fn is_guild_pane_filter_active(&self) -> bool {
        self.is_pane_filter_active(FocusPane::Guilds)
    }

    pub fn is_channel_pane_filter_active(&self) -> bool {
        self.is_pane_filter_active(FocusPane::Channels)
    }

    pub fn is_pane_filter_active(&self, pane: FocusPane) -> bool {
        self.pane_filter(pane).is_some()
    }

    pub fn guild_pane_filter_query(&self) -> Option<&str> {
        self.pane_filter_query(FocusPane::Guilds)
    }

    pub fn channel_pane_filter_query(&self) -> Option<&str> {
        self.pane_filter_query(FocusPane::Channels)
    }

    pub fn guild_pane_filter_cursor(&self) -> Option<usize> {
        self.pane_filter_cursor(FocusPane::Guilds)
    }

    pub fn channel_pane_filter_cursor(&self) -> Option<usize> {
        self.pane_filter_cursor(FocusPane::Channels)
    }

    #[cfg(test)]
    pub fn open_guild_pane_filter(&mut self) {
        self.open_pane_filter(FocusPane::Guilds);
    }

    #[cfg(test)]
    pub fn open_channel_pane_filter(&mut self) {
        self.open_pane_filter(FocusPane::Channels);
    }

    pub fn open_pane_filter(&mut self, pane: FocusPane) {
        self.reset_pane_filter_view(pane);
        self.set_pane_filter(pane, Some(PaneFilterState::new()));
    }

    pub fn is_pane_filter_editing(&self, pane: FocusPane) -> bool {
        self.pane_filter(pane).is_some_and(|f| f.is_editing())
    }

    fn pane_filter_query(&self, pane: FocusPane) -> Option<&str> {
        self.pane_filter(pane).map(|f| f.query())
    }

    fn pane_filter_cursor(&self, pane: FocusPane) -> Option<usize> {
        self.pane_filter(pane)
            .and_then(|f| f.is_editing().then(|| f.cursor_byte_index()))
    }

    pub fn close_active_pane_filters(&mut self) {
        self.close_pane_filter(FocusPane::Guilds);
        self.close_pane_filter(FocusPane::Channels);
    }

    pub fn close_pane_filter(&mut self, pane: FocusPane) {
        self.set_pane_filter(pane, None);
        self.reset_pane_filter_view(pane);
    }

    pub fn commit_pane_filter(&mut self, pane: FocusPane) {
        if let Some(f) = self.pane_filter_mut(pane) {
            f.commit();
        }
    }

    #[cfg(test)]
    pub fn push_guild_pane_filter_char(&mut self, value: char) {
        self.push_pane_filter_char(FocusPane::Guilds, value);
    }

    #[cfg(test)]
    pub fn push_channel_pane_filter_char(&mut self, value: char) {
        self.push_pane_filter_char(FocusPane::Channels, value);
    }

    pub fn push_pane_filter_char(&mut self, pane: FocusPane, value: char) {
        if let Some(f) = self.pane_filter_mut(pane) {
            f.push_char(value);
            self.reset_pane_filter_selection(pane);
        }
    }

    pub fn pop_pane_filter_char(&mut self, pane: FocusPane) {
        if let Some(f) = self.pane_filter_mut(pane) {
            f.pop_char();
            self.reset_pane_filter_selection(pane);
        }
    }

    pub fn move_pane_filter_cursor_left(&mut self, pane: FocusPane) {
        if let Some(f) = self.pane_filter_mut(pane) {
            f.cursor_left();
        }
    }

    pub fn move_pane_filter_cursor_right(&mut self, pane: FocusPane) {
        if let Some(f) = self.pane_filter_mut(pane) {
            f.cursor_right();
        }
    }

    fn pane_filter(&self, pane: FocusPane) -> Option<&PaneFilterState> {
        match pane {
            FocusPane::Guilds => self.navigation.guilds.filter.as_ref(),
            FocusPane::Channels => self.navigation.channels.filter.as_ref(),
            FocusPane::Messages | FocusPane::Members => None,
        }
    }

    fn pane_filter_mut(&mut self, pane: FocusPane) -> Option<&mut PaneFilterState> {
        match pane {
            FocusPane::Guilds => self.navigation.guilds.filter.as_mut(),
            FocusPane::Channels => self.navigation.channels.filter.as_mut(),
            FocusPane::Messages | FocusPane::Members => None,
        }
    }

    fn set_pane_filter(&mut self, pane: FocusPane, filter: Option<PaneFilterState>) {
        match pane {
            FocusPane::Guilds => self.navigation.guilds.filter = filter,
            FocusPane::Channels => self.navigation.channels.filter = filter,
            FocusPane::Messages | FocusPane::Members => {}
        }
    }

    fn reset_pane_filter_view(&mut self, pane: FocusPane) {
        self.reset_pane_filter_selection(pane);
        match pane {
            FocusPane::Guilds => self.navigation.guilds.list.keep_selection_visible(),
            FocusPane::Channels => self.navigation.channels.list.keep_selection_visible(),
            FocusPane::Messages | FocusPane::Members => {}
        }
    }

    fn reset_pane_filter_selection(&mut self, pane: FocusPane) {
        match pane {
            FocusPane::Guilds => {
                self.navigation.guilds.list.reset_selection_and_scroll();
            }
            FocusPane::Channels => {
                self.navigation.channels.list.reset_selection_and_scroll();
            }
            FocusPane::Messages | FocusPane::Members => {}
        }
    }

    pub fn activate_pane_filter_selection(&mut self, pane: FocusPane) -> Option<AppCommand> {
        match pane {
            FocusPane::Guilds => {
                if self.confirm_guild_pane_filter() {
                    self.focus_pane(FocusPane::Channels);
                }
                None
            }
            FocusPane::Channels => {
                let command = self.confirm_channel_pane_filter();
                if command.is_some() {
                    self.focus_pane(FocusPane::Messages);
                }
                command
            }
            FocusPane::Messages | FocusPane::Members => None,
        }
    }
}

impl PaneFilterState {
    pub(super) fn new() -> Self {
        Self {
            editing: true,
            ..Self::default()
        }
    }

    pub(super) fn is_editing(&self) -> bool {
        self.editing
    }

    pub(super) fn commit(&mut self) {
        self.editing = false;
    }

    pub(super) fn query(&self) -> &str {
        self.query.value()
    }

    pub(super) fn cursor_byte_index(&self) -> usize {
        self.query.cursor_byte_index()
    }

    pub(super) fn push_char(&mut self, value: char) {
        self.query.insert_char(value);
    }

    pub(super) fn pop_char(&mut self) {
        self.query.delete_previous_grapheme();
    }

    pub(super) fn cursor_left(&mut self) {
        self.query.move_left();
    }

    pub(super) fn cursor_right(&mut self) {
        self.query.move_right();
    }
}
