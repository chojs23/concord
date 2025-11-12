use ratatui::style::Color;

use crate::discord::AppEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    Gateway,
    Message,
    Error,
}

#[derive(Clone, Debug)]
pub struct EventItem {
    pub seq: u64,
    pub kind: EventKind,
    pub summary: String,
    pub detail: String,
}

impl EventItem {
    pub fn from_app_event(seq: u64, event: AppEvent) -> Self {
        match event {
            AppEvent::Ready { user } => Self {
                seq,
                kind: EventKind::Gateway,
                summary: format!("ready as {user}"),
                detail: format!("Gateway session is ready.\n\nUser: {user}"),
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
                let content = content.unwrap_or_else(|| "<message content unavailable>".to_owned());

                Self {
                    seq,
                    kind: EventKind::Message,
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
                    summary: format!("message updated: {content}"),
                    detail: format!(
                        "Message updated\n\nGuild: {guild}\nChannel: {}\nMessage: {}\n\n{content}",
                        channel_id.get(),
                        message_id.get()
                    ),
                }
            }
            AppEvent::GatewayError { message } => Self {
                seq,
                kind: EventKind::Error,
                summary: format!("gateway error: {message}"),
                detail: format!("Gateway error\n\n{message}"),
            },
            AppEvent::GatewayClosed => Self {
                seq,
                kind: EventKind::Gateway,
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
