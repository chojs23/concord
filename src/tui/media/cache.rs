use std::{collections::HashMap, hash::Hash, sync::Arc};

use super::decode::{MediaImageDecodeJob, MediaImageDecodeKey};

pub(super) trait MediaImageCacheEntry {
    fn last_used(&self) -> u64;
    fn touch(&mut self, tick: u64);
    fn is_loading(&self) -> bool;
    fn decoding_generation(&self) -> Option<u64>;
}

pub(super) struct MediaImageCacheCore<K, E> {
    pub(super) entries: HashMap<K, E>,
    pub(super) tick: u64,
    pub(super) decode_generation: u64,
}

impl<K, E> MediaImageCacheCore<K, E>
where
    K: Clone + Eq + Hash,
    E: MediaImageCacheEntry,
{
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
            decode_generation: 0,
        }
    }

    pub(super) fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    pub(super) fn next_decode_generation(&mut self) -> u64 {
        self.decode_generation = self.decode_generation.saturating_add(1);
        self.decode_generation
    }

    pub(super) fn touch(&mut self, key: &K) {
        let tick = self.next_tick();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.touch(tick);
        }
    }

    pub(super) fn insert_loading(&mut self, key: K, make_loading: impl FnOnce(u64) -> E) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        let last_used = self.next_tick();
        self.entries.insert(key, make_loading(last_used));
        true
    }

    pub(super) fn start_decode_job(
        &mut self,
        key: K,
        bytes: Arc<[u8]>,
        picker_available: bool,
        make_decoding: impl FnOnce(u64, u64) -> E,
        make_failed: impl FnOnce(u64) -> E,
        make_key: impl FnOnce(K) -> MediaImageDecodeKey,
    ) -> Option<MediaImageDecodeJob> {
        if !self.entries.get(&key).is_some_and(E::is_loading) {
            return None;
        }

        let last_used = self.next_tick();
        if !picker_available {
            self.entries.insert(key, make_failed(last_used));
            return None;
        }

        let generation = self.next_decode_generation();
        self.entries
            .insert(key.clone(), make_decoding(generation, last_used));
        Some(MediaImageDecodeJob {
            key: make_key(key),
            generation,
            bytes,
        })
    }

    pub(super) fn decoded_generation_matches(&self, key: &K, result_generation: u64) -> bool {
        self.entries
            .get(key)
            .and_then(E::decoding_generation)
            .is_some_and(|generation| generation == result_generation)
    }

    pub(super) fn store_failed_if_present(&mut self, key: K, make_failed: impl FnOnce(u64) -> E) {
        if self.entries.contains_key(&key) {
            let last_used = self.next_tick();
            self.entries.insert(key, make_failed(last_used));
        }
    }

    pub(super) fn prune_to_limit(&mut self, limit: usize, is_protected: impl Fn(&K) -> bool) {
        if self.entries.len() <= limit {
            return;
        }

        let mut removable = self
            .entries
            .iter()
            .filter(|(key, _)| !is_protected(key))
            .map(|(key, entry)| (key.clone(), entry.last_used()))
            .collect::<Vec<_>>();
        removable.sort_by_key(|(_, last_used)| *last_used);

        for (key, _) in removable {
            if self.entries.len() <= limit {
                break;
            }
            self.entries.remove(&key);
        }
    }
}
