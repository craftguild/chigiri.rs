use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chigiri_http::{
    AdmissionDecision, BoxFuture, DecodedRequestHook, DecodedRequestView, OperationSourceResolver,
    Rejection, RequestEnvelope, RequestMethod, ResolvedOperationSource, ResponseMediaType,
    SourceResolutionError,
};
use chigiri_runtime::GraphQlError;

#[derive(Default)]
pub(super) struct IngressProbe {
    pub(super) source_calls: AtomicUsize,
    pub(super) decoded_calls: AtomicUsize,
    pub(super) decoded_policy: Mutex<Vec<(RequestMethod, ResponseMediaType)>>,
}

pub(super) enum SourceMode {
    Document(Arc<str>),
    Reject,
}

pub(super) struct Source {
    pub(super) probe: Arc<IngressProbe>,
    pub(super) mode: SourceMode,
    pub(super) gate: Option<Arc<BlockingGate>>,
}

impl OperationSourceResolver for Source {
    fn resolve<'a>(
        &'a self,
        _: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        self.probe.source_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.enter_and_wait();
        }
        match &self.mode {
            SourceMode::Document(document) => {
                let document = document.to_string();
                Box::pin(async move { Ok(ResolvedOperationSource::new(document)) })
            }
            SourceMode::Reject => Box::pin(async {
                Err(SourceResolutionError::Reject(Box::new(
                    Rejection::new(
                        403,
                        GraphQlError::new("source denied").code("SOURCE_DENIED"),
                    )
                    .unwrap(),
                )))
            }),
        }
    }
}

pub(super) struct Decoded {
    pub(super) probe: Arc<IngressProbe>,
    pub(super) gate: Option<Arc<BlockingGate>>,
}

impl DecodedRequestHook for Decoded {
    fn inspect<'a>(
        &'a self,
        request: DecodedRequestView<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, chigiri_http::AdmissionError>> {
        self.probe.decoded_calls.fetch_add(1, Ordering::SeqCst);
        self.probe
            .decoded_policy
            .lock()
            .unwrap()
            .push((request.method(), request.response_media_type()));
        if let Some(gate) = &self.gate {
            gate.enter_and_wait();
        }
        Box::pin(async { Ok(AdmissionDecision::Continue) })
    }
}

#[derive(Default)]
pub(super) struct BlockingGate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl BlockingGate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }

    pub(super) fn wait_entered(&self) {
        let mut state = self.state.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.0 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "application callback did not start");
            let result = self.changed.wait_timeout(state, remaining).unwrap();
            state = result.0;
        }
    }

    pub(super) fn release_on_drop(&self) -> ReleaseOnDrop<'_> {
        ReleaseOnDrop(self)
    }

    pub(super) fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = true;
        self.changed.notify_all();
    }
}

// Release before the owned runtime is dropped even if a fixture assertion unwinds.
pub(super) struct ReleaseOnDrop<'a>(&'a BlockingGate);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
