use std::collections::{BTreeMap, VecDeque};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use super::{AppEvent, ChannelInfo, GuildFolder, MemberInfo, MessageInfo, PresenceStatus};

const DEFAULT_MAX_MESSAGES_PER_CHANNEL: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildState {
    pub id: Id<GuildMarker>,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelState {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
}

impl ChannelState {
    pub fn is_category(&self) -> bool {
        matches!(self.kind.as_str(), "category" | "GuildCategory")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageState {
    pub id: Id<MessageMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub status: PresenceStatus,
}

#[derive(Clone, Debug)]
pub struct DiscordState {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    members: BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it; the dashboard falls back to a flat guild list.
    guild_folders: Vec<GuildFolder>,
    max_messages_per_channel: usize,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGES_PER_CHANNEL)
    }
}

impl DiscordState {
    pub fn new(max_messages_per_channel: usize) -> Self {
        Self {
            guilds: BTreeMap::new(),
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
            members: BTreeMap::new(),
            guild_folders: Vec::new(),
            max_messages_per_channel,
        }
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate {
                guild_id,
                name,
                channels,
                members,
                presences,
            } => {
                self.guilds.insert(
                    *guild_id,
                    GuildState {
                        id: *guild_id,
                        name: name.clone(),
                    },
                );

                for channel in channels {
                    self.upsert_channel(channel);
                }

                let entry = self.members.entry(*guild_id).or_default();
                for member in members {
                    upsert_member(entry, member, None);
                }
                for (user_id, status) in presences {
                    if let Some(member) = entry.get_mut(user_id) {
                        member.status = *status;
                    }
                }
            }
            AppEvent::GuildUpdate { guild_id, name } => {
                if let Some(guild) = self.guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                }
            }
            AppEvent::GuildDelete { guild_id } => {
                self.guilds.remove(guild_id);
                self.channels
                    .retain(|_, channel| channel.guild_id != Some(*guild_id));
                self.messages
                    .retain(|channel_id, _| self.channels.contains_key(channel_id));
                self.members.remove(guild_id);
            }
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.channels.remove(channel_id);
                self.messages.remove(channel_id);
            }
            AppEvent::MessageCreate {
                guild_id,
                channel_id,
                message_id,
                author_id,
                author,
                content,
            } => self.upsert_message(MessageState {
                id: *message_id,
                guild_id: *guild_id,
                channel_id: *channel_id,
                author_id: *author_id,
                author: author.clone(),
                content: content.clone(),
            }),
            AppEvent::MessageHistoryLoaded {
                channel_id,
                messages,
            } => self.merge_message_history(*channel_id, messages),
            AppEvent::MessageUpdate {
                guild_id,
                channel_id,
                message_id,
                content,
            } => self.update_message(*guild_id, *channel_id, *message_id, content.clone()),
            AppEvent::MessageDelete {
                channel_id,
                message_id,
                ..
            } => self.delete_message(*channel_id, *message_id),
            AppEvent::GuildMemberUpsert { guild_id, member } => {
                let entry = self.members.entry(*guild_id).or_default();
                let previous_status = entry.get(&member.user_id).map(|m| m.status);
                upsert_member(entry, member, previous_status);
            }
            AppEvent::GuildMemberRemove { guild_id, user_id } => {
                if let Some(entry) = self.members.get_mut(guild_id) {
                    entry.remove(user_id);
                }
            }
            AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
            } => {
                let entry = self.members.entry(*guild_id).or_default();
                if let Some(member) = entry.get_mut(user_id) {
                    member.status = *status;
                } else {
                    entry.insert(
                        *user_id,
                        GuildMemberState {
                            user_id: *user_id,
                            display_name: format!("user-{}", user_id.get()),
                            is_bot: false,
                            status: *status,
                        },
                    );
                }
            }
            AppEvent::GuildFoldersUpdate { folders } => {
                self.guild_folders = folders.clone();
            }
            AppEvent::Ready { .. } | AppEvent::GatewayError { .. } | AppEvent::GatewayClosed => {}
        }
    }

    pub fn guild_folders(&self) -> &[GuildFolder] {
        &self.guild_folders
    }

    pub fn guilds(&self) -> Vec<&GuildState> {
        self.guilds.values().collect()
    }

    pub fn channels_for_guild(&self, guild_id: Option<Id<GuildMarker>>) -> Vec<&ChannelState> {
        self.channels
            .values()
            .filter(|channel| channel.guild_id == guild_id)
            .collect()
    }

    pub fn messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.messages
            .get(&channel_id)
            .map(|messages| messages.iter().collect())
            .unwrap_or_default()
    }

    pub fn members_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&GuildMemberState> {
        self.members
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn channel(&self, channel_id: Id<ChannelMarker>) -> Option<&ChannelState> {
        self.channels.get(&channel_id)
    }

    pub fn first_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.guilds.keys().next().copied()
    }

    pub fn first_channel_id_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<Id<ChannelMarker>> {
        self.channels_for_guild(guild_id)
            .first()
            .map(|channel| channel.id)
    }

    fn upsert_channel(&mut self, channel: &ChannelInfo) {
        let last_message_id = self
            .channels
            .get(&channel.channel_id)
            .and_then(|existing| existing.last_message_id)
            .max(channel.last_message_id);

        self.channels.insert(
            channel.channel_id,
            ChannelState {
                id: channel.channel_id,
                guild_id: channel.guild_id,
                parent_id: channel.parent_id,
                position: channel.position,
                last_message_id,
                name: channel.name.clone(),
                kind: channel.kind.clone(),
            },
        );
    }

    fn upsert_message(&mut self, message: MessageState) {
        let channel_id = message.channel_id;
        let message_id = message.id;
        let messages = self.messages.entry(message.channel_id).or_default();
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message.id) {
            existing.guild_id = message.guild_id;
            existing.channel_id = message.channel_id;
            existing.author_id = message.author_id;
            existing.author = message.author;
            if message.content.is_some() {
                existing.content = message.content;
            }
        } else {
            messages.push_back(message);
        }

        while messages.len() > self.max_messages_per_channel {
            messages.pop_front();
        }
        self.record_channel_message_id(channel_id, message_id);
    }

    fn merge_message_history(&mut self, channel_id: Id<ChannelMarker>, history: &[MessageInfo]) {
        let messages = self.messages.entry(channel_id).or_default();
        let mut by_id: BTreeMap<Id<MessageMarker>, MessageState> = messages
            .drain(..)
            .map(|message| (message.id, message))
            .collect();

        for message in history {
            if message.channel_id != channel_id {
                continue;
            }

            let incoming = MessageState {
                id: message.message_id,
                guild_id: message.guild_id,
                channel_id: message.channel_id,
                author_id: message.author_id,
                author: message.author.clone(),
                content: message.content.clone(),
            };

            by_id
                .entry(incoming.id)
                .and_modify(|existing| merge_message(existing, &incoming))
                .or_insert(incoming);
        }

        *messages = by_id.into_values().collect();
        while messages.len() > self.max_messages_per_channel {
            messages.pop_front();
        }
        if let Some(last_message_id) = messages.back().map(|message| message.id) {
            self.record_channel_message_id(channel_id, last_message_id);
        }
    }

    fn record_channel_message_id(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.last_message_id = channel.last_message_id.max(Some(message_id));
        }
    }

    fn update_message(
        &mut self,
        _guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: Option<String>,
    ) {
        let messages = self.messages.entry(channel_id).or_default();
        if let Some(content) = content
            && let Some(existing) = messages.iter_mut().find(|item| item.id == message_id)
        {
            existing.content = Some(content);
        }
    }

    fn delete_message(&mut self, channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            messages.retain(|message| message.id != message_id);
        }
    }
}

