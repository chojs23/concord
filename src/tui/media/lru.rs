use std::{collections::HashMap, hash::Hash};

pub(super) fn next_tick(tick: &mut u64) -> u64 {
    *tick = tick.saturating_add(1);
    *tick
}

pub(super) fn prune_to_limit<K, V>(
    entries: &mut HashMap<K, V>,
    limit: usize,
    is_protected: impl Fn(&K) -> bool,
    last_used: impl Fn(&V) -> u64,
) where
    K: Clone + Eq + Hash,
{
    if entries.len() <= limit {
        return;
    }

    let mut removable = entries
        .iter()
        .filter(|(key, _)| !is_protected(key))
        .map(|(key, entry)| (key.clone(), last_used(entry)))
        .collect::<Vec<_>>();
    removable.sort_by_key(|(_, last_used)| *last_used);

    for (key, _) in removable {
        if entries.len() <= limit {
            break;
        }
        entries.remove(&key);
    }
}
