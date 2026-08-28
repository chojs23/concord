//! Ordered Gateway member-list rows and atomic refresh handling.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::discord::{GuildMemberListItem, GuildMemberListOperation, GuildMemberListUpdateInfo};

/// One ordered row from Discord's streamed guild member list.
///
/// Member details stay in the guild member entity cache. This type only keeps
/// the server-provided list structure used by the member pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuildMemberListEntry {
    Group { id: String, count: u64 },
    Member { user_id: Id<UserMarker> },
}

#[derive(Clone, Debug, Default)]
struct GuildMemberListSnapshot {
    list_id: Option<String>,
    entries: BTreeMap<u32, GuildMemberListEntry>,
    synced_ranges: Vec<(u32, u32)>,
    total_items: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::discord) struct GuildMemberListState {
    /// The last complete list shown by the TUI.
    stable: Option<GuildMemberListSnapshot>,
    /// A replacement list that is not visible until all requested ranges sync.
    refreshing: Option<GuildMemberListSnapshot>,
    /// Ranges from the latest Opcode 37 subscription request.
    requested_ranges: Vec<(u32, u32)>,
    /// Channel used by the current subscription request.
    ///
    /// Member-list updates do not contain a channel ID or request nonce. This
    /// value therefore controls whether the stable snapshot can seed a refresh.
    /// It is not used to claim that an incoming update belongs to a channel.
    subscription_channel_id: Option<Id<ChannelMarker>>,
    refresh_generation: u64,
    /// Prevents one incomplete response from scheduling duplicate refreshes.
    refresh_pending: bool,
}

impl GuildMemberListState {
    pub(super) fn entries(&self) -> Vec<(u32, &GuildMemberListEntry)> {
        self.stable
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .entries
                    .iter()
                    .map(|(index, entry)| (*index, entry))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn refresh_generation(&self) -> u64 {
        self.refresh_generation
    }

    pub(super) fn prepare_subscription(
        &mut self,
        channel_id: Id<ChannelMarker>,
        ranges: Vec<(u32, u32)>,
    ) {
        self.requested_ranges = ranges;
        self.refresh_pending = false;
        let same_channel = self.subscription_channel_id == Some(channel_id);
        self.subscription_channel_id = Some(channel_id);

        if same_channel && self.refreshing.is_some() {
            return;
        }

        if same_channel
            && self
                .stable
                .as_ref()
                .is_some_and(|snapshot| snapshot.has_ranges(&self.requested_ranges))
        {
            self.refreshing = None;
            return;
        }

        self.refreshing = Some(if same_channel {
            self.stable.clone().unwrap_or_default()
        } else {
            GuildMemberListSnapshot::default()
        });
    }

    pub(super) fn apply(&mut self, update: &GuildMemberListUpdateInfo) {
        // Discord can reuse the same list id after a new channel subscription.
        // An explicitly prepared refresh therefore owns the next matching
        // response instead of letting a partial response overwrite `stable`.
        let refreshing_matches = self.refreshing.as_ref().is_some_and(|snapshot| {
            snapshot.list_id.is_none() || snapshot.list_id == update.list_id
        });
        let stable_matches = self.stable.as_ref().is_some_and(|snapshot| {
            snapshot.list_id.is_some() && snapshot.list_id == update.list_id
        });
        let use_refreshing = refreshing_matches
            || (!stable_matches && self.refreshing.is_some())
            || (!stable_matches && (update.list_id.is_some() || self.stable.is_none()));

        if use_refreshing {
            if self.refreshing.is_none() {
                self.refreshing = Some(GuildMemberListSnapshot::default());
            }

            let contains_invalidation = update
                .ops
                .iter()
                .any(|operation| matches!(operation, GuildMemberListOperation::Invalidate { .. }));
            let understood = self
                .refreshing
                .as_mut()
                .expect("refreshing member list exists")
                .apply(update);
            if self.requested_ranges.is_empty() {
                self.requested_ranges = synced_ranges(update);
            }

            // A cold snapshot can receive its first authoritative SYNC before
            // local subscription metadata is available. Its declared ranges
            // are the only completion boundary present in that event.
            let complete = understood
                && !self.requested_ranges.is_empty()
                && self
                    .refreshing
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.has_ranges(&self.requested_ranges));
            if complete {
                self.stable = self.refreshing.take();
                self.refresh_pending = false;
            } else if !understood || contains_invalidation {
                self.require_refresh();
            }
            return;
        }

        let Some(mut updated) = self.stable.clone() else {
            return;
        };
        // Normal same-list deltas are applied to a clone and swapped in as one
        // unit. Invalid or unknown ranges become a refresh while `stable`
        // remains visible.
        let contains_invalidation = update
            .ops
            .iter()
            .any(|operation| matches!(operation, GuildMemberListOperation::Invalidate { .. }));
        let understood = updated.apply(update);
        if understood && !contains_invalidation {
            self.stable = Some(updated);
        } else {
            self.refreshing = Some(updated);
            self.require_refresh();
        }
    }