fn merge_message(existing: &mut MessageState, incoming: &MessageState) {
    existing.guild_id = incoming.guild_id;
    existing.channel_id = incoming.channel_id;
    existing.author_id = incoming.author_id;
    existing.author = incoming.author.clone();

    if let Some(content) = &incoming.content {
        let existing_is_empty = existing
            .content
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true);
        if !content.is_empty() || existing_is_empty {
            existing.content = Some(content.clone());
        }
    }
}

fn upsert_member(
    map: &mut BTreeMap<Id<UserMarker>, GuildMemberState>,
    member: &MemberInfo,
    previous_status: Option<PresenceStatus>,
) {
    let status = previous_status.unwrap_or(PresenceStatus::Offline);
    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name: member.display_name.clone(),
            is_bot: member.is_bot,
            status,
        },
    );
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker};

    use crate::discord::{
        AppEvent, ChannelInfo, DiscordState, MemberInfo, MessageInfo, PresenceStatus,
    };

    #[test]
    fn applies_guild_channels_and_messages() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
        });

        assert_eq!(state.guilds().len(), 1);
        assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(state.messages_for_channel(channel_id).len(), 1);
    }

    #[test]
    fn stores_channel_parent_and_position() {
        let guild_id = Id::new(1);
        let category_id = Id::new(2);
        let channel_id = Id::new(3);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: Some(category_id),
            position: Some(7),
            last_message_id: Some(Id::new(9)),
            name: "general".to_owned(),
            kind: "text".to_owned(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.parent_id, Some(category_id));
        assert_eq!(channel.position, Some(7));
        assert_eq!(channel.last_message_id, Some(Id::new(9)));
    }

    #[test]
    fn bounds_messages_per_channel() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(1);

        for id in [1, 2] {
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: None,
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("message {id}")),
            });
        }

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id.get(), 2);
    }

    #[test]
    fn keeps_known_content_when_gateway_echo_has_no_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(30);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            content: None,
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            content: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("hello"));
    }

    #[test]
    fn merges_history_in_chronological_order() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("live".to_owned()),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![
                message_info(channel_id, 20, "history 20"),
                message_info(channel_id, 10, "history 10"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn history_dedupes_and_preserves_known_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("known".to_owned()),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![MessageInfo {
                content: Some(String::new()),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("known"));
    }

    #[test]
    fn history_respects_message_limit_after_merge() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(2);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 20, "middle"),
                message_info(channel_id, 30, "new"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![20, 30]
        );
    }

    #[test]
    fn live_messages_update_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("new".to_owned()),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(10),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some("old".to_owned()),
        });

        assert_eq!(
            state.channel(channel_id).and_then(|channel| channel.last_message_id),
            Some(Id::new(30))
        );
    }

    #[test]
    fn history_updates_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 40, "new"),
            ],
        });

        assert_eq!(
            state.channel(channel_id).and_then(|channel| channel.last_message_id),
            Some(Id::new(40))
        );
    }

    #[test]
    fn channel_upsert_does_not_regress_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "neo renamed".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo renamed again".to_owned(),
            kind: "dm".to_owned(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.name, "neo renamed again");
        assert_eq!(channel.last_message_id, Some(Id::new(30)));
    }

    #[test]
    fn tracks_members_and_presences() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: Vec::new(),
            members: vec![
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    is_bot: false,
                },
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                },
            ],
            presences: vec![(alice, PresenceStatus::Online)],
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(members.len(), 2);
        let alice_state = members.iter().find(|m| m.user_id == alice).unwrap();
        assert_eq!(alice_state.status, PresenceStatus::Online);
        let bob_state = members.iter().find(|m| m.user_id == bob).unwrap();
        assert_eq!(bob_state.status, PresenceStatus::Offline);

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: bob,
            status: PresenceStatus::Idle,
        });
        assert_eq!(
            state
                .members_for_guild(guild_id)
                .iter()
                .find(|m| m.user_id == bob)
                .unwrap()
                .status,
            PresenceStatus::Idle,
        );
    }

    #[test]
    fn member_upsert_preserves_existing_status() {
        let guild_id = Id::new(1);
        let user = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice".to_owned(),
                is_bot: false,
            },
        });
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: user,
            status: PresenceStatus::Online,
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice-renamed".to_owned(),
                is_bot: false,
            },
        });

        let member = state
            .members_for_guild(guild_id)
            .into_iter()
            .find(|m| m.user_id == user)
            .unwrap();
        assert_eq!(member.display_name, "alice-renamed");
        assert_eq!(member.status, PresenceStatus::Online);
    }

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
        MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            content: Some(content.to_owned()),
        }
    }
}
