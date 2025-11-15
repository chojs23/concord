use std::{collections::VecDeque, fmt};

use twilight_model::id::{Id, marker::ChannelMarker, marker::GuildMarker};

use crate::discord::{AppCommand, AppEvent, ChannelState, DiscordState, GuildState, MessageState};

use super::format::{EventItem, EventKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Sidebar,
    Guilds,
    Channels,
    Messages,
    Events,
    Detail,
    Composer,
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
    discord: DiscordState,
    events: VecDeque<EventItem>,
    selected: usize,
    selected_guild: usize,
    selected_channel: usize,
    selected_message: usize,
    focus: FocusPane,
    filter: EventFilter,
    composer_input: String,
    composer_active: bool,
    next_seq: u64,
    current_user: Option<String>,
    skipped_events: u64,
    max_events: usize,
    should_quit: bool,
}

impl DashboardState {
    pub fn new(max_events: usize) -> Self {
        Self {
            discord: DiscordState::default(),
            events: VecDeque::new(),
            selected: 0,
            selected_guild: 0,
            selected_channel: 0,
            selected_message: 0,
            focus: FocusPane::Events,
            filter: EventFilter::All,
            composer_input: String::new(),
            composer_active: false,
            next_seq: 1,
            current_user: None,
            skipped_events: 0,
            max_events,
            should_quit: false,
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        let was_at_bottom = self.is_at_bottom();

        self.discord.apply_event(&event);
        self.clamp_domain_selection();

        if let AppEvent::Ready { user } = &event {
            self.current_user = Some(user.clone());
        }

        let item = EventItem::from_app_event(self.next_seq, event);
        if self.merge_duplicate_event(&item) {
            if was_at_bottom {
                self.follow_latest();
            } else {
                self.selected = self.selected();
            }
            return;
        }

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

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
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

    pub fn guilds(&self) -> Vec<&GuildState> {
        self.discord.guilds()
    }

    pub fn selected_guild(&self) -> usize {
        self.selected_guild
            .min(self.guilds().len().saturating_sub(1))
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.guilds()
            .get(self.selected_guild())
            .map(|guild| guild.id)
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        self.discord.channels_for_guild(self.selected_guild_id())
    }

    pub fn selected_channel(&self) -> usize {
        self.selected_channel
            .min(self.channels().len().saturating_sub(1))
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.channels()
            .get(self.selected_channel())
            .map(|channel| channel.id)
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        self.selected_channel_id()
            .map(|channel_id| self.discord.messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    pub fn selected_message(&self) -> usize {
        self.selected_message
            .min(self.messages().len().saturating_sub(1))
    }

    pub fn selected_message_item(&self) -> Option<&MessageState> {
        self.messages().get(self.selected_message()).copied()
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self
                    .selected_guild
                    .saturating_add(1)
                    .min(self.guilds().len().saturating_sub(1));
                self.selected_channel = 0;
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Channels => {
                self.selected_channel = self
                    .selected_channel
                    .saturating_add(1)
                    .min(self.channels().len().saturating_sub(1));
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Messages => {
                self.selected_message = self
                    .selected_message
                    .saturating_add(1)
                    .min(self.messages().len().saturating_sub(1));
            }
            FocusPane::Events | FocusPane::Sidebar | FocusPane::Detail | FocusPane::Composer => {
                let max = self.visible_events().len().saturating_sub(1);
                self.selected = self.selected.saturating_add(1).min(max);
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.selected_guild.saturating_sub(1);
                self.selected_channel = 0;
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Channels => {
                self.selected_channel = self.selected_channel.saturating_sub(1);
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Messages => self.selected_message = self.selected_message.saturating_sub(1),
            FocusPane::Events | FocusPane::Sidebar | FocusPane::Detail | FocusPane::Composer => {
                self.selected = self.selected.saturating_sub(1);
            }
        }
    }

    pub fn jump_top(&mut self) {
        self.selected = 0;
    }

    pub fn jump_bottom(&mut self) {
        self.follow_latest();
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Sidebar => FocusPane::Guilds,
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Events,
            FocusPane::Events => FocusPane::Detail,
            FocusPane::Detail => FocusPane::Composer,
            FocusPane::Composer => FocusPane::Sidebar,
        };
    }

    pub fn start_composer(&mut self) {
        self.composer_active = true;
        self.focus = FocusPane::Composer;
    }

    pub fn cancel_composer(&mut self) {
        self.composer_active = false;
        self.composer_input.clear();
    }

    pub fn push_composer_char(&mut self, value: char) {
        self.composer_input.push(value);
    }

    pub fn pop_composer_char(&mut self) {
        self.composer_input.pop();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let content = self.composer_input.trim().to_owned();
        if content.is_empty() {
            return None;
        }

        self.composer_input.clear();
        self.composer_active = false;
        Some(AppCommand::SendMessage {
            channel_id,
            content,
        })
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

    fn merge_duplicate_event(&mut self, item: &EventItem) -> bool {
        let Some(dedupe_key) = item.dedupe_key else {
            return false;
        };

        let Some(existing) = self
            .events
            .iter_mut()
            .find(|event| event.dedupe_key == Some(dedupe_key))
        else {
            return false;
        };

        if item.has_known_message_content || !existing.has_known_message_content {
            let seq = existing.seq;
            *existing = item.clone();
            existing.seq = seq;
        }

        true
    }

    fn clamp_domain_selection(&mut self) {
        self.selected_guild = self.selected_guild();
        self.selected_channel = self.selected_channel();
        self.selected_message = self.messages().len().saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{
        Id,
        marker::{ChannelMarker, UserMarker},
    };

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

    #[test]
    fn dedupes_message_create_events_by_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let author_id: Id<UserMarker> = Id::new(20);
        let mut state = DashboardState::new(10);

        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
        });
        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: None,
        });

        assert_eq!(state.total_events(), 1);
        assert_eq!(state.visible_events()[0].summary, "neo: hello");
    }

    #[test]
    fn updates_duplicate_message_event_when_content_becomes_known() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let author_id: Id<UserMarker> = Id::new(20);
        let mut state = DashboardState::new(10);

        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: None,
        });
        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id,
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
        });

        assert_eq!(state.total_events(), 1);
        assert_eq!(state.visible_events()[0].summary, "neo: hello");
    }
}