    pub(super) fn reset_from_snapshot(&mut self, member_count: Option<u64>) {
        self.begin_full_refresh();
        if let Some(refreshing) = self.refreshing.as_mut() {
            refreshing.total_items =
                member_count.map(|count| u32::try_from(count).unwrap_or(u32::MAX));
        }
    }

    pub(super) fn begin_full_refresh(&mut self) {
        self.refreshing = Some(GuildMemberListSnapshot::default());
        self.require_refresh();
    }

    pub(super) fn clear(&mut self) {
        self.stable = None;
        self.refreshing = None;
        self.requested_ranges.clear();
        self.subscription_channel_id = None;
        self.require_refresh();
    }

    pub(super) fn remove_user(&mut self, user_id: Id<UserMarker>) {
        let stable_changed = self
            .stable
            .as_mut()
            .is_some_and(|snapshot| snapshot.remove_user(user_id));
        let refreshing_changed = self
            .refreshing
            .as_mut()
            .is_some_and(|snapshot| snapshot.remove_user(user_id));
        if stable_changed || refreshing_changed {
            self.require_refresh();
        }
    }

    fn require_refresh(&mut self) {
        if self.refresh_pending {
            return;
        }
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.refresh_pending = true;
    }

    pub(super) fn has_ranges(&self, ranges: &[(u32, u32)]) -> bool {
        self.refreshing
            .as_ref()
            .or(self.stable.as_ref())
            .is_some_and(|snapshot| snapshot.has_ranges(ranges))
    }
}

impl GuildMemberListSnapshot {
    fn apply(&mut self, update: &GuildMemberListUpdateInfo) -> bool {
        if let Some(list_id) = update.list_id.as_ref()
            && self.list_id.as_ref() != Some(list_id)
        {
            self.list_id = Some(list_id.clone());
            self.entries.clear();
            self.synced_ranges.clear();
            self.total_items = None;
        }

        let total_is_authoritative = update.member_count.is_some();
        if let Some(member_count) = update.member_count {
            let member_count = u32::try_from(member_count).unwrap_or(u32::MAX);
            let group_count = u32::try_from(update.groups.len()).unwrap_or(u32::MAX);
            self.total_items = Some(member_count.saturating_add(group_count));
        }

        let mut understood = true;
        for operation in &update.ops {
            match operation {
                GuildMemberListOperation::Sync { range, items } => {
                    understood &= self.sync_range(*range, items);
                }
                GuildMemberListOperation::Insert { index, item } => {
                    let Some(entry) = item_entry(item) else {
                        self.invalidate_all();
                        understood = false;
                        continue;
                    };
                    self.insert(*index, entry);
                    if !total_is_authoritative {
                        self.total_items = self.total_items.map(|total| total.saturating_add(1));
                    }
                }
                GuildMemberListOperation::Update { index, item } => {
                    let Some(entry) = item_entry(item) else {
                        self.invalidate_all();
                        understood = false;
                        continue;
                    };
                    self.entries.insert(*index, entry);
                }
                GuildMemberListOperation::Delete { index } => {
                    self.delete(*index);
                    if !total_is_authoritative {
                        self.total_items = self.total_items.map(|total| total.saturating_sub(1));
                    }
                }
                GuildMemberListOperation::Invalidate { range } => self.invalidate_range(*range),
                GuildMemberListOperation::Unknown { .. } => {
                    self.invalidate_all();
                    understood = false;
                }
            }
        }
        self.update_group_counts(&update.groups);
        understood
    }

    fn update_group_counts(&mut self, groups: &[Value]) {
        for group in groups {
            let (Some(id), Some(count)) = (
                group.get("id").and_then(Value::as_str),
                group.get("count").and_then(Value::as_u64),
            ) else {
                continue;
            };
            for entry in self.entries.values_mut() {
                if let GuildMemberListEntry::Group {
                    id: entry_id,
                    count: entry_count,
                } = entry
                    && entry_id == id
                {
                    *entry_count = count;
                }
            }
        }
    }

    fn sync_range(&mut self, (start, end): (u32, u32), items: &[GuildMemberListItem]) -> bool {
        self.entries
            .retain(|index, _| *index < start || *index > end);
        self.remove_synced_range((start, end));
        let mut understood = true;
        for (offset, item) in items.iter().enumerate() {
            let Ok(offset) = u32::try_from(offset) else {
                understood = false;
                break;
            };
            let index = start.saturating_add(offset);
            if index > end {
                break;
            }
            if let Some(entry) = item_entry(item) {
                self.entries.insert(index, entry);
            } else {
                understood = false;
            }
        }
        if understood {
            self.mark_range_synced((start, end));
        }
        understood
    }

