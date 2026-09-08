use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    time::{Duration, Instant},
};

use ratatui_image::protocol::Protocol;

use super::{
    decode::{
        DecodedMediaImage, MAX_RETAINED_ANIMATION_FRAMES, MediaImageDecodeKey,
        MediaImageDecodeRequest,
    },
    work::{MediaProtocolRequest, MediaWorkError, MediaWorkResult},
};

const MAX_RENDER_PROTOCOLS_PER_MEDIA_ENTRY: usize = MAX_RETAINED_ANIMATION_FRAMES;
const MIN_RENDER_PROTOCOLS_PER_MEDIA_ENTRY: usize = 2;
const MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS: u8 = 2;
/// A terminal graphics protocol holds the whole frame as an encoded payload:
/// one kitty protocol for a 60x30 preview measures ~1.3MB. Keeping an
/// animation's worth of them per entry reached ~650MB across a full preview
/// cache, none of which the decoded-image budget could see. Only the frames
/// around the one on screen are worth keeping, so bound them by size as well as
/// by count; the count alone cannot tell a 2-cell emoji from a 30-row preview.
pub(super) const RENDER_PROTOCOL_BYTE_BUDGET_PER_MEDIA_ENTRY: u64 = 6 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct MediaCacheStats {
    pub(in crate::tui) entries: usize,
    pub(in crate::tui) entry_limit: usize,
    pub(in crate::tui) loading: usize,
    pub(in crate::tui) decoding: usize,
    pub(in crate::tui) ready: usize,
    pub(in crate::tui) failed: usize,
    /// These decoded pixels share frame storage with the shared decode cache.
    /// Do not add both values when estimating process memory.
    pub(in crate::tui) decoded_bytes: u64,
    pub(in crate::tui) decoded_byte_budget: u64,
    pub(in crate::tui) render_protocol_bytes: u64,
    pub(in crate::tui) retryable: usize,
}

pub(super) struct RenderProtocolCache<K> {
    entries: HashMap<K, Protocol>,
    insertion_order: VecDeque<K>,
    pending: Option<K>,
    failed_attempts: HashMap<K, u8>,
    last_ready: Option<K>,
    entry_bytes: HashMap<K, u64>,
    retained_bytes: u64,
    protected: Option<HashSet<K>>,
    request: Option<MediaProtocolRequest>,
    retiring: Option<MediaProtocolRequest>,
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
            entry_bytes: HashMap::new(),
            retained_bytes: 0,
            protected: None,
            request: None,
            retiring: None,
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

    pub(super) fn protect(&mut self, keys: impl IntoIterator<Item = K>) {
        let protected = keys.into_iter().collect::<HashSet<_>>();
        debug_assert!(protected.len() <= MAX_RENDER_PROTOCOLS_PER_MEDIA_ENTRY);
        if self
            .retiring
            .as_ref()
            .is_some_and(MediaProtocolRequest::is_finished)
        {
            self.retiring = None;
        }
        if (self
            .pending
            .as_ref()
            .is_some_and(|key| !protected.contains(key))
            || self
                .request
                .as_ref()
                .is_some_and(MediaProtocolRequest::is_cancelled))
            && let Some(request) = &self.request
        {
            request.cancel();
            if request.is_finished() {
                self.request = None;
                self.pending = None;
            } else if self.retiring.is_none() {
                self.retiring = self.request.take();
                self.pending = None;
            }
            // Two encoders may already be running. Keep the cancelled current
            // slot until one finishes, rather than growing a queue.
        }
        self.protected = Some(protected);
        self.trim_to_limits();
    }

    pub(super) fn request_protected_build(&mut self, key: &K) -> Option<MediaProtocolRequest> {
        if !self
            .protected
            .as_ref()
            .is_some_and(|keys| keys.contains(key))
            || !self.request_build(key)
        {
            return None;
        }
        let request = MediaProtocolRequest::new();
        self.request = Some(request.clone());
        Some(request)
    }

    pub(super) fn store_requested_result(
        &mut self,
        key: K,
        request: &MediaProtocolRequest,
        result: MediaWorkResult<Protocol>,
        bytes: u64,
    ) -> bool {
        if !self
            .request
            .as_ref()
            .is_some_and(|current| current.same_request(request))
            || self.pending.as_ref() != Some(&key)
            || request.is_cancelled()
            || !self
                .protected
                .as_ref()
                .is_some_and(|keys| keys.contains(&key))
        {
            return false;
        }
        self.request = None;
        self.store_result(key, result, bytes);
        true
    }

