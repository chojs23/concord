use twilight_gateway::Event;
use twilight_model::id::{
    Id, marker::ChannelMarker, marker::GuildMarker, marker::MessageMarker, marker::UserMarker,
};

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
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
    GatewayError {
        message: String,
    },
    GatewayClosed,
}

pub fn map_event(event: Event, message_content_enabled: bool) -> Option<AppEvent> {
    match event {
        Event::Ready(ready) => Some(AppEvent::Ready {
            user: ready.user.name,
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
        _ => None,
    }
}

fn map_message_content(content: &str, message_content_enabled: bool) -> Option<String> {
    if message_content_enabled || !content.is_empty() {
        return Some(content.to_owned());
    }

    None
}
