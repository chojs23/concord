use ratatui::style::{Color, Style};

use crate::discord::{
    ChannelRecipientState, ChannelState, GuildMemberState, PresenceStatus, RoleState,
};
use crate::tui::theme;

/// Keep the configured folder style while allowing Discord to supply its
/// foreground when a folder has a nonzero source color.
pub fn folder_style(color: Option<u32>) -> Style {
    apply_discord_foreground(
        theme::current().style(theme::HighlightGroup::FolderFallback),
        color,
    )
}

pub fn discord_color(color: Option<u32>, fallback: Color) -> Color {
    match color {
        Some(value) if value != 0 => {
            let r = ((value >> 16) & 0xFF) as u8;
            let g = ((value >> 8) & 0xFF) as u8;
            let b = (value & 0xFF) as u8;
            Color::Rgb(r, g, b)
        }
        _ => fallback,
    }
}

pub fn apply_discord_foreground(style: Style, color: Option<u32>) -> Style {
    match color {
        Some(value) if value != 0 => style.fg(discord_color(Some(value), Color::Reset)),
        _ => style,
    }
}

pub fn normal_text_style() -> Style {
    let mut style = theme::current().style(theme::HighlightGroup::Normal);
    style.bg = None;
    style
}

pub fn discord_role_mention_background(color: u32) -> Color {
    const ROLE_PERCENT: u32 = 40;
    const BACKGROUND_PERCENT: u32 = 100 - ROLE_PERCENT;
    let (background_red, background_green, background_blue) = match theme::current()
        .style(theme::HighlightGroup::Normal)
        .bg
    {
        Some(Color::Rgb(red, green, blue)) => (u32::from(red), u32::from(green), u32::from(blue)),
        _ => (0, 0, 0),
    };
    let blend = |role: u32, background: u32| {
        ((role * ROLE_PERCENT + background * BACKGROUND_PERCENT) / 100) as u8
    };
    Color::Rgb(
        blend((color >> 16) & 0xFF, background_red),
        blend((color >> 8) & 0xFF, background_green),
        blend(color & 0xFF, background_blue),
    )
}

pub fn presence_style(status: PresenceStatus) -> Style {
    let theme = theme::current();
    match status {
        PresenceStatus::Online => theme.style(theme::HighlightGroup::PresenceOnline),
        PresenceStatus::Idle => theme.style(theme::HighlightGroup::PresenceIdle),
        PresenceStatus::DoNotDisturb => theme.style(theme::HighlightGroup::PresenceDnd),
        PresenceStatus::Offline | PresenceStatus::Unknown => {
            theme.style(theme::HighlightGroup::PresenceOffline)
        }
    }
}

pub(super) fn is_online_status(status: PresenceStatus) -> bool {
    matches!(
        status,
        PresenceStatus::Online | PresenceStatus::Idle | PresenceStatus::DoNotDisturb
    )
}

pub(super) fn sorted_hoisted_roles<'a>(roles: &'a [&'a RoleState]) -> Vec<&'a RoleState> {
    let mut roles: Vec<&RoleState> = roles.iter().copied().filter(|role| role.hoist).collect();
    roles.sort_by(|left, right| role_display_order(left, right));
    roles
}

fn role_display_order(left: &RoleState, right: &RoleState) -> std::cmp::Ordering {
    right
        .position
        .cmp(&left.position)
        .then(left.id.get().cmp(&right.id.get()))
}

pub(super) fn sort_member_entries(entries: &mut [&GuildMemberState]) {
    entries.sort_by_cached_key(|member| {
        (
            member_status_rank(member.status),
            member.display_name.to_lowercase(),
        )
    });
}

pub(super) fn sort_recipient_entries(entries: &mut [&ChannelRecipientState]) {
    entries.sort_by_cached_key(|recipient| {
        (
            member_status_rank(recipient.status),
            recipient.display_name.to_lowercase(),
        )
    });
}

pub(super) fn is_direct_message_channel(channel: &ChannelState) -> bool {
    matches!(
        channel.kind.as_str(),
        "dm" | "Private" | "group-dm" | "Group"
    )
}

fn member_status_rank(status: PresenceStatus) -> u8 {
    match status {
        PresenceStatus::Online => 0,
        PresenceStatus::Idle => 1,
        PresenceStatus::DoNotDisturb => 2,
        PresenceStatus::Offline => 3,
        PresenceStatus::Unknown => 4,
    }
}

pub fn presence_marker(status: PresenceStatus) -> char {
    match status {
        PresenceStatus::Online | PresenceStatus::Idle | PresenceStatus::DoNotDisturb => '●',
        PresenceStatus::Offline | PresenceStatus::Unknown => '○',
    }
}

pub(super) fn sort_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| (channel.position.unwrap_or(i32::MAX), channel.id));
}

pub(super) fn sort_direct_message_channels(channels: &mut [&ChannelState]) {
    channels.sort_by(|left, right| {
        right
            .last_message_id
            .cmp(&left.last_message_id)
            .then_with(|| right.id.cmp(&left.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_marker_shows_empty_circle_for_offline_like_statuses() {
        assert_eq!(presence_marker(PresenceStatus::Offline), '○');
        assert_eq!(presence_marker(PresenceStatus::Unknown), '○');
    }

    #[test]
    fn presence_marker_shows_filled_circle_for_online_like_statuses() {
        for status in [
            PresenceStatus::Online,
            PresenceStatus::Idle,
            PresenceStatus::DoNotDisturb,
        ] {
            assert_eq!(presence_marker(status), '●');
        }
    }
}
