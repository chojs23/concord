use ratatui::style::Color;
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};

use crate::discord::AppEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    Gateway,
    Message,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKey {
    MessageCreate {
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
}

#[derive(Clone, Debug)]
pub struct EventItem {
    pub seq: u64,
    pub kind: EventKind,
    pub dedupe_key: Option<EventKey>,
    pub has_known_message_content: bool,
    pub summary: String,
    pub detail: String,
}

impl EventItem {
    pub fn from_app_event(seq: u64, event: AppEvent) -> Self {
        match event {
            AppEvent::Ready { user } => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("ready as {user}"),
                detail: format!("Gateway session is ready.\n\nUser: {user}"),
            },
            AppEvent::GuildCreate {
                guild_id,
                name,
                channels,
            } => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("guild available: {name}"),
                detail: format!(
                    "Guild available\n\nGuild: {name} ({})\nChannels: {}",
                    guild_id.get(),
                    channels.len()
                ),
            },
            AppEvent::GuildDelete { guild_id } => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("guild removed: {}", guild_id.get()),
                detail: format!("Guild removed\n\nGuild: {}", guild_id.get()),
            },
            AppEvent::ChannelUpsert(channel) => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("channel available: #{}", channel.name),
                detail: format!(
                    "Channel available\n\nGuild: {}\nChannel: {}\nName: {}\nKind: {}",
                    channel
                        .guild_id
                        .map(|id| id.get().to_string())
                        .unwrap_or_else(|| "dm".to_owned()),
                    channel.channel_id.get(),
                    channel.name,
                    channel.kind
                ),
            },
            AppEvent::ChannelDelete {
                guild_id,
                channel_id,
            } => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("channel removed: {}", channel_id.get()),
                detail: format!(
                    "Channel removed\n\nGuild: {}\nChannel: {}",
                    guild_id
                        .map(|id| id.get().to_string())
                        .unwrap_or_else(|| "dm".to_owned()),
                    channel_id.get()
                ),
            },
            AppEvent::MessageCreate {
                guild_id,
                channel_id,
                message_id,
                author_id,
                author,
                content,
            } => {
                let guild = guild_id
                    .map(|id| id.get().to_string())
                    .unwrap_or_else(|| "dm".to_owned());
                let has_known_message_content = content.is_some();
                let content = content.unwrap_or_else(|| "<message content unavailable>".to_owned());

                Self {
                    seq,
                    kind: EventKind::Message,
                    dedupe_key: Some(EventKey::MessageCreate {
                        channel_id,
                        message_id,
                    }),
                    has_known_message_content,
                    summary: format!("{author}: {content}"),
                    detail: format!(
                        "Message created\n\nGuild: {guild}\nChannel: {}\nMessage: {}\nAuthor: {author} ({})\n\n{content}",
                        channel_id.get(),
                        message_id.get(),
                        author_id.get()
                    ),
                }
            }
            AppEvent::MessageUpdate {
                guild_id,
                channel_id,
                message_id,
                content,
            } => {
                let guild = guild_id
                    .map(|id| id.get().to_string())
                    .unwrap_or_else(|| "dm".to_owned());
                let content = content.unwrap_or_else(|| "<message content unavailable>".to_owned());

                Self {
                    seq,
                    kind: EventKind::Message,
                    dedupe_key: None,
                    has_known_message_content: false,
                    summary: format!("message updated: {content}"),
                    detail: format!(
                        "Message updated\n\nGuild: {guild}\nChannel: {}\nMessage: {}\n\n{content}",
                        channel_id.get(),
                        message_id.get()
                    ),
                }
            }
            AppEvent::MessageDelete {
                guild_id,
                channel_id,
                message_id,
            } => {
                let guild = guild_id
                    .map(|id| id.get().to_string())
                    .unwrap_or_else(|| "dm".to_owned());

                Self {
                    seq,
                    kind: EventKind::Message,
                    dedupe_key: None,
                    has_known_message_content: false,
                    summary: format!("message deleted: {}", message_id.get()),
                    detail: format!(
                        "Message deleted\n\nGuild: {guild}\nChannel: {}\nMessage: {}",
                        channel_id.get(),
                        message_id.get()
                    ),
                }
            }
            AppEvent::GatewayError { message } => Self {
                seq,
                kind: EventKind::Error,
                dedupe_key: None,
                has_known_message_content: false,
                summary: format!("gateway error: {message}"),
                detail: format!("Gateway error\n\n{message}"),
            },
            AppEvent::GatewayClosed => Self {
                seq,
                kind: EventKind::Gateway,
                dedupe_key: None,
                has_known_message_content: false,
                summary: "gateway closed".to_owned(),
                detail: "Gateway stream closed.".to_owned(),
            },
        }
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            EventKind::Gateway => "gateway",
            EventKind::Message => "message",
            EventKind::Error => "error",
        }
    }

    pub fn color(&self) -> Color {
        match self.kind {
            EventKind::Gateway => Color::Cyan,
            EventKind::Message => Color::Green,
            EventKind::Error => Color::Red,
        }
    }
}

pub fn truncate_text(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let text: String = chars.by_ref().take(limit).collect();

    if chars.next().is_some() {
        format!("{text}...")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_text;

    #[test]
    fn truncates_long_text() {
        assert_eq!(truncate_text("abcdef", 3), "abc...");
    }

    #[test]
    fn keeps_short_text() {
        assert_eq!(truncate_text("abc", 10), "abc");
    }
}
