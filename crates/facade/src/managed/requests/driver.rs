//! Bounded fair HTTP sweeps, independent of native response future polling.

use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;
use std::task::{Context, Poll};

use super::RequestRegistry;
use super::entry::Entry;
use crate::managed::{ManagedServiceError, StopPhase};

pub(crate) struct RequestDriver {
    registry: Arc<RequestRegistry>,
    entries: BTreeMap<u64, Entry>,
    cursor: Option<u64>,
    remaining: usize,
}

impl RequestDriver {
    pub(super) fn new(registry: Arc<RequestRegistry>) -> Self {
        Self {
            registry,
            entries: BTreeMap::new(),
            cursor: None,
            remaining: 0,
        }
    }

    pub(crate) fn poll(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), ManagedServiceError>> {
        let mut pending = {
            let mut state = self
                .registry
                .state
                .lock()
                .map_err(|_| ManagedServiceError::Infrastructure)?;
            std::mem::take(&mut state.pending)
        };
        let added = pending.len();
        self.entries.append(&mut pending);
        self.remaining = self.remaining.saturating_add(added);
        if self.remaining == 0 {
            self.remaining = self.entries.len();
        }
        for _ in 0..32 {
            if self.remaining == 0 || self.entries.is_empty() {
                break;
            }
            let id = self
                .cursor
                .and_then(|cursor| {
                    self.entries
                        .range((Excluded(cursor), Unbounded))
                        .next()
                        .map(|(id, _)| *id)
                })
                .or_else(|| self.entries.first_key_value().map(|(id, _)| *id))
                .expect("live HTTP sweep");
            self.cursor = Some(id);
            self.remaining -= 1;
            if self
                .entries
                .get_mut(&id)
                .expect("selected HTTP entry")
                .poll(context)
            {
                drop(self.entries.remove(&id));
            }
        }
        if self.remaining != 0 && !self.entries.is_empty() {
            context.waker().wake_by_ref();
        }
        let stopping = self.registry.control.phase() != StopPhase::Running;
        let idle = self.registry.control.requests_idle();
        if stopping
            && self.entries.is_empty()
            && (idle || self.registry.control.phase() == StopPhase::Cancelled)
        {
            self.registry.close();
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl Drop for RequestDriver {
    fn drop(&mut self) {
        self.entries.clear();
        self.registry.close();
    }
}
