//! Application-owned finite request fixtures; no lifecycle drivers or owners.

mod ready_drop;

use super::support::{Mode, Probe};
use crate::experimental::{OwnedResolverDispatch, OwnedSchema};
use crate::{FieldResult, Value};
use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseParts,
};
use chigiri_runtime::__private::{ArgumentValues, ResolvedObject, ResolvedValue};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(super) struct Dispatch {
    pub(super) schema: OwnedSchema,
    pub(super) coordinates: [(Arc<str>, Arc<str>); 2],
    pub(super) probe: Arc<Probe>,
}

impl OwnedResolverDispatch<()> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }
    fn register_custom_scalars(&self, _: &mut crate::core::executable::CustomScalarRegistry) {}
    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.probe.resolved.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let _drop = ResolverDrop(Arc::clone(&self.probe));
            self.probe.executing.fetch_add(1, Ordering::SeqCst);
            let mode = *self.probe.mode.lock().unwrap();
            if mode == Mode::ResolverPanic {
                panic!("private resolver panic marker");
            }
            if self.probe.pending_execution.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(ResolvedValue::Leaf(Value::String("pong".into())))
        })
    }
}

struct ContextDrop(Arc<Probe>);
impl Drop for ContextDrop {
    fn drop(&mut self) {
        self.0.context_drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct ResolverDrop(Arc<Probe>);
impl Drop for ResolverDrop {
    fn drop(&mut self) {
        self.0.resolver_drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct Factory(pub(super) Arc<Probe>);
impl ContextFactory<(), ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        let drop_guard = ContextDrop(Arc::clone(&self.0));
        self.0.entered.fetch_add(1, Ordering::SeqCst);
        let blocking = self.0.block_context.lock().unwrap().take();
        if let Some(blocking) = blocking {
            blocking.wait();
        }
        let future = Box::pin(async move {
            let _drop = drop_guard;
            let mut release = self.0.release.subscribe();
            while !*release.borrow_and_update() {
                release.changed().await.unwrap();
            }
            Ok(PreparedContext::new((), ())
                .with_response_completion(Completion(Arc::clone(&self.0))))
        });
        if *self.0.mode.lock().unwrap() == Mode::ContextReadyDropPanic {
            Box::pin(ready_drop::ReadyDrop::new(future, Arc::clone(&self.0)))
        } else {
            future
        }
    }
}

pub(super) struct Hook(pub(super) Arc<Probe>);
impl ResponseHook<()> for Hook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.0.hooks.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if *self.0.mode.lock().unwrap() == Mode::HookError {
                return Err(HookError::new(std::io::Error::other(
                    "private hook failure marker",
                )));
            }
            if self.0.pending_hook.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(ResponseHeaders::new())
        })
    }
}

struct Completion(Arc<Probe>);
impl ResponseCompletion for Completion {
    fn complete(&self, parts: ResponseParts, outcome: ResponseCompletionOutcome) -> ResponseParts {
        self.0.completions.lock().unwrap().push(outcome);
        self.0.completion_saw_disposed_resolver.store(
            self.0.resolver_drops.load(Ordering::SeqCst) == 1,
            Ordering::SeqCst,
        );
        let block = self.0.block_completion.lock().unwrap().take();
        if let Some(block) = block {
            block.wait();
        }
        if *self.0.mode.lock().unwrap() == Mode::CompletionPanic {
            panic!("private completion panic marker");
        }
        if *self.0.mode.lock().unwrap() == Mode::CompletionPayloadDropPanic {
            std::panic::panic_any(PoisonPayload);
        }
        parts
    }
}

struct PoisonPayload;
impl Drop for PoisonPayload {
    fn drop(&mut self) {
        panic!("private panic payload disposal marker");
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.0.completion_drops.fetch_add(1, Ordering::SeqCst);
        if *self.0.mode.lock().unwrap() == Mode::CompletionDropPanic {
            panic!("private completion destructor marker");
        }
    }
}

pub(super) struct Observer(pub(super) Arc<Probe>);
impl crate::ErrorObserver for Observer {
    fn observe_error(&self, event: crate::ErrorEvent<'_>) {
        self.0.errors.lock().unwrap().push((
            event.component().to_owned(),
            event
                .source()
                .source()
                .map_or_else(|| event.source().to_string(), ToString::to_string),
        ));
        if self.0.observer_panics.load(Ordering::SeqCst) {
            panic!("private observer panic marker");
        }
    }
}

pub(super) struct SourceResolver(pub(super) Arc<Probe>);
impl chigiri_http::OperationSourceResolver for SourceResolver {
    fn resolve<'a>(
        &'a self,
        envelope: &'a chigiri_http::RequestEnvelope,
    ) -> BoxFuture<
        'a,
        Result<chigiri_http::ResolvedOperationSource, chigiri_http::SourceResolutionError>,
    > {
        Box::pin(async move {
            if self.0.pending_source.load(Ordering::SeqCst) {
                let _guard = SourceDrop(Arc::clone(&self.0));
                self.0.source_entered.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
            let stop = self.0.stop_at_source_return.lock().unwrap().take();
            if let Some(control) = stop {
                control.begin(
                    crate::ShutdownOptions::graceful_for(std::time::Duration::from_millis(50))
                        .unwrap(),
                    tokio::time::Instant::now(),
                );
            }
            Ok(chigiri_http::ResolvedOperationSource::new(
                envelope.document_text().unwrap().to_owned(),
            ))
        })
    }
}

struct SourceDrop(Arc<Probe>);
impl Drop for SourceDrop {
    fn drop(&mut self) {
        self.0.source_drops.fetch_add(1, Ordering::SeqCst);
    }
}
