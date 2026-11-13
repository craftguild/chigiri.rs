//! Bounded store that touches only one key on the hot path.

use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

use crate::MonotonicTick;
use chigiri_rate::QuotaKey;

pub(super) struct BoundedStore<S> {
    entries: BTreeMap<QuotaKey, S>,
    max_keys: usize,
    idle_ttl_nanos: u64,
    cursor: Option<QuotaKey>,
}

impl<S> BoundedStore<S> {
    pub(super) fn new(max_keys: usize, idle_ttl_nanos: u64) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_keys,
            idle_ttl_nanos,
            cursor: None,
        }
    }

    pub(super) fn maintain_one(
        &mut self,
        now: MonotonicTick,
        last_seen: impl FnOnce(&S) -> MonotonicTick + Copy,
    ) {
        let candidate = self
            .cursor
            .as_ref()
            .and_then(|cursor| {
                self.entries
                    .range((Excluded(cursor), Unbounded))
                    .next()
                    .map(|(key, _)| key.clone())
            })
            .or_else(|| self.entries.keys().next().cloned());
        let Some(candidate) = candidate else {
            self.cursor = None;
            return;
        };
        let expired = self
            .entries
            .get(&candidate)
            .is_some_and(|state| now.saturating_sub(last_seen(state)) >= self.idle_ttl_nanos);
        if expired {
            self.entries.remove(&candidate);
        }
        self.cursor = Some(candidate);
    }

    pub(super) fn get_mut(&mut self, key: &QuotaKey) -> Option<&mut S> {
        self.entries.get_mut(key)
    }

    pub(super) fn insert(&mut self, key: QuotaKey, state: S) -> Result<&mut S, ()> {
        if self.entries.len() >= self.max_keys {
            return Err(());
        }
        self.entries.insert(key.clone(), state);
        Ok(self
            .entries
            .get_mut(&key)
            .expect("key must exist immediately after insertion"))
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(super) trait LastSeen {
    fn last_seen(&self) -> MonotonicTick;
}

pub(super) fn observed_now(now: MonotonicTick, last_seen: MonotonicTick) -> MonotonicTick {
    now.max(last_seen)
}
