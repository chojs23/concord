use crate::discord::ids::{Id, marker::RoleMarker};
use ratatui::style::Color;

use crate::discord::{
    ChannelRecipientState, ChannelState, GuildMemberState, PresenceStatus, RoleState,
};

/// Convert a Discord folder color (24-bit RGB integer) to a ratatui color.
/// Falls back to a neutral cyan when the color is missing or zero so
/// uncolored folders still read as folder headers.
pub fn folder_color(color: Option<u32>) -> Color {
    discord_color(color, Color::Cyan)
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

pub fn presence_color(status: PresenceStatus) -> Color {
    match status {
        PresenceStatus::Online => Color::Green,
        PresenceStatus::Idle => Color::Rgb(180, 140, 0),
        PresenceStatus::DoNotDisturb => Color::Red,
        PresenceStatus::Offline => Color::DarkGray,
        PresenceStatus::Unknown => Color::DarkGray,
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

pub(super) fn primary_hoisted_role(
    member: &GuildMemberState,
    roles: &[&RoleState],
) -> Option<Id<RoleMarker>> {
    member
        .role_ids
        .iter()
        .filter_map(|role_id| roles.iter().find(|role| role.id == *role_id).copied())
        .filter(|role| role.hoist)
        .min_by(|left, right| role_display_order(left, right))
        .map(|role| role.id)
}

fn role_display_order(left: &RoleState, right: &RoleState) -> std::cmp::Ordering {
    right
        .position
        .cmp(&left.position)
        .then(left.id.get().cmp(&right.id.get()))
}

pub(super) fn sort_member_entries(entries: &mut [&GuildMemberState]) {
    entries.sort_by(|left, right| {
        member_status_rank(left.status)
            .cmp(&member_status_rank(right.status))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
    });
}

pub(super) fn sort_recipient_entries(entries: &mut [&ChannelRecipientState]) {
    entries.sort_by(|left, right| {
        member_status_rank(left.status)
            .cmp(&member_status_rank(right.status))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
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
        PresenceStatus::Offline | PresenceStatus::Unknown => ' ',
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
