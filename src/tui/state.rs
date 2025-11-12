use std::{collections::VecDeque, fmt};

use crate::discord::AppEvent;

use super::format::{EventItem, EventKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Sidebar,
    Events,
    Detail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventFilter {
    All,
    Messages,
    Gateway,
    Errors,
}

impl EventFilter {
    pub fn matches(self, item: &EventItem) -> bool {
        match self {
            Self::All => true,
            Self::Messages => item.kind == EventKind::Message,
            Self::Gateway => item.kind == EventKind::Gateway,
            Self::Errors => item.kind == EventKind::Error,
        }
    }
}

impl fmt::Display for EventFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Messages => f.write_str("messages"),
            Self::Gateway => f.write_str("gateway"),
            Self::Errors => f.write_str("errors"),
        }
    }
}

#[derive(Debug)]
pub struct DashboardState {
    events: VecDeque<EventItem>,
    selected: usize,
    focus: FocusPane,
    filter: EventFilter,
    next_seq: u64,
    current_user: Option<String>,
    skipped_events: u64,
    max_events: usize,
    should_quit: bool,
}

impl DashboardState {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: VecDeque::new(),
            selected: 0,
            focus: FocusPane::Events,
            filter: EventFilter::All,
            next_seq: 1,
            current_user: None,
            skipped_events: 0,
            max_events,
            should_quit: false,
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        let was_at_bottom = self.is_at_bottom();

        if let AppEvent::Ready { user } = &event {
            self.current_user = Some(user.clone());
        }

        let item = EventItem::from_app_event(self.next_seq, event);
        self.next_seq += 1;
        self.events.push_back(item);

        while self.events.len() > self.max_events {
            self.events.pop_front();
        }

        if was_at_bottom {
            self.follow_latest();
        } else {
            self.selected = self.selected();
        }
    }

    pub fn record_lag(&mut self, skipped: u64) {
        self.skipped_events += skipped;
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.selected = 0;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn focus(&self) -> FocusPane {
        self.focus
    }

    pub fn filter(&self) -> EventFilter {
        self.filter
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn skipped_events(&self) -> u64 {
        self.skipped_events
    }

    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    pub fn visible_events(&self) -> Vec<&EventItem> {
        self.events
            .iter()
            .filter(|event| self.filter.matches(event))
            .collect()
    }

    pub fn selected(&self) -> usize {
        let visible_len = self.visible_events().len();
        self.selected.min(visible_len.saturating_sub(1))
    }

    pub fn selected_event(&self) -> Option<&EventItem> {
        self.visible_events().get(self.selected()).copied()
    }

    pub fn move_down(&mut self) {
        let max = self.visible_events().len().saturating_sub(1);
        self.selected = self.selected.saturating_add(1).min(max);
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn jump_top(&mut self) {
        self.selected = 0;
    }

    pub fn jump_bottom(&mut self) {
        self.follow_latest();
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Sidebar => FocusPane::Events,
            FocusPane::Events => FocusPane::Detail,
            FocusPane::Detail => FocusPane::Sidebar,
        };
    }

    pub fn set_filter(&mut self, filter: EventFilter) {
        self.filter = filter;
        self.selected = self
            .selected()
            .min(self.visible_events().len().saturating_sub(1));
    }

    fn follow_latest(&mut self) {
        self.selected = self.visible_events().len().saturating_sub(1);
    }

    fn is_at_bottom(&self) -> bool {
        let visible_len = self.visible_events().len();
        self.selected >= visible_len.saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{DashboardState, EventFilter};
    use crate::discord::AppEvent;

    #[test]
    fn bounds_event_history() {
        let mut state = DashboardState::new(2);

        state.push_event(AppEvent::GatewayClosed);
        state.push_event(AppEvent::GatewayClosed);
        state.push_event(AppEvent::GatewayClosed);

        assert_eq!(state.total_events(), 2);
    }

    #[test]
    fn filters_gateway_events() {
        let mut state = DashboardState::new(10);
        state.push_event(AppEvent::GatewayClosed);
        state.set_filter(EventFilter::Messages);

        assert!(state.visible_events().is_empty());
    }

    #[test]
    fn preserves_selection_when_user_is_browsing_history() {
        let mut state = DashboardState::new(10);
        state.push_event(AppEvent::GatewayClosed);
        state.push_event(AppEvent::GatewayClosed);
        state.push_event(AppEvent::GatewayClosed);

        state.move_up();
        let selected_before = state.selected();
        state.push_event(AppEvent::GatewayClosed);

        assert_eq!(state.selected(), selected_before);
    }

    #[test]
    fn follows_latest_when_selection_is_at_bottom() {
        let mut state = DashboardState::new(10);
        state.push_event(AppEvent::GatewayClosed);
        state.push_event(AppEvent::GatewayClosed);

        state.push_event(AppEvent::GatewayClosed);

        assert_eq!(state.selected(), 2);
    }
}
