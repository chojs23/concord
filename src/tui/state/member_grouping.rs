use crate::discord::ids::{
    Id,
    marker::{RoleMarker, UserMarker},
};

use crate::discord::{
    ChannelRecipientState, ChannelState, GuildMemberListEntry, GuildMemberState, PresenceStatus,
    RoleState,
};

use super::DashboardState;
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

pub(in crate::tui) enum MemberRow<'groups, 'members> {
    Gap,
    GroupHeader(&'groups MemberGroup<'members>),
    Member {
        member_index: usize,
        entry: MemberEntry<'members>,
    },
    Activity {
        member_index: usize,
        entry: MemberEntry<'members>,
    },
}

pub(in crate::tui) struct MemberRows<'state, 'groups, 'members> {
    state: &'state DashboardState,
    groups: &'groups [MemberGroup<'members>],
    group_index: usize,
    entry_index: usize,
    member_index: usize,
    emit_gap: bool,
    emit_header: bool,
    pending_activity: Option<(usize, MemberEntry<'members>)>,
}

impl<'state, 'groups, 'members> MemberRows<'state, 'groups, 'members> {
    pub(in crate::tui) fn new(
        state: &'state DashboardState,
        groups: &'groups [MemberGroup<'members>],
    ) -> Self {
        Self {
            state,
            groups,
            group_index: 0,
            entry_index: 0,
            member_index: 0,
            emit_gap: false,
            emit_header: true,
            pending_activity: None,
        }
    }
}

impl<'groups, 'members> Iterator for MemberRows<'_, 'groups, 'members> {
    type Item = MemberRow<'groups, 'members>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((member_index, entry)) = self.pending_activity.take() {
                self.entry_index += 1;
                self.member_index += 1;
                return Some(MemberRow::Activity {
                    member_index,
                    entry,
                });
            }

            let group = self.groups.get(self.group_index)?;
            if self.emit_gap {
                self.emit_gap = false;
                self.emit_header = true;
                return Some(MemberRow::Gap);
            }
            if self.emit_header {
                self.emit_header = false;
                return Some(MemberRow::GroupHeader(group));
            }
            if let Some(entry) = group.entries.get(self.entry_index).copied() {
                let member_index = self.member_index;
                if member_has_activity_row(self.state, entry) {
                    self.pending_activity = Some((member_index, entry));
                } else {
                    self.entry_index += 1;
                    self.member_index += 1;
                }
                return Some(MemberRow::Member {
                    member_index,
                    entry,
                });
            }

            self.group_index += 1;
            self.entry_index = 0;
            if self.group_index < self.groups.len() {
                self.emit_gap = true;
            }
        }
    }
}

pub(super) fn member_has_activity_row(state: &DashboardState, member: MemberEntry<'_>) -> bool {
    !matches!(
        member.status(),
        PresenceStatus::Offline | PresenceStatus::Unknown
    ) && !state.user_activities(member.user_id()).is_empty()
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
