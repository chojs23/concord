use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) struct LastSelection<K> {
    selected: Option<K>,
}

impl<K> Default for LastSelection<K> {
    fn default() -> Self {
        Self { selected: None }
    }
}

impl<K> LastSelection<K>
where
    K: Copy + Eq,
{
    pub(super) fn clear(&mut self) {
        self.selected = None;
    }

    pub(super) fn select(&mut self, key: K) -> bool {
        let changed = self.selected != Some(key);
        self.selected = Some(key);
        changed
    }
}

/// One on-demand request per key. A key is requested when first selected,
/// deduped while in flight or loaded, and a failed key retries when the
/// selection moves away and comes back.
#[derive(Debug)]
pub(super) struct OnDemandRequests<K> {
    requests: HashMap<K, OnDemandRequestState>,
    last_selection: LastSelection<K>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OnDemandRequestState {
    Requested,
    Loaded,
    Failed,
}

impl<K> Default for OnDemandRequests<K> {
    fn default() -> Self {
        Self {
            requests: HashMap::new(),
            last_selection: LastSelection::default(),
        }
    }
}

impl<K> OnDemandRequests<K>
where
    K: Copy + Eq + Hash,
{
    /// Returns the key when a request should be sent now. `force_reload`
    /// additionally re-requests an already loaded key on reselect.
    pub(super) fn next(&mut self, key: Option<K>, force_reload: bool) -> Option<K> {
        let Some(key) = key else {
            self.last_selection.clear();
            return None;
        };
        let selection_changed = self.last_selection.select(key);

        match self.requests.get(&key).copied() {
            None => {
                self.requests.insert(key, OnDemandRequestState::Requested);
                Some(key)
            }
            Some(OnDemandRequestState::Failed) if selection_changed => {
                self.requests.insert(key, OnDemandRequestState::Requested);
                Some(key)
            }
            Some(OnDemandRequestState::Loaded) if force_reload && selection_changed => {
                self.requests.insert(key, OnDemandRequestState::Requested);
                Some(key)
            }
            Some(
                OnDemandRequestState::Requested
                | OnDemandRequestState::Loaded
                | OnDemandRequestState::Failed,
            ) => None,
        }
    }

    pub(super) fn mark_loaded(&mut self, key: K) {
        self.requests.insert(key, OnDemandRequestState::Loaded);
    }

    pub(super) fn mark_failed(&mut self, key: K) {
        self.requests.insert(key, OnDemandRequestState::Failed);
    }

    /// Forgets the key so the next selection requests it again.
    pub(super) fn reset(&mut self, key: &K) {
        self.requests.remove(key);
    }
}

/// Deduplicates cursor-paged requests per key. One in-flight cursor at a
/// time, and an exhausted cursor is never re-requested.
#[derive(Debug)]
pub(super) struct CursorRequests<K, C> {
    requests: HashMap<K, CursorRequestState<C>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CursorRequestState<C> {
    Requested { cursor: C, exhausts_on_empty: bool },
    Exhausted { cursor: C },
}

impl<K, C> Default for CursorRequests<K, C> {
    fn default() -> Self {
        Self {
            requests: HashMap::new(),
        }
    }
}

impl<K, C> CursorRequests<K, C>
where
    K: Copy + Eq + Hash,
    C: Copy + Eq,
{
    pub(super) fn begin_request(&mut self, key: K, cursor: C, exhausts_on_empty: bool) -> bool {
        match self.requests.get(&key) {
            Some(CursorRequestState::Requested { .. }) => false,
            Some(CursorRequestState::Exhausted { cursor: exhausted }) if *exhausted == cursor => {
                false
            }
            _ => {
                self.requests.insert(
                    key,
                    CursorRequestState::Requested {
                        cursor,
                        exhausts_on_empty,
                    },
                );
                true
            }
        }
    }

    /// Ignores responses whose cursor does not match the in-flight request.
    pub(super) fn record_loaded(&mut self, key: K, response_cursor: C, is_empty: bool) {
        let Some(CursorRequestState::Requested {
            cursor,
            exhausts_on_empty,
        }) = self.requests.get(&key).copied()
        else {
            return;
        };
        if response_cursor != cursor {
            return;
        }
        if is_empty && exhausts_on_empty {
            self.requests
                .insert(key, CursorRequestState::Exhausted { cursor });
        } else {
            self.requests.remove(&key);
        }
    }

    pub(super) fn record_failed(&mut self, key: K, response_cursor: C) {
        let Some(CursorRequestState::Requested { cursor, .. }) = self.requests.get(&key).copied()
        else {
            return;
        };
        if response_cursor == cursor {
            self.requests.remove(&key);
        }
    }
}

#[derive(Debug)]
pub(super) struct TimedRequestSet<K> {
    requested: HashMap<K, Instant>,
    requested_order: VecDeque<K>,
    ttl: Duration,
    max_requested: usize,
}

impl<K> TimedRequestSet<K>
where
    K: Clone + Eq + Hash,
{
    pub(super) fn new(ttl: Duration, max_requested: usize) -> Self {
        Self {
            requested: HashMap::new(),
            requested_order: VecDeque::new(),
            ttl,
            max_requested,
        }
    }

    pub(super) fn insert(&mut self, key: K, now: Instant) -> bool {
        if self.requested.contains_key(&key) {
            return false;
        }
        self.requested.insert(key.clone(), now);
        self.requested_order.push_back(key);
        self.prune(now);
        true
    }

    pub(super) fn contains(&self, key: &K) -> bool {
        self.requested.contains_key(key)
    }

    pub(super) fn remove(&mut self, key: &K) {
        self.requested.remove(key);
        self.requested_order
            .retain(|requested_key| requested_key != key);
    }

    pub(super) fn prune(&mut self, now: Instant) {
        self.requested.retain(|_, requested_at| {
            now.checked_duration_since(*requested_at)
                .is_none_or(|age| age <= self.ttl)
        });
        self.requested_order
            .retain(|key| self.requested.contains_key(key));
        while self.requested.len() > self.max_requested {
            let Some(oldest) = self.requested_order.pop_front() else {
                break;
            };
            self.requested.remove(&oldest);
        }
    }
}