    fn insert(&mut self, index: u32, entry: GuildMemberListEntry) {
        let previous = std::mem::take(&mut self.entries);
        self.entries = previous
            .into_iter()
            .map(|(current, item)| {
                let shifted = if current >= index {
                    current.saturating_add(1)
                } else {
                    current
                };
                (shifted, item)
            })
            .collect();
        self.entries.insert(index, entry);
        self.shift_synced_ranges_for_insert(index);
        self.mark_range_synced((index, index));
    }

    fn delete(&mut self, index: u32) {
        let previous = std::mem::take(&mut self.entries);
        self.entries = previous
            .into_iter()
            .filter_map(|(current, item)| {
                if current == index {
                    None
                } else {
                    Some((
                        if current > index {
                            current - 1
                        } else {
                            current
                        },
                        item,
                    ))
                }
            })
            .collect();
        self.shift_synced_ranges_for_delete(index);
    }

    fn shift_synced_ranges_for_insert(&mut self, index: u32) {
        for (start, end) in &mut self.synced_ranges {
            if *end < index {
                continue;
            }
            if *start >= index {
                *start = start.saturating_add(1);
            }
            *end = end.saturating_add(1);
        }
        self.merge_synced_ranges();
    }

    fn shift_synced_ranges_for_delete(&mut self, index: u32) {
        self.synced_ranges = self
            .synced_ranges
            .iter()
            .filter_map(|(start, end)| {
                if *end < index {
                    return Some((*start, *end));
                }
                if *start > index {
                    return Some((start - 1, end - 1));
                }
                (*start < *end).then_some((*start, end - 1))
            })
            .collect();
        self.merge_synced_ranges();
    }

    fn invalidate_range(&mut self, (start, end): (u32, u32)) {
        self.entries
            .retain(|index, _| *index < start || *index > end);
        self.remove_synced_range((start, end));
    }

    fn remove_synced_range(&mut self, (start, end): (u32, u32)) {
        self.synced_ranges = self
            .synced_ranges
            .iter()
            .flat_map(|(synced_start, synced_end)| {
                let mut retained = Vec::with_capacity(2);
                if *synced_end < start || *synced_start > end {
                    retained.push((*synced_start, *synced_end));
                } else {
                    if *synced_start < start {
                        retained.push((*synced_start, start - 1));
                    }
                    if *synced_end > end {
                        retained.push((end.saturating_add(1), *synced_end));
                    }
                }
                retained
            })
            .collect();
    }

    fn invalidate_all(&mut self) {
        self.entries.clear();
        self.synced_ranges.clear();
    }

    fn remove_user(&mut self, user_id: Id<UserMarker>) -> bool {
        let indexes = self
            .entries
            .iter()
            .filter_map(|(index, entry)| {
                matches!(entry, GuildMemberListEntry::Member { user_id: listed } if *listed == user_id)
                    .then_some(*index)
            })
            .collect::<Vec<_>>();
        for index in &indexes {
            self.entries.remove(index);
            self.remove_synced_range((*index, *index));
        }
        !indexes.is_empty()
    }

    fn has_ranges(&self, ranges: &[(u32, u32)]) -> bool {
        ranges.iter().all(|(start, end)| {
            if start > end || self.total_items.is_some_and(|total| *start >= total) {
                return true;
            }
            let required_end = self
                .total_items
                .map(|total| (*end).min(total.saturating_sub(1)))
                .unwrap_or(*end);
            self.synced_ranges.iter().any(|(synced_start, synced_end)| {
                *synced_start <= *start && *synced_end >= required_end
            })
        })
    }

    fn mark_range_synced(&mut self, range: (u32, u32)) {
        self.synced_ranges.push(range);
        self.merge_synced_ranges();
    }

    fn merge_synced_ranges(&mut self) {
        self.synced_ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(self.synced_ranges.len());
        for (start, end) in self.synced_ranges.drain(..) {
            if let Some((_, merged_end)) = merged.last_mut()
                && start <= merged_end.saturating_add(1)
            {
                *merged_end = (*merged_end).max(end);
            } else {
                merged.push((start, end));
            }
        }
        self.synced_ranges = merged;
    }
}

fn synced_ranges(update: &GuildMemberListUpdateInfo) -> Vec<(u32, u32)> {
    update
        .ops
        .iter()
        .filter_map(|operation| match operation {
            GuildMemberListOperation::Sync { range, .. } => Some(*range),
            _ => None,
        })
        .collect()
}

fn item_entry(item: &GuildMemberListItem) -> Option<GuildMemberListEntry> {
    match item {
        GuildMemberListItem::Member { member, .. } => Some(GuildMemberListEntry::Member {
            user_id: member.user_id,
        }),
        GuildMemberListItem::Group { id, count } => Some(GuildMemberListEntry::Group {
            id: id.clone(),
            count: *count,
        }),
        GuildMemberListItem::Unknown { .. } => None,
    }
}
