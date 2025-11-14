use twilight_gateway::Event;
use twilight_model::{
    channel::{Channel, Message},
    gateway::payload::incoming::GuildCreate as GuildCreatePayload,
    id::{
        Id, marker::ChannelMarker, marker::GuildMarker, marker::MessageMarker, marker::UserMarker,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        channels: Vec<ChannelInfo>,
    },
    GuildDelete {
        guild_id: Id<GuildMarker>,
    },
    ChannelUpsert(ChannelInfo),
    ChannelDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
    },
    MessageCreate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_id: Id<UserMarker>,
        author: String,
        content: Option<String>,
    },
    MessageUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: Option<String>,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    GatewayError {
        message: String,
    },
    GatewayClosed,
}

impl AppEvent {
    pub fn from_message(message: Message) -> Self {
        Self::MessageCreate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message.author.name,
            content: Some(message.content),
        }
    }
}

pub fn map_event(event: Event, message_content_enabled: bool) -> Option<AppEvent> {
    match event {
        Event::Ready(ready) => Some(AppEvent::Ready {
            user: ready.user.name,
        }),
        Event::GuildCreate(guild) => map_guild_create(*guild),
        Event::GuildDelete(guild) => Some(AppEvent::GuildDelete { guild_id: guild.id }),
        Event::GuildUpdate(guild) => Some(AppEvent::GuildCreate {
            guild_id: guild.id,
            name: guild.name.clone(),
            channels: Vec::new(),
        }),
        Event::ChannelCreate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelUpdate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelDelete(channel) => Some(AppEvent::ChannelDelete {
            guild_id: channel.guild_id,
            channel_id: channel.id,
        }),
        Event::MessageCreate(message) => Some(AppEvent::MessageCreate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message.author.name.clone(),
            content: map_message_content(&message.content, message_content_enabled),
        }),
        Event::MessageUpdate(message) => Some(AppEvent::MessageUpdate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            content: map_message_content(&message.content, message_content_enabled),
        }),
        Event::MessageDelete(message) => Some(AppEvent::MessageDelete {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
        }),
        _ => None,
    }
}

fn map_guild_create(guild: GuildCreatePayload) -> Option<AppEvent> {
    match guild {
        GuildCreatePayload::Available(guild) => Some(AppEvent::GuildCreate {
            guild_id: guild.id,
            name: guild.name,
            channels: guild.channels.iter().map(channel_info).collect(),
        }),
        GuildCreatePayload::Unavailable(_) => None,
    }
}

fn channel_info(channel: &Channel) -> ChannelInfo {
    ChannelInfo {
        guild_id: channel.guild_id,
        channel_id: channel.id,
        name: channel
            .name
            .clone()
            .unwrap_or_else(|| format!("channel-{}", channel.id.get())),
        kind: format!("{:?}", channel.kind),
    }
}

fn map_message_content(content: &str, message_content_enabled: bool) -> Option<String> {
    if message_content_enabled || !content.is_empty() {
        return Some(content.to_owned());
    }

    None
}
