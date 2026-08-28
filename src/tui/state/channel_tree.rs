use std::collections::HashSet;

use crate::discord::ChannelState;
use crate::discord::ids::{Id, marker::ChannelMarker};

use super::{
    model::{ChannelBranch, ChannelPaneEntry},
    presentation::sort_channels,
};

pub(super) fn sorted_channel_tree_roots<'a>(
    channels: &[&'a ChannelState],
) -> Vec<&'a ChannelState> {
    let category_ids = channel_category_ids(channels);
    let mut roots: Vec<&ChannelState> = channels
        .iter()
        .copied()
        .filter(|channel| {
            channel.is_category()
                || channel
                    .parent_id
                    .is_none_or(|parent_id| !category_ids.contains(&parent_id))
                    && !channel.is_thread()
        })
        .collect();
    sort_channels(&mut roots);
    roots
}

pub(super) fn sorted_category_children<'a>(
    channels: &[&'a ChannelState],
    category_id: Id<ChannelMarker>,
) -> Vec<&'a ChannelState> {
    let mut children: Vec<&ChannelState> = channels
        .iter()
        .copied()
        .filter(|channel| {
            !channel.is_category() && !channel.is_thread() && channel.parent_id == Some(category_id)
        })
        .collect();
    sort_channels(&mut children);
    children
}

pub(super) fn child_branch(index: usize, len: usize) -> ChannelBranch {
    if index == len.saturating_sub(1) {
        ChannelBranch::Last
    } else {
        ChannelBranch::Middle
    }
}

pub(super) fn sorted_child_threads<'a>(
    channels: impl IntoIterator<Item = &'a ChannelState>,
    parent_id: Id<ChannelMarker>,
) -> Vec<&'a ChannelState> {
    let mut threads: Vec<&ChannelState> = channels
        .into_iter()
        .filter(|channel| channel.is_thread() && channel.parent_id == Some(parent_id))
        .collect();
    sort_thread_channels(&mut threads);
    threads
}

pub(super) fn preceding_category_id(
    entries: &[ChannelPaneEntry<'_>],
    selected: usize,
) -> Option<Id<ChannelMarker>> {
    entries
        .get(..selected)?
        .iter()
        .rev()
        .find_map(|entry| match entry {
            ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
            _ => None,
        })
}

fn channel_category_ids(channels: &[&ChannelState]) -> HashSet<Id<ChannelMarker>> {
    channels
        .iter()
        .filter(|channel| channel.is_category())
        .map(|channel| channel.id)
        .collect()
}

pub(super) fn sort_thread_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| std::cmp::Reverse(channel.id));
}
