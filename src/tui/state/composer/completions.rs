use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, EmojiMarker, RoleMarker, UserMarker},
};

use crate::discord::{
    ApplicationCommandIdentity, ApplicationCommandInfo, ApplicationCommandOptionInfo, ChannelState,
    CustomEmojiInfo, PresenceStatus, RoleState, builtin_slash_commands,
};

use super::super::{MemberEntry, emoji::custom_emoji_can_be_used_directly};

/// Maximum number of suggestions composer pickers show at once. Candidate
/// builders still return every match. Rendering scrolls this many rows.
pub const MAX_MENTION_PICKER_VISIBLE: usize = 8;

/// One entry in the rendered @-mention picker list.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MentionPickerEntry {
    pub target: MentionPickerTarget,
    pub display_name: String,
    /// Discord login handle. Shown as a hint in the picker so the user can
    /// tell which entry matches when they typed against the username instead
    /// of the alias.
    pub username: Option<String>,
    pub status: PresenceStatus,
    pub is_bot: bool,
    pub role_color: Option<u32>,
}

impl MentionPickerEntry {
    pub fn display_label(&self) -> &str {
        self.display_name
            .strip_prefix(self.target.visible_prefix())
            .unwrap_or(&self.display_name)
    }

    pub fn visible_text(&self) -> String {
        if self.display_name.starts_with(self.target.visible_prefix()) {
            self.display_name.clone()
        } else {
            format!("{}{}", self.target.visible_prefix(), self.display_name)
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MentionPickerTarget {
    User(Id<UserMarker>),
    Everyone(Id<RoleMarker>),
    Role(Id<RoleMarker>),
    Channel(Id<ChannelMarker>),
}

impl MentionPickerTarget {
    pub fn wire_format(self) -> String {
        match self {
            Self::User(id) => format!("<@{}>", id.get()),
            Self::Everyone(_) => "@everyone".to_owned(),
            Self::Role(id) => format!("<@&{}>", id.get()),
            Self::Channel(id) => format!("<#{}>", id.get()),
        }
    }

    pub fn command_wire_format(self) -> String {
        match self {
            Self::Everyone(id) => format!("<@&{}>", id.get()),
            target => target.wire_format(),
        }
    }

    pub fn visible_prefix(self) -> &'static str {
        match self {
            Self::User(_) | Self::Everyone(_) | Self::Role(_) => "@",
            Self::Channel(_) => "#",
        }
    }
}

/// One entry in the rendered emoji shortcode picker list.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmojiPickerEntry {
    pub emoji: String,
    pub shortcode: String,
    pub name: String,
    pub wire_format: Option<String>,
    pub available: bool,
    pub available_as_link: bool,
    pub custom_image_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommandPickerEntry {
    pub label: String,
    pub detail: String,
    pub replacement: String,
    pub top_level: bool,
    pub command_identity: Option<ApplicationCommandIdentity>,
}

pub(super) fn build_builtin_command_candidates(query: &str) -> Vec<CommandPickerEntry> {
    let needle = query.to_ascii_lowercase();
    builtin_slash_commands()
        .iter()
        .filter(|command| needle.is_empty() || command.name.starts_with(&needle))
        .map(|command| CommandPickerEntry {
            label: format!("/{}", command.name),
            detail: command.description.to_owned(),
            replacement: command.replacement.to_owned(),
            top_level: true,
            command_identity: None,
        })
        .collect()
}

pub(super) fn build_command_candidates(
    query: &str,
    commands: &[ApplicationCommandInfo],
) -> Vec<CommandPickerEntry> {
    let needle = query.to_ascii_lowercase();
    let mut scored: Vec<(u8, String, CommandPickerEntry)> = commands
        .iter()
        .filter_map(|command| {
            let lowered = command.name.to_ascii_lowercase();
            let rank = if needle.is_empty() {
                1
            } else if lowered.starts_with(&needle) {
                0
            } else if lowered.contains(&needle) {
                2
            } else {
                return None;
            };
            Some((
                rank,
                lowered,
                CommandPickerEntry {
                    label: format!("/{}", command.name),
                    detail: command_picker_detail(command),
                    replacement: format!("/{} ", command.name),
                    top_level: true,
                    command_identity: Some(command.identity()),
                },
            ))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

fn command_picker_detail(command: &ApplicationCommandInfo) -> String {
    match command.application_name.as_deref() {
        Some(name) if !command.description.is_empty() => {
            format!("{name} - {}", command.description)
        }
        Some(name) => name.to_owned(),
        None => command.description.clone(),
    }
}

pub(super) fn build_command_option_candidates(
    query: &str,
    options: &[ApplicationCommandOptionInfo],
) -> Vec<CommandPickerEntry> {
    let needle = query.to_ascii_lowercase();
    options
        .iter()
        .filter(|option| needle.is_empty() || option.name.to_ascii_lowercase().starts_with(&needle))
        .map(|option| CommandPickerEntry {
            label: command_option_label(option),
            detail: command_option_detail(option),
            replacement: command_option_replacement(option),
            top_level: false,
            command_identity: None,
        })
        .collect()
}

fn command_option_detail(option: &ApplicationCommandOptionInfo) -> String {
    if matches!(option.kind, 1 | 2) {
        return option.description.clone();
    }

    let requirement = if option.required {
        "required"
    } else {
        "optional"
    };
    if option.description.is_empty() {
        requirement.to_owned()
    } else {
        format!("{requirement} - {}", option.description)
    }
}

fn command_option_label(option: &ApplicationCommandOptionInfo) -> String {
    if matches!(option.kind, 1 | 2) {
        option.name.clone()
    } else {
        format!("{}:", option.name)
    }
}

fn command_option_replacement(option: &ApplicationCommandOptionInfo) -> String {
    if matches!(option.kind, 1 | 2) {
        format!("{} ", option.name)
    } else {
        format!("{}:", option.name)
    }
}

pub(super) fn build_command_choice_candidates(
    query: &str,
    option: &ApplicationCommandOptionInfo,
) -> Vec<CommandPickerEntry> {
    let needle = query.to_ascii_lowercase();
    option
        .choices
        .iter()
        .filter(|choice| choice.name.to_ascii_lowercase().contains(&needle))
        .map(|choice| CommandPickerEntry {
            label: choice.name.clone(),
            detail: choice.value.as_str().unwrap_or_default().to_owned(),
            replacement: format!(
                "{} ",
                choice
                    .value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| choice.value.to_string())
            ),
            top_level: false,
            command_identity: None,
        })
        .collect()
}

pub(super) fn build_mention_candidates(
    query: &str,
    entries: Vec<MemberEntry<'_>>,
    roles: Vec<&RoleState>,
    everyone_role_id: Option<Id<RoleMarker>>,
) -> Vec<MentionPickerEntry> {
    let needle = query.to_lowercase();
    let mut scored: Vec<(u8, String, MentionPickerEntry)> = entries
        .into_iter()
        .filter_map(|entry| {
            let display_name = entry.display_name();
            let username = entry.username();
            let lowered_display = display_name.to_lowercase();
            let lowered_username = username.as_deref().map(str::to_lowercase);

            // Lower rank wins. We deliberately stagger the ladder so an alias
            // prefix beats a username prefix and either beats a substring hit
            // on the other field.
            let rank = if needle.is_empty() {
                2
            } else if lowered_display.starts_with(&needle) {
                0
            } else if lowered_username
                .as_deref()
                .is_some_and(|name| name.starts_with(&needle))
            {
                1
            } else if lowered_display.contains(&needle) {
                2
            } else if lowered_username
                .as_deref()
                .is_some_and(|name| name.contains(&needle))
            {
                3
            } else {
                return None;
            };
            Some((
                rank,
                lowered_display,
                MentionPickerEntry {
                    target: MentionPickerTarget::User(entry.user_id()),
                    display_name,
                    username,
                    status: entry.status(),
                    is_bot: entry.is_bot(),
                    role_color: None,
                },
            ))
        })
        .collect();

    scored.extend(roles.into_iter().filter_map(|role| {
        let lowered_name = role.name.trim_start_matches('@').to_lowercase();
        let rank = match_name(&needle, &lowered_name)?;
        Some((
            rank.saturating_add(1),
            lowered_name,
            MentionPickerEntry {
                target: if Some(role.id) == everyone_role_id {
                    MentionPickerTarget::Everyone(role.id)
                } else {
                    MentionPickerTarget::Role(role.id)
                },
                display_name: role.name.clone(),
                username: None,
                status: PresenceStatus::Unknown,
                is_bot: false,
                role_color: role.color,
            },
        ))
    }));

    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

pub(super) fn build_channel_mention_candidates(
    query: &str,
    channels: Vec<&ChannelState>,
) -> Vec<MentionPickerEntry> {
    let needle = query.to_lowercase();
    let mut scored: Vec<(u8, String, MentionPickerEntry)> = channels
        .into_iter()
        .filter(|channel| !channel.is_category())
        .filter_map(|channel| {
            let lowered_name = channel.name.to_lowercase();
            let rank = match_name(&needle, &lowered_name)?;
            Some((
                rank,
                lowered_name,
                MentionPickerEntry {
                    target: MentionPickerTarget::Channel(channel.id),
                    display_name: channel.name.clone(),
                    username: None,
                    status: PresenceStatus::Unknown,
                    is_bot: false,
                    role_color: None,
                },
            ))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

fn match_name(needle: &str, lowered_name: &str) -> Option<u8> {
    if needle.is_empty() {
        Some(2)
    } else if lowered_name.starts_with(needle) {
        Some(0)
    } else if lowered_name.contains(needle) {
        Some(2)
    } else {
        None
    }
}

pub(super) fn move_picker_selection(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let current = selected.min(len - 1) as isize;
    (current + delta).clamp(0, len as isize - 1) as usize
}

pub(in crate::tui::state) fn build_emoji_candidates<'a>(
    query: &str,
    foreign_emojis: impl Iterator<Item = &'a CustomEmojiInfo>,
    guild_emojis: impl Iterator<Item = &'a CustomEmojiInfo>,
    has_nitro: bool,
    emojis_as_links: bool,
) -> Vec<EmojiPickerEntry> {
    let needle = query.to_ascii_lowercase();
    if needle.chars().count() < 2 {
        return Vec::new();
    }

    let make_entry = |is_foreign: bool| {
        move |emoji: &CustomEmojiInfo| {
            let can_send_directly = custom_emoji_can_be_used_directly(emoji, is_foreign, has_nitro);
            let as_link = emojis_as_links && !can_send_directly;
            let shortcode = emoji.name.clone();
            let marker = if emoji.animated { "◇" } else { "◆" };
            let label = if emoji.animated {
                "animated custom emoji"
            } else {
                "custom emoji"
            };
            (
                0,
                shortcode.to_ascii_lowercase(),
                EmojiPickerEntry {
                    emoji: marker.to_owned(),
                    shortcode: shortcode.clone(),
                    name: label.to_owned(),
                    wire_format: Some(custom_emoji_markup(
                        &shortcode,
                        emoji.id,
                        emoji.animated,
                        as_link,
                    )),
                    available: emoji.available && (can_send_directly || as_link),
                    available_as_link: emoji.available && as_link,
                    custom_image_url: Some(custom_emoji_image_url(emoji.id, emoji.animated)),
                },
            )
        }
    };
    let mut scored: Vec<(u8, String, EmojiPickerEntry)> = guild_emojis
        .filter(|emoji| emoji.name.to_ascii_lowercase().starts_with(&needle))
        .map(|emoji| make_entry(false)(emoji))
        .collect();

    if has_nitro || emojis_as_links {
        scored.extend(
            foreign_emojis
                .filter(|emoji| emoji.name.to_ascii_lowercase().starts_with(&needle))
                .map(|emoji| make_entry(true)(emoji)),
        );
    }

    scored.extend(emojis::iter().flat_map(|emoji| {
        emoji
            .shortcodes()
            .filter(|shortcode| shortcode.starts_with(&needle))
            .map(|shortcode| {
                (
                    1,
                    shortcode.to_owned(),
                    EmojiPickerEntry {
                        emoji: emoji.as_str().to_owned(),
                        shortcode: shortcode.to_owned(),
                        name: emoji.name().to_owned(),
                        wire_format: None,
                        available: true,
                        available_as_link: false,
                        custom_image_url: None,
                    },
                )
            })
    }));
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, entry)| entry).collect()
}

fn custom_emoji_markup(name: &str, id: Id<EmojiMarker>, animated: bool, as_link: bool) -> String {
    if as_link {
        let link = custom_emoji_image_url(id, animated);
        format!("[{name}]({link}?size=48&name={name}&lossless=true)")
    } else if animated {
        format!("<a:{name}:{}>", id.get())
    } else {
        format!("<:{name}:{}>", id.get())
    }
}

fn custom_emoji_image_url(id: Id<EmojiMarker>, animated: bool) -> String {
    let extension = if animated { "gif" } else { "png" };
    format!("https://cdn.discordapp.com/emojis/{}.{extension}", id.get())
}

pub(in crate::tui::state) fn should_start_completion_query(input: &str) -> bool {
    input.chars().last().is_none_or(char::is_whitespace)
}

pub(super) fn is_mention_query_char(value: char) -> bool {
    value.is_alphanumeric() || matches!(value, '_' | '.' | '-')
}

pub(in crate::tui::state) fn is_emoji_query_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '+')
}

pub(super) fn is_command_query_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-')
}

pub(in crate::tui::state) fn expand_emoji_shortcodes(input: &str) -> String {
    let mut rest = input;
    let mut output = String::with_capacity(input.len());

    while let Some((start, name_start, name_end, end)) = rest.find(':').and_then(|start| {
        let name_start = start + ':'.len_utf8();
        rest[name_start..].find(':').map(|relative_end| {
            (
                start,
                name_start,
                name_start + relative_end,
                name_start + relative_end + ':'.len_utf8(),
            )
        })
    }) {
        if starts_custom_emoji_markup(rest, start) {
            output.push_str(&rest[..name_start]);
            rest = &rest[name_start..];
            continue;
        }

        let shortcode = &rest[name_start..name_end];
        if shortcode.is_empty() {
            let colon_run_end = rest[start..]
                .char_indices()
                .find_map(|(offset, value)| (value != ':').then_some(start + offset))
                .unwrap_or(rest.len());
            let keep_to = if colon_run_end < rest.len() {
                colon_run_end - ':'.len_utf8()
            } else {
                colon_run_end
            };
            output.push_str(&rest[..keep_to]);
            rest = &rest[keep_to..];
            continue;
        }
        if let Some(emoji) = emojis::get_by_shortcode(shortcode) {
            output.push_str(&rest[..start]);
            output.push_str(emoji.as_str());
            rest = &rest[end..];
        } else {
            output.push_str(&rest[..name_end]);
            rest = &rest[name_end..];
        }
    }

    output.push_str(rest);
    output
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EmojiCompletion {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) replacement: String,
    pub(super) custom_image_url: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(in crate::tui) struct ComposerEmojiImageCompletion {
    pub(in crate::tui) byte_start: usize,
    pub(in crate::tui) byte_end: usize,
    pub(in crate::tui) url: String,
}

fn starts_custom_emoji_markup(input: &str, colon_start: usize) -> bool {
    input[..colon_start].ends_with('<') || input[..colon_start].ends_with("<a")
}

/// A previously confirmed mention recorded by byte range inside the composer
/// input. The composer keeps the human-readable `@displayname` text in the
/// editor so the user can see what they wrote, and rewrites these ranges to
/// `<@USER_ID>` only at submission time.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(in crate::tui::state) struct MentionCompletion {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) target: MentionPickerTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui::state) enum MentionExpansionMode {
    Message,
    Command,
}

/// Rewrites recorded mention and custom emoji ranges in one back-to-front pass.
/// Both completion kinds store byte ranges against the visible composer text, so
/// applying them together prevents earlier replacements from shifting later
/// ranges before they are used.
pub(super) fn expand_composer_completions(
    input: &str,
    mention_completions: &[MentionCompletion],
    emoji_completions: &[EmojiCompletion],
    mention_mode: MentionExpansionMode,
) -> String {
    if mention_completions.is_empty() && emoji_completions.is_empty() {
        return input.to_owned();
    }

    let mut replacements: Vec<CompletionReplacement> = mention_completions
        .iter()
        .filter(|completion| completion.byte_end <= input.len())
        .map(|completion| CompletionReplacement {
            byte_start: completion.byte_start,
            byte_end: completion.byte_end,
            replacement: match mention_mode {
                MentionExpansionMode::Message => completion.target.wire_format(),
                MentionExpansionMode::Command => completion.target.command_wire_format(),
            },
        })
        .collect();

    replacements.extend(
        emoji_completions
            .iter()
            .filter(|completion| completion.byte_end <= input.len())
            .map(|completion| CompletionReplacement {
                byte_start: completion.byte_start,
                byte_end: completion.byte_end,
                replacement: completion.replacement.clone(),
            }),
    );

    replacements.sort_by_key(|replacement| std::cmp::Reverse(replacement.byte_start));
    let mut buffer = input.to_owned();
    for replacement in replacements {
        if !buffer.is_char_boundary(replacement.byte_start)
            || !buffer.is_char_boundary(replacement.byte_end)
        {
            continue;
        }
        buffer.replace_range(
            replacement.byte_start..replacement.byte_end,
            &replacement.replacement,
        );
    }
    buffer
}

struct CompletionReplacement {
    byte_start: usize,
    byte_end: usize,
    replacement: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(id: u64, name: &str) -> RoleState {
        RoleState {
            id: Id::new(id),
            name: name.to_owned(),
            color: None,
            position: 0,
            hoist: false,
            permissions: 0,
        }
    }

    #[test]
    fn everyone_role_detection_uses_id_not_name() {
        let everyone = role(1, "renamed base role");
        let candidates =
            build_mention_candidates("", Vec::new(), vec![&everyone], Some(Id::new(1)));

        assert!(
            candidates
                .iter()
                .any(|entry| entry.target == MentionPickerTarget::Everyone(Id::new(1)))
        );
    }
}
