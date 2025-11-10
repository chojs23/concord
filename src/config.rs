use std::{collections::HashMap, env};

use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{AppError, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub default_channel_id: Option<Id<ChannelMarker>>,
    pub boot_message: Option<String>,
    pub enable_message_content: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let values: HashMap<String, String> = pairs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();

        Self::from_lookup(|key| values.get(key).cloned())
    }

    fn from_lookup<F>(mut lookup: F) -> Result<Self>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let default_channel_id = lookup("DISCORD_DEFAULT_CHANNEL_ID")
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(parse_channel_id)
            .transpose()?;

        let boot_message = lookup("DISCORD_BOOT_MESSAGE")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        let enable_message_content = lookup("DISCORD_ENABLE_MESSAGE_CONTENT")
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(parse_bool_flag)
            .transpose()?
            .unwrap_or(false);

        Ok(Self {
            default_channel_id,
            boot_message,
            enable_message_content,
        })
    }
}

fn parse_channel_id(value: &str) -> Result<Id<ChannelMarker>> {
    let parsed = value
        .parse::<u64>()
        .map_err(|source| AppError::InvalidChannelId {
            value: value.to_owned(),
            source,
        })?;

    Ok(Id::new(parsed))
}

fn parse_bool_flag(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(AppError::InvalidMessageContentFlag {
            value: value.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_required_and_optional_values() {
        let config = Config::from_pairs([
            ("DISCORD_DEFAULT_CHANNEL_ID", "123456789"),
            ("DISCORD_BOOT_MESSAGE", "hello from startup"),
        ])
        .expect("config should parse");

        assert_eq!(
            config.default_channel_id.expect("channel id").get(),
            123456789
        );
        assert_eq!(config.boot_message.as_deref(), Some("hello from startup"));
        assert!(!config.enable_message_content);
    }

    #[test]
    fn trims_blank_optional_values() {
        let config = Config::from_pairs([
            ("DISCORD_DEFAULT_CHANNEL_ID", "   "),
            ("DISCORD_BOOT_MESSAGE", "   "),
        ])
        .expect("config should parse");

        assert!(config.default_channel_id.is_none());
        assert!(config.boot_message.is_none());
        assert!(!config.enable_message_content);
    }

    #[test]
    fn parses_message_content_flag() {
        let config = Config::from_pairs([("DISCORD_ENABLE_MESSAGE_CONTENT", "true")])
            .expect("config should parse");

        assert!(config.enable_message_content);
    }
}
