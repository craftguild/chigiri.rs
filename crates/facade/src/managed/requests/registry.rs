//! Native-admitted HTTP tasks use their own exact completion registration.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chigiri_http::{RequestBody, ResponseMediaType};
use tokio::runtime::{Handle, Id};
use tokio::sync::oneshot;
use tokio::time::Instant;

use super::driver::RequestDriver;
use super::entry::{Entry, RetainedHttpRequest};
use super::task::HttpTask;
use super::{Disconnect, RequestResponse};
use crate::BoxFuture;
use crate::managed::control::RequestPermit;
use crate::managed::http::HttpWork;
use crate::managed::{Control, ManagedHttpResponse, ManagedServiceError};
use crate::subscription::worker::{EndpointWorkerOwner, register_endpoint, spawn_worker};
use crate::transport::deadline::HttpDeadlineState;

pub(crate) struct RequestRegistry {
    pub(super) control: Arc<Control>,
    pub(super) state: Mutex<State>,
    owner: Mutex<Option<EndpointWorkerOwner>>,
    runtime: Id,
}

pub(super) struct State {
    pub(super) open: bool,
    next_id: u64,
    pub(super) pending: BTreeMap<u64, Entry>,
}

impl RequestRegistry {
    pub(crate) fn new(
        control: Arc<Control>,
    ) -> Result<(Arc<Self>, RequestDriver), ManagedServiceError> {
        let runtime = Handle::try_current().map_err(|_| ManagedServiceError::MissingRuntime)?;
        let owner = register_endpoint().map_err(|_| ManagedServiceError::Startup)?;
        let registry = Arc::new(Self {
            control,
            state: Mutex::new(State {
                open: true,
                next_id: 0,
                pending: BTreeMap::new(),
            }),
            owner: Mutex::new(Some(owner)),
            runtime: runtime.id(),
        });
        Ok((Arc::clone(&registry), RequestDriver::new(registry)))
    }

    pub(crate) fn start<C: Send + Sync + 'static, B: RequestBody + 'static>(
        &self,
        work: HttpWork<C, B>,
        permit: RequestPermit,
        started: Instant,
        timeout: Duration,
        media: ResponseMediaType,
    ) -> BoxFuture<'static, ManagedHttpResponse> {
        match self.register(work, permit, started, timeout, media) {
            Ok(response) => Box::pin(response),
            Err(ManagedServiceError::Closed) => {
                let response = super::response::cancelled(&self.control);
                Box::pin(async move { response })
            }
            Err(_) => {
                self.control.fail();
                Box::pin(async { ManagedHttpResponse::internal_failure() })
            }
        }
    }

    fn register<C: Send + Sync + 'static, B: RequestBody + 'static>(
        &self,
        work: HttpWork<C, B>,
        permit: RequestPermit,
        started: Instant,
        timeout: Duration,
        media: ResponseMediaType,
    ) -> Result<RequestResponse, ManagedServiceError> {
        let mut failure = RegistrationFailure {
            control: &self.control,
            armed: true,
        };
        let runtime = Handle::try_current().map_err(|_| ManagedServiceError::MissingRuntime)?;
        if runtime.id() != self.runtime {
            return Err(ManagedServiceError::RuntimeMismatch);
        }
        let deadline = started
            .checked_add(timeout)
            .ok_or(ManagedServiceError::Infrastructure)?;
        let slot = self
            .owner
            .lock()
            .map_err(|_| ManagedServiceError::Infrastructure)?
            .as_ref()
            .ok_or(ManagedServiceError::Closed)?
            .reserve_generation()
            .map_err(|_| ManagedServiceError::Capacity)?;
        let disconnect = Disconnect::new(Arc::clone(&self.control));
        let finished = Arc::new(OnceLock::new());
        let (sender, receiver) = oneshot::channel();
        let (start, start_receiver) = oneshot::channel();
        let (deadlines, monitor) = HttpDeadlineState::new(
            work.deadline_factory(),
            deadline,
            Arc::clone(&self.control),
            work.instrumentation_capture(),
        )?;
        let task = HttpTask::new(work, start_receiver, Arc::clone(&finished), monitor);
        let worker = spawn_worker(&slot, task).map_err(|_| ManagedServiceError::Startup)?;
        // All proposed owners precede the locks. Even failed publication keeps
        // the original concrete join and request permit in registered recovery.
        let mut proposed = Some(Entry {
            worker: Some(worker),
            slot: Some(slot),
            retained: Some(RetainedHttpRequest {
                _permit: permit,
                deadlines,
            }),
            response: Some(sender),
            disconnect: Arc::clone(&disconnect),
            control: Arc::clone(&self.control),
            finished,
            deadline,
            media,
        });
        let published = (|| {
            let gate = self.control.emission_gate()?;
            let mut state = self
                .state
                .lock()
                .map_err(|_| ManagedServiceError::Infrastructure)?;
            if !state.open {
                return Err(ManagedServiceError::Closed);
            }
            let id = state
                .next_id
                .checked_add(1)
                .ok_or(ManagedServiceError::Capacity)?;
            state.next_id = id;
            state
                .pending
                .insert(id, proposed.take().expect("proposed HTTP owner"));
            drop(state);
            drop(gate);
            Ok::<(), ManagedServiceError>(())
        })();
        if let Err(error) = published {
            self.control.fail();
            return Err(error);
        }
        self.control.notify();
        start.send(()).map_err(|_| ManagedServiceError::Startup)?;
        failure.armed = false;
        Ok(RequestResponse::new(
            receiver,
            disconnect,
            Arc::clone(&self.control),
        ))
    }

    pub(super) fn close(&self) {
        let pending = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.open = false;
            std::mem::take(&mut state.pending)
        };
        drop(pending);
        let owner = self
            .owner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        drop(owner);
    }
}

struct RegistrationFailure<'a> {
    control: &'a Control,
    armed: bool,
}

impl Drop for RegistrationFailure<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.control.fail();
        }
    }
}
