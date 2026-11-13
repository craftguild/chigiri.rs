//! Reconnect bounded native identities to the original application HTTP scope.

use std::sync::{Arc, Mutex, Weak};

use chigiri::TraceId;

use super::{HttpScope, Trace};
use crate::luau::ScriptError;

const PREFIX: &str = "mock-http-";

#[cfg(test)]
mod tests;

pub(crate) struct HttpScopes {
    slots: Mutex<Vec<Slot>>,
}

struct Slot {
    id: u64,
    trace: Weak<Trace>,
}

impl HttpScopes {
    pub(crate) fn new() -> Result<Arc<Self>, ScriptError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(crate::limits::HTTP_REQUESTS)
            .map_err(|_| {
                ScriptError::new("environment", "cannot allocate HTTP scope references")
            })?;
        slots.resize_with(crate::limits::HTTP_REQUESTS, || Slot {
            id: 0,
            trace: Weak::new(),
        });
        Ok(Arc::new(Self {
            slots: Mutex::new(slots),
        }))
    }

    pub(crate) fn bind(&self, scope: &HttpScope) -> Result<TraceId, ScriptError> {
        if scope.0.admission.get().is_none() {
            return Err(ScriptError::new(
                "environment",
                "HTTP scope has no admission owner",
            ));
        }
        let id = scope.request_id();
        let identity = TraceId::new(format!("{PREFIX}{id}"))
            .map_err(|_| ScriptError::new("environment", "invalid HTTP scope identity"))?;
        let mut slots = self.slots.lock().map_err(|_| {
            ScriptError::new("environment", "HTTP scope references are unavailable")
        })?;
        let slot = slots
            .iter_mut()
            .find(|slot| slot.trace.strong_count() == 0)
            .ok_or_else(|| ScriptError::new("environment", "HTTP scope references are occupied"))?;
        slot.id = id;
        slot.trace = Arc::downgrade(&scope.0);
        Ok(identity)
    }

    pub(in crate::service) fn scope(&self, identity: Option<&TraceId>) -> Option<HttpScope> {
        self.trace(identity).map(HttpScope)
    }

    pub(super) fn trace(&self, identity: Option<&TraceId>) -> Option<Arc<Trace>> {
        let id = identity?
            .as_str()
            .strip_prefix(PREFIX)?
            .parse::<u64>()
            .ok()?;
        let trace = {
            let slots = self.slots.lock().ok()?;
            slots.iter().find(|slot| slot.id == id)?.trace.clone()
        };
        // Never upgrade/drop an application graph while the index is locked.
        trace.upgrade()
    }
}