    pub(super) fn is_terminally_failed(&self, key: &K) -> bool {
        self.failed_attempts
            .get(key)
            .is_some_and(|attempts| *attempts >= MAX_RENDER_PROTOCOL_BUILD_ATTEMPTS)
    }

    pub(super) fn retain_failures(&mut self, mut retain: impl FnMut(&K) -> bool) {
        self.failed_attempts.retain(|key, _| retain(key));
    }

    pub(super) fn forget_failures(&mut self) {
        self.failed_attempts.clear();
    }

    /// Record only the matching job. Layout-specific fallback belongs to the caller.
    pub(super) fn store_result(&mut self, key: K, result: MediaWorkResult<Protocol>, bytes: u64) {
        if self.pending.as_ref() != Some(&key) {
            return;
        }
        self.pending = None;
        match result {
            Ok(protocol) => {
                self.failed_attempts.remove(&key);
                self.insert(key, protocol, bytes);
            }
            Err(MediaWorkError::Busy) => {}
            Err(MediaWorkError::Failed(_)) => {
                let attempts = self.failed_attempts.entry(key).or_default();
                *attempts = attempts.saturating_add(1);
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn insert(&mut self, key: K, protocol: Protocol, bytes: u64) {
        if !self.entries.contains_key(&key) {
            self.insertion_order.push_back(key.clone());
        }
        self.last_ready = Some(key.clone());
        self.entries.insert(key.clone(), protocol);
        self.set_entry_bytes(key, bytes);
        self.trim_to_limits();
    }

    fn trim_to_limits(&mut self) {
        // Previews protect every visible crop in the current/next frame window.
        // That finite set may exceed the byte budget, but scroll history may not.
        // Avatar and emoji caches retain their existing two-frame policy.
        let minimum = if self.protected.is_some() {
            0
        } else {
            MIN_RENDER_PROTOCOLS_PER_MEDIA_ENTRY
        };
        while self.entries.len() > MAX_RENDER_PROTOCOLS_PER_MEDIA_ENTRY
            || (self.entries.len() > minimum
                && self.retained_bytes > RENDER_PROTOCOL_BYTE_BUDGET_PER_MEDIA_ENTRY)
        {
            let Some(index) = self.insertion_order.iter().position(|key| {
                !self
                    .protected
                    .as_ref()
                    .is_some_and(|keys| keys.contains(key))
            }) else {
                break;
            };
            let oldest = self
                .insertion_order
                .remove(index)
                .expect("eviction index exists");
            self.entries.remove(&oldest);
            self.forget_entry_bytes(&oldest);
            self.failed_attempts.remove(&oldest);
            if self.last_ready.as_ref() == Some(&oldest) {
                self.last_ready = self.insertion_order.back().cloned();
            }
        }
    }

    fn set_entry_bytes(&mut self, key: K, bytes: u64) {
        if let Some(previous) = self.entry_bytes.insert(key, bytes) {
            self.retained_bytes = self.retained_bytes.saturating_sub(previous);
        }
        self.retained_bytes = self.retained_bytes.saturating_add(bytes);
    }

    fn forget_entry_bytes(&mut self, key: &K) {
        if let Some(bytes) = self.entry_bytes.remove(key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(bytes);
        }
    }

    pub(super) fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<K> Drop for RenderProtocolCache<K> {
    fn drop(&mut self) {
        for request in self.request.iter().chain(self.retiring.iter()) {
            request.cancel();
        }
    }
}

pub(super) trait MediaImageCacheEntry {
    fn last_used(&self) -> u64;
    fn decoded_image(&self) -> Option<&DecodedMediaImage>;
    fn decoded_image_mut(&mut self) -> Option<&mut DecodedMediaImage>;
    fn touch(&mut self, tick: u64);
    fn is_loading(&self) -> bool;
    fn is_failed(&self) -> bool;
    fn decoding_generation(&self) -> Option<u64>;

    fn retained_decoded_bytes(&self) -> u64 {
        self.decoded_image()
            .map_or(0, DecodedMediaImage::retained_bytes)
    }

    /// Bytes held by this entry's built protocols, which the decoded budget
    /// cannot see. Zero for entries that do not cache protocols.
    fn retained_protocol_bytes(&self) -> u64 {
        0
    }
}

pub(super) struct MediaImageCacheCore<K, E> {
    pub(super) entries: HashMap<K, E>,
    pub(super) tick: u64,
    pub(super) decode_generation: u64,
    failed: HashMap<K, FailedMediaFetch>,
}

struct FailedMediaFetch {
    attempts: u32,
    /// `None` once the retries are spent: only a manual refresh revives it.
    retry_at: Option<Instant>,
}

/// A failed entry stays in the cache, and a present entry suppresses its own
/// request, so without these a download that failed once stayed broken for the
/// rest of the session. Back off between tries so a CDN that is refusing
/// connections is not hammered, then stop and leave recovery to a refresh.
const MEDIA_FETCH_RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(3),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

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
            failed: HashMap::new(),
        }
    }

    /// Reports whether a failed entry is due for another try, consuming that
    /// try. The failure record outlives the entry it replaces, so the attempt
    /// count keeps climbing across retries and the backoff terminates.
    pub(super) fn take_due_retry(&mut self, key: &K, now: Instant) -> bool {
        if !self.entries.get(key).is_some_and(E::is_failed) {
            return false;
        }
        let Some(failure) = self.failed.get_mut(key) else {
            return false;
        };
        if failure.retry_at.is_some_and(|retry_at| retry_at <= now) {
            failure.retry_at = None;
            return true;
        }
        false
    }

    pub(super) fn retry_deadline(&self, key: &K) -> Option<Instant> {
        if !self.entries.get(key).is_some_and(E::is_failed) {
            return None;
        }
        self.failed.get(key)?.retry_at
    }

    /// Forgets every failure so the next request pass retries all of them,
    /// however many times they already failed.
    pub(super) fn forget_failures(&mut self) {
        self.failed.clear();
        self.entries.retain(|_, entry| !entry.is_failed());
    }

    fn note_failure(&mut self, key: K, now: Instant) {
        let attempts = self
            .failed
            .get(&key)
            .map_or(0, |failure| failure.attempts)
            .saturating_add(1);
        let retry_at = MEDIA_FETCH_RETRY_BACKOFF
            .get(attempts as usize - 1)
            .map(|backoff| now + *backoff);
        self.failed
            .insert(key, FailedMediaFetch { attempts, retry_at });
    }

    /// Entry count, decoded bytes, and protocol bytes held right now. Decoded
    /// bytes are shared through an `Arc` with the shared decode cache, so they
    /// overlap with its total rather than adding to it.
    pub(super) fn retained_stats(&self) -> (usize, u64, u64) {
        self.entries.values().fold(
            (self.entries.len(), 0, 0),
            |(count, decoded, protocols), entry| {
                (
                    count,
                    decoded.saturating_add(entry.retained_decoded_bytes()),
                    protocols.saturating_add(entry.retained_protocol_bytes()),
                )
            },
        )
    }

    pub(super) fn diagnostics(
        &self,
        entry_limit: usize,
        decoded_byte_budget: u64,
    ) -> MediaCacheStats {
        let mut stats = MediaCacheStats {
            entries: self.entries.len(),
            entry_limit,
            decoded_byte_budget,
            ..MediaCacheStats::default()
        };
        for (key, entry) in &self.entries {
            if entry.is_loading() {
                stats.loading += 1;
            } else if entry.decoding_generation().is_some() {
                stats.decoding += 1;
            } else if entry.decoded_image().is_some() {
                stats.ready += 1;
            } else if entry.is_failed() {
                stats.failed += 1;
                if self
                    .failed
                    .get(key)
                    .and_then(|failure| failure.retry_at)
                    .is_some()
                {
                    stats.retryable += 1;
                }
            }
            stats.decoded_bytes = stats
                .decoded_bytes
                .saturating_add(entry.retained_decoded_bytes());
            stats.render_protocol_bytes = stats
                .render_protocol_bytes
                .saturating_add(entry.retained_protocol_bytes());
        }
        stats
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
        let now = Instant::now();
        if !self.can_insert_loading(&key, now) {
            return false;
        }
        self.take_due_retry(&key, now);
        let last_used = self.next_tick();
        self.entries.insert(key, make_loading(last_used));
        true
    }

    /// Check eligibility before copying a local upload, without spending its retry.
    pub(super) fn can_insert_loading(&self, key: &K, now: Instant) -> bool {
        !self.entries.contains_key(key)
            || self
                .retry_deadline(key)
                .is_some_and(|deadline| deadline <= now)
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
            self.entries.insert(key.clone(), make_failed(last_used));
            self.note_failure(key, Instant::now());
        }
    }

    /// For callers that build the failed entry themselves.
    pub(super) fn note_failed_entry(&mut self, key: K) {
        self.note_failure(key, Instant::now());
    }

    pub(super) fn prune_to_limits(
        &mut self,
        entry_limit: usize,
        decoded_byte_budget: u64,
        is_protected: impl Fn(&K) -> bool,
    ) {
        // Failure records are only meaningful while their entry is around, so
        // they are evicted with it rather than growing for the whole session.
        let entries = &self.entries;
        self.failed.retain(|key, _| entries.contains_key(key));

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
