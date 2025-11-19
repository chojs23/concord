use ratatui::style::Color;
use twilight_model::id::{Id, marker::ChannelMarker, marker::GuildMarker};

use crate::discord::{
    AppCommand, AppEvent, ChannelState, DiscordState, GuildMemberState, GuildState, MessageState,
    PresenceStatus,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Composer,
    Members,
}

#[derive(Debug)]
pub struct DashboardState {
    discord: DiscordState,
    focus: FocusPane,
    selected_guild: usize,
    selected_channel: usize,
    selected_message: usize,
    selected_member: usize,
    composer_input: String,
    composer_active: bool,
    current_user: Option<String>,
    last_error: Option<String>,
    skipped_events: u64,
    should_quit: bool,
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            discord: DiscordState::default(),
            focus: FocusPane::Messages,
            selected_guild: 0,
            selected_channel: 0,
            selected_message: 0,
            selected_member: 0,
            composer_input: String::new(),
            composer_active: false,
            current_user: None,
            last_error: None,
            skipped_events: 0,
            should_quit: false,
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        match &event {
            AppEvent::Ready { user } => self.current_user = Some(user.clone()),
            AppEvent::GatewayError { message } => self.last_error = Some(message.clone()),
            AppEvent::GatewayClosed => {
                self.last_error = Some("gateway closed".to_owned());
            }
            _ => {}
        }
        self.discord.apply_event(&event);
        self.clamp_selection_indices();
        // Prefer to keep the message viewport on the latest message.
        let messages_len = self.messages().len();
        if messages_len > 0 {
            self.selected_message = messages_len - 1;
        }
    }

    pub fn record_lag(&mut self, skipped: u64) {
        self.skipped_events += skipped;
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

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn skipped_events(&self) -> u64 {
        self.skipped_events
    }

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
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

    pub fn selected_channel_name(&self) -> Option<String> {
        self.channels()
            .get(self.selected_channel())
            .map(|channel| channel.name.clone())
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

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return Vec::new();
        };
        let members = self.discord.members_for_guild(guild_id);
        let mut groups: Vec<MemberGroup<'_>> = Vec::new();

        for status in [
            PresenceStatus::Online,
            PresenceStatus::Idle,
            PresenceStatus::DoNotDisturb,
            PresenceStatus::Offline,
        ] {
            let mut entries: Vec<&GuildMemberState> = members
                .iter()
                .filter(|member| member.status == status)
                .copied()
                .collect();
            if entries.is_empty() {
                continue;
            }
            entries.sort_by(|a, b| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            });
            groups.push(MemberGroup { status, entries });
        }

        groups
    }

    pub fn flattened_members(&self) -> Vec<&GuildMemberState> {
        self.members_grouped()
            .into_iter()
            .flat_map(|group| group.entries)
            .collect()
    }

    pub fn selected_member(&self) -> usize {
        self.selected_member
            .min(self.flattened_members().len().saturating_sub(1))
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
                self.selected_member = 0;
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
            FocusPane::Members => {
                self.selected_member = self
                    .selected_member
                    .saturating_add(1)
                    .min(self.flattened_members().len().saturating_sub(1));
            }
            FocusPane::Composer => {}
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.selected_guild.saturating_sub(1);
                self.selected_channel = 0;
                self.selected_message = self.messages().len().saturating_sub(1);
                self.selected_member = 0;
            }
            FocusPane::Channels => {
                self.selected_channel = self.selected_channel.saturating_sub(1);
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Messages => {
                self.selected_message = self.selected_message.saturating_sub(1);
            }
            FocusPane::Members => {
                self.selected_member = self.selected_member.saturating_sub(1);
            }
            FocusPane::Composer => {}
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Guilds => self.selected_guild = 0,
            FocusPane::Channels => self.selected_channel = 0,
            FocusPane::Messages => self.selected_message = 0,
            FocusPane::Members => self.selected_member = 0,
            FocusPane::Composer => {}
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.guilds().len().saturating_sub(1);
            }
            FocusPane::Channels => {
                self.selected_channel = self.channels().len().saturating_sub(1);
            }
            FocusPane::Messages => {
                self.selected_message = self.messages().len().saturating_sub(1);
            }
            FocusPane::Members => {
                self.selected_member = self.flattened_members().len().saturating_sub(1);
            }
            FocusPane::Composer => {}
        }
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Composer,
            FocusPane::Composer => FocusPane::Members,
            FocusPane::Members => FocusPane::Guilds,
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

    fn clamp_selection_indices(&mut self) {
        self.selected_guild = self.selected_guild();
        self.selected_channel = self.selected_channel();
        self.selected_message = self.selected_message();
        self.selected_member = self.selected_member();
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct MemberGroup<'a> {
    pub status: PresenceStatus,
    pub entries: Vec<&'a GuildMemberState>,
}

pub fn presence_color(status: PresenceStatus) -> Color {
    match status {
        PresenceStatus::Online => Color::Green,
        PresenceStatus::Idle => Color::Yellow,
        PresenceStatus::DoNotDisturb => Color::Red,
        PresenceStatus::Offline => Color::DarkGray,
    }
}

pub fn presence_marker(status: PresenceStatus) -> char {
    match status {
        PresenceStatus::Online => '●',
        PresenceStatus::Idle => '◐',
        PresenceStatus::DoNotDisturb => '⊘',
        PresenceStatus::Offline => '○',
    }
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker, marker::UserMarker};

    use super::DashboardState;
    use crate::discord::{AppEvent, ChannelInfo, MemberInfo, PresenceStatus};

    #[test]
    fn tracks_current_user_from_ready() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
        });
        assert_eq!(state.current_user(), Some("neo"));
    }

    #[test]
    fn captures_last_gateway_error() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GatewayError {
            message: "boom".to_owned(),
        });
        assert_eq!(state.last_error(), Some("boom"));
    }

    #[test]
    fn member_groups_are_sorted_by_status_then_name() {
        let guild_id = Id::new(1);
        let alice: Id<UserMarker> = Id::new(10);
        let bob: Id<UserMarker> = Id::new(20);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(2),
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: vec![
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                },
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    is_bot: false,
                },
            ],
            presences: vec![
                (alice, PresenceStatus::Online),
                (bob, PresenceStatus::Online),
            ],
        });

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, PresenceStatus::Online);
        assert_eq!(
            groups[0]
                .entries
                .iter()
                .map(|m| m.display_name.as_str())
                .collect::<Vec<_>>(),
            vec!["alice", "bob"],
        );
    }

    #[test]
    fn message_creation_keeps_viewport_on_latest() {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        for id in 1..=3u64 {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("msg {id}")),
            });
        }

        assert_eq!(state.selected_message(), 2);
    }
}
