use std::collections::{BTreeMap, VecDeque};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use super::{AppEvent, ChannelInfo};

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
    pub name: String,
    pub kind: String,
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

#[derive(Clone, Debug)]
pub struct DiscordState {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
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
            max_messages_per_channel,
        }
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate {
                guild_id,
                name,
                channels,
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
            }
            AppEvent::GuildDelete { guild_id } => {
                self.guilds.remove(guild_id);
                self.channels
                    .retain(|_, channel| channel.guild_id != Some(*guild_id));
                self.messages
                    .retain(|channel_id, _| self.channels.contains_key(channel_id));
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
            AppEvent::Ready { .. } | AppEvent::GatewayError { .. } | AppEvent::GatewayClosed => {}
        }
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
        self.channels.insert(
            channel.channel_id,
            ChannelState {
                id: channel.channel_id,
                guild_id: channel.guild_id,
                name: channel.name.clone(),
                kind: channel.kind.clone(),
            },
        );
    }

    fn upsert_message(&mut self, message: MessageState) {
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

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker};

    use crate::discord::{AppEvent, ChannelInfo, DiscordState};

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
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
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
}
