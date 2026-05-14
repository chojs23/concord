use ratatui::style::{Color, Style};

use crate::config::Theme;
use crate::discord::PresenceStatus;

/// Resolved color scheme used throughout the TUI. All fields are concrete
/// ratatui `Color` values derived from the `[theme]` section of `config.toml`
/// (or from built-in defaults when the section is absent).
#[derive(Clone, Debug)]
pub(crate) struct ColorScheme {
    pub background: Color,
    pub accent: Color,
    pub dim: Color,
    pub scrollbar_thumb: Color,
    pub selection_border: Color,
    pub mention_badge: Color,
    pub channel_read: Color,
    pub channel_unread: Color,
    pub self_reaction: Color,
    pub self_mention_bg: Color,
    pub self_mention_fg: Color,
    pub other_mention_bg: Color,
    pub other_mention_fg: Color,
    pub presence_online: Color,
    pub presence_idle: Color,
    pub presence_dnd: Color,
    pub presence_offline: Color,
    pub folder_default: Color,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            background: Color::Reset,
            accent: Color::Cyan,
            dim: Color::DarkGray,
            scrollbar_thumb: Color::Rgb(170, 170, 170),
            selection_border: Color::Green,
            mention_badge: Color::Rgb(255, 165, 0),
            channel_read: Color::Rgb(130, 130, 130),
            channel_unread: Color::Rgb(255, 255, 255),
            self_reaction: Color::Yellow,
            self_mention_bg: Color::Rgb(92, 76, 35),
            self_mention_fg: Color::Yellow,
            other_mention_bg: Color::Rgb(40, 50, 92),
            other_mention_fg: Color::Rgb(193, 206, 247),
            presence_online: Color::Green,
            presence_idle: Color::Rgb(180, 140, 0),
            presence_dnd: Color::Red,
            presence_offline: Color::DarkGray,
            folder_default: Color::Cyan,
        }
    }
}

impl ColorScheme {
    pub fn from_theme(theme: &Theme) -> Self {
        let d = Self::default();
        Self {
            background: parse_hex(&theme.background).unwrap_or(Color::Reset),
            accent: parse_hex(&theme.accent).unwrap_or(d.accent),
            dim: parse_hex(&theme.dim).unwrap_or(d.dim),
            scrollbar_thumb: parse_hex(&theme.scrollbar_thumb).unwrap_or(d.scrollbar_thumb),
            selection_border: parse_hex(&theme.selection_border).unwrap_or(d.selection_border),
            mention_badge: parse_hex(&theme.mention_badge).unwrap_or(d.mention_badge),
            channel_read: parse_hex(&theme.channel_read).unwrap_or(d.channel_read),
            channel_unread: parse_hex(&theme.channel_unread).unwrap_or(d.channel_unread),
            self_reaction: parse_hex(&theme.self_reaction).unwrap_or(d.self_reaction),
            self_mention_bg: parse_hex(&theme.self_mention_bg).unwrap_or(d.self_mention_bg),
            self_mention_fg: parse_hex(&theme.self_mention_fg).unwrap_or(d.self_mention_fg),
            other_mention_bg: parse_hex(&theme.other_mention_bg).unwrap_or(d.other_mention_bg),
            other_mention_fg: parse_hex(&theme.other_mention_fg).unwrap_or(d.other_mention_fg),
            presence_online: parse_hex(&theme.presence_online).unwrap_or(d.presence_online),
            presence_idle: parse_hex(&theme.presence_idle).unwrap_or(d.presence_idle),
            presence_dnd: parse_hex(&theme.presence_dnd).unwrap_or(d.presence_dnd),
            presence_offline: parse_hex(&theme.presence_offline).unwrap_or(d.presence_offline),
            folder_default: parse_hex(&theme.folder_default).unwrap_or(d.folder_default),
        }
    }

    pub fn presence_color(&self, status: PresenceStatus) -> Color {
        match status {
            PresenceStatus::Online => self.presence_online,
            PresenceStatus::Idle => self.presence_idle,
            PresenceStatus::DoNotDisturb => self.presence_dnd,
            PresenceStatus::Offline | PresenceStatus::Unknown => self.presence_offline,
        }
    }

    pub fn folder_color(&self, color: Option<u32>) -> Color {
        match color {
            Some(value) if value != 0 => {
                let r = ((value >> 16) & 0xFF) as u8;
                let g = ((value >> 8) & 0xFF) as u8;
                let b = (value & 0xFF) as u8;
                Color::Rgb(r, g, b)
            }
            _ => self.folder_default,
        }
    }

    pub fn self_mention_style(&self) -> Style {
        Style::default()
            .bg(self.self_mention_bg)
            .fg(self.self_mention_fg)
    }

    pub fn other_mention_style(&self) -> Style {
        Style::default()
            .bg(self.other_mention_bg)
            .fg(self.other_mention_fg)
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color::Rgb(r, g, b))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_valid_with_hash() {
        assert_eq!(parse_hex("#00FFFF"), Some(Color::Rgb(0, 255, 255)));
    }

    #[test]
    fn parse_hex_valid_without_hash() {
        assert_eq!(parse_hex("FF6600"), Some(Color::Rgb(255, 102, 0)));
    }

    #[test]
    fn parse_hex_invalid_returns_none() {
        assert_eq!(parse_hex("ZZZZZZ"), None);
        assert_eq!(parse_hex("#FFF"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn from_theme_uses_parsed_hex() {
        let theme = Theme {
            accent: "#FF6600".to_owned(),
            ..Default::default()
        };
        let scheme = ColorScheme::from_theme(&theme);
        assert_eq!(scheme.accent, Color::Rgb(255, 102, 0));
    }

    #[test]
    fn from_theme_falls_back_on_invalid_hex() {
        let theme = Theme {
            accent: "not-a-color".to_owned(),
            ..Default::default()
        };
        let scheme = ColorScheme::from_theme(&theme);
        assert_eq!(scheme.accent, ColorScheme::default().accent);
    }
}
