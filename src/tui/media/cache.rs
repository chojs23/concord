use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    time::Instant,
};

use ratatui_image::protocol::Protocol;

use super::{
    decode::{
        DecodedMediaImage, MAX_RETAINED_ANIMATION_FRAMES, MediaImageDecodeKey,
        MediaImageDecodeRequest,
    },
    work::{MediaWorkError, MediaWorkResult},
};

const MAX_RENDER_PROTOCOLS_PER_MEDIA_ENTRY: usize = MAX_RETAINED_ANIMATION_FRAMES;
const MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS: u8 = 2;

pub(super) struct RenderProtocolCache<K> {
    entries: HashMap<K, Protocol>,
    insertion_order: VecDeque<K>,
    pending: Option<K>,
    failed_attempts: HashMap<K, u8>,
    last_ready: Option<K>,
}

impl<K> RenderProtocolCache<K>
where
    K: Clone + Eq + Hash,
{
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            insertion_order: VecDeque::new(),
            pending: None,
            failed_attempts: HashMap::new(),
            last_ready: None,
        }
    }

    pub(super) fn get(&self, key: &K) -> Option<&Protocol> {
        self.entries.get(key)
    }

    pub(super) fn get_or_last(&self, key: &K) -> Option<&Protocol> {
        self.get(key)
            .or_else(|| self.last_ready.as_ref().and_then(|key| self.get(key)))
    }

    pub(super) fn get_or_last_matching(
        &self,
        key: &K,
        matches: impl Fn(&K) -> bool,
    ) -> Option<&Protocol> {
        self.get(key).or_else(|| {
            self.insertion_order
                .iter()
                .rev()
                .find(|candidate| matches(candidate))
                .and_then(|candidate| self.get(candidate))
        })
    }

    pub(super) fn request_build(&mut self, key: &K) -> bool {
        if self.entries.contains_key(key)
            || self
                .failed_attempts
                .get(key)
                .is_some_and(|attempts| *attempts >= MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS)
            || self.pending.is_some()
        {
            return false;
        }
        self.pending = Some(key.clone());
        true
    }

    pub(super) fn is_terminally_failed(&self, key: &K) -> bool {
        self.failed_attempts
            .get(key)
            .is_some_and(|attempts| *attempts >= MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS)
    }

    /// Returns an error only when retries are exhausted and no prior protocol
    /// can remain on screen as a fallback.
    pub(super) fn store_result(
        &mut self,
        key: K,
        result: MediaWorkResult<Protocol>,
    ) -> Result<(), String> {
        if self.pending.as_ref() != Some(&key) {
            return Ok(());
        }
        self.pending = None;
        match result {
            Ok(protocol) => {
                self.failed_attempts.remove(&key);
                self.insert(key, protocol);
                Ok(())
            }
            Err(MediaWorkError::Busy) => Ok(()),
            Err(MediaWorkError::Failed(error)) => {
                let attempts = self.failed_attempts.entry(key).or_default();
                *attempts = attempts.saturating_add(1);
                if *attempts < MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS {
                    return Ok(());
                }
                if self.entries.is_empty() {
                    Err(error)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn insert(&mut self, key: K, protocol: Protocol) {
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.entries.entry(key.clone())
        {
            entry.insert(protocol);
            self.last_ready = Some(key);
            return;
        }

        while self.entries.len() >= MAX_RENDER_PROTOCOLS_PER_MEDIA_ENTRY {
            let Some(oldest) = self.insertion_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.insertion_order.push_back(key.clone());
        self.last_ready = Some(key.clone());
        self.entries.insert(key, protocol);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(super) trait MediaImageCacheEntry {
    fn last_used(&self) -> u64;
    fn decoded_image(&self) -> Option<&DecodedMediaImage>;
    fn decoded_image_mut(&mut self) -> Option<&mut DecodedMediaImage>;
    fn touch(&mut self, tick: u64);
    fn is_loading(&self) -> bool;
    fn decoding_generation(&self) -> Option<u64>;

    fn retained_decoded_bytes(&self) -> u64 {
        self.decoded_image()
            .map_or(0, DecodedMediaImage::retained_bytes)
    }
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

    pub(super) fn pause_animations(&mut self) {
        for entry in self.entries.values_mut() {
            if let Some(image) = entry.decoded_image_mut() {
                image.pause_animation();
            }
        }
    }

    pub(super) fn next_animation_deadline(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter_map(|entry| entry.decoded_image()?.next_frame_deadline())
            .min()
    }

    pub(super) fn advance_animations(&mut self, now: Instant) -> bool {
        let mut advanced = false;
        for entry in self.entries.values_mut() {
            if let Some(image) = entry.decoded_image_mut() {
                advanced |= image.advance_frame(now);
            }
        }
        advanced
    }

    pub(super) fn insert_loading(&mut self, key: K, make_loading: impl FnOnce(u64) -> E) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        let last_used = self.next_tick();
        self.entries.insert(key, make_loading(last_used));
        true
    }

    pub(super) fn start_decode_request(
        &mut self,
        key: K,
        picker_available: bool,
        make_decoding: impl FnOnce(u64, u64) -> E,
        make_failed: impl FnOnce(u64) -> E,
        make_key: impl FnOnce(K) -> MediaImageDecodeKey,
    ) -> Option<MediaImageDecodeRequest> {
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
        Some(MediaImageDecodeRequest {
            key: make_key(key),
            generation,
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

    pub(super) fn prune_to_limits(
        &mut self,
        entry_limit: usize,
        decoded_byte_budget: u64,
        is_protected: impl Fn(&K) -> bool,
    ) {
        let mut retained_decoded_bytes = self
            .entries
            .values()
            .map(E::retained_decoded_bytes)
            .fold(0u64, u64::saturating_add);
        if self.entries.len() <= entry_limit && retained_decoded_bytes <= decoded_byte_budget {
            return;
        }

        // Visible entries stay available even when they temporarily exceed the
        // budget. Pruning only older off-screen entries avoids image flicker.
        let mut removable = self
            .entries
            .iter()
            .filter(|(key, _)| !is_protected(key))
            .map(|(key, entry)| (key.clone(), entry.last_used()))
            .collect::<Vec<_>>();
        removable.sort_by_key(|(_, last_used)| *last_used);

        for (key, _) in removable {
            if self.entries.len() <= entry_limit && retained_decoded_bytes <= decoded_byte_budget {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                retained_decoded_bytes =
                    retained_decoded_bytes.saturating_sub(entry.retained_decoded_bytes());
            }
        }
    }

    #[cfg(test)]
    pub(super) fn retained_decoded_bytes(&self) -> u64 {
        self.entries
            .values()
            .map(E::retained_decoded_bytes)
            .fold(0u64, u64::saturating_add)
    }
}
