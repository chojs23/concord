use crate::discord::ids::{
    Id,
    marker::{RoleMarker, UserMarker},
};

use crate::discord::{
    ChannelRecipientState, ChannelState, GuildMemberListEntry, GuildMemberState, RoleState,
};

use super::presentation::{is_direct_message_channel, member_status_rank, sort_recipient_entries};

#[derive(Debug)]
pub struct MemberGroup<'a> {
    pub label: String,
    pub color: Option<u32>,
    pub count: u64,
    pub entries: Vec<MemberEntry<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub enum MemberEntry<'a> {
    Guild(&'a GuildMemberState),
    Recipient(&'a ChannelRecipientState),
}

impl MemberEntry<'_> {
    pub fn user_id(self) -> Id<UserMarker> {
        match self {
            Self::Guild(member) => member.user_id,
            Self::Recipient(recipient) => recipient.user_id,
        }
    }

    pub fn display_name(self) -> String {
        match self {
            Self::Guild(member) => member.display_name.clone(),
            Self::Recipient(recipient) => recipient.display_name.clone(),
        }
    }

    /// Discord login handle (username), distinct from `display_name` which
    /// already prefers the per-server alias / global display name.
    pub fn username(self) -> Option<String> {
        match self {
            Self::Guild(member) => member.username.clone(),
            Self::Recipient(recipient) => recipient.username.clone(),
        }
    }

    pub fn member_search_alias(self) -> Option<String> {
        match self {
            Self::Guild(member) => member.nickname.clone(),
            // Private-channel recipients are local-only candidates. Their
            // display name remains searchable even though Opcode 8 is not used.
            Self::Recipient(recipient) => Some(recipient.display_name.clone()),
        }
    }

    pub fn has_fallback_identity(self) -> bool {
        match self {
            Self::Guild(member) => member.username.is_none() && member.display_name == "unknown",
            Self::Recipient(recipient) => {
                recipient.username.is_none() && recipient.display_name == "unknown"
            }
        }
    }

    pub fn is_bot(self) -> bool {
        match self {
            Self::Guild(member) => member.is_bot,
            Self::Recipient(recipient) => recipient.is_bot,
        }
    }

    pub fn status(self) -> crate::discord::PresenceStatus {
        match self {
            Self::Guild(member) => member.status,
            Self::Recipient(recipient) => recipient.status,
        }
    }
}

pub(super) fn guild_member_groups<'a>(
    list_entries: Vec<(u32, &GuildMemberListEntry)>,
    member_for_id: impl Fn(Id<UserMarker>) -> Option<&'a GuildMemberState>,
    role_for_id: impl Fn(Id<RoleMarker>) -> Option<&'a RoleState>,
) -> Vec<MemberGroup<'a>> {
    let mut groups = Vec::new();
    let mut current_group_index = None;
    let mut current_group_is_implicit = false;
    let mut previous_entry_index: Option<u32> = None;
    for (entry_index, entry) in list_entries {
        if previous_entry_index.is_some_and(|previous| previous.checked_add(1) != Some(entry_index))
        {
            current_group_index = None;
            current_group_is_implicit = false;
        }

        match entry {
            GuildMemberListEntry::Group { id, count } => {
                let (label, color) = member_group_presentation(id, &role_for_id);
                groups.push(MemberGroup {
                    label,
                    color,
                    count: *count,
                    entries: Vec::new(),
                });
                current_group_index = Some(groups.len() - 1);
                current_group_is_implicit = false;
            }
            GuildMemberListEntry::Member { user_id } => {
                let Some(member) = member_for_id(*user_id) else {
                    previous_entry_index = Some(entry_index);
                    continue;
                };
                if current_group_index.is_none() {
                    groups.push(MemberGroup {
                        label: "Members".to_owned(),
                        color: None,
                        count: 0,
                        entries: Vec::new(),
                    });
                    current_group_index = Some(groups.len() - 1);
                    current_group_is_implicit = true;
                }
                let group = groups
                    .get_mut(current_group_index.expect("member group index exists"))
                    .expect("member group exists");
                if current_group_is_implicit {
                    group.count = group.count.saturating_add(1);
                }
                group.entries.push(MemberEntry::Guild(member));
            }
        }
        previous_entry_index = Some(entry_index);
    }
    groups
}

fn member_group_presentation<'a>(
    id: &str,
    role_for_id: &impl Fn(Id<RoleMarker>) -> Option<&'a RoleState>,
) -> (String, Option<u32>) {
    match id {
        "online" => ("Online".to_owned(), None),
        "offline" => ("Offline".to_owned(), None),
        _ => id
            .parse::<u64>()
            .ok()
            .and_then(Id::<RoleMarker>::new_checked)
            .and_then(role_for_id)
            .map(|role| (role.name.clone(), role.color))
            .unwrap_or_else(|| ("Members".to_owned(), None)),
    }
}

pub(super) fn channel_recipient_group(channel: &ChannelState) -> Vec<MemberGroup<'_>> {
    if !is_direct_message_channel(channel) || channel.recipients.is_empty() {
        return Vec::new();
    }

    let mut recipients: Vec<&ChannelRecipientState> = channel.recipients.iter().collect();
    sort_recipient_entries(&mut recipients);
    vec![MemberGroup {
        label: "Members".to_owned(),
        color: None,
        count: recipients.len() as u64,
        entries: recipients.into_iter().map(MemberEntry::Recipient).collect(),
    }]
}

pub(super) fn thread_member_group(mut members: Vec<&GuildMemberState>) -> Vec<MemberGroup<'_>> {
    if members.is_empty() {
        return Vec::new();
    }
    members.sort_by_cached_key(|member| {
        (
            member_status_rank(member.status),
            member.display_name.to_lowercase(),
        )
    });
    vec![MemberGroup {
        label: "Members".to_owned(),
        color: None,
        count: u64::try_from(members.len()).unwrap_or(u64::MAX),
        entries: members.into_iter().map(MemberEntry::Guild).collect(),
    }]
}

pub(super) fn flatten_member_groups(groups: Vec<MemberGroup<'_>>) -> Vec<MemberEntry<'_>> {
    groups.into_iter().flat_map(|group| group.entries).collect()
}
