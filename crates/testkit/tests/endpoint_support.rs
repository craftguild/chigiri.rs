//! Body, hook, and deadline probes for transport-neutral endpoint conformance.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{
    AdmissionDecision, AdmissionError, BodyReadError, BoxFuture, DeadlineFactory, HookError,
    RequestBody, ResponseHeaders, ResponseHook, ResponseHookContext, TransportAdmission,
    TransportRequest,
};

pub(crate) struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

pub(crate) struct CountingBody(pub(crate) Arc<AtomicUsize>);

impl RequestBody for CountingBody {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending())
    }
}

pub(crate) struct PendingBody(pub(crate) Arc<AtomicBool>);

impl RequestBody for PendingBody {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        Box::pin(Pending::new(Arc::clone(&self.0)))
    }
}

pub(crate) struct ChunkBody {
    chunks: std::vec::IntoIter<Vec<u8>>,
    polls: Arc<AtomicUsize>,
}

impl ChunkBody {
    pub(crate) fn new(chunks: Vec<Vec<u8>>, polls: Arc<AtomicUsize>) -> Self {
        Self {
            chunks: chunks.into_iter(),
            polls,
        }
    }
}

impl RequestBody for ChunkBody {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        let chunk = self.chunks.next();
        Box::pin(async move { Ok(chunk) })
    }
}

pub(crate) struct PendingHook(pub(crate) Arc<AtomicBool>);

impl ResponseHook<()> for PendingHook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        Box::pin(Pending::new(Arc::clone(&self.0)))
    }
}

pub(crate) struct PendingTransport(pub(crate) Arc<AtomicBool>);

impl TransportAdmission for PendingTransport {
    fn admit<'a>(
        &'a self,
        _: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(Pending::new(Arc::clone(&self.0)))
    }
}

pub(crate) struct Pending<T> {
    dropped: Arc<AtomicBool>,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Pending<T> {
    pub(crate) fn new(dropped: Arc<AtomicBool>) -> Self {
        Self {
            dropped,
            marker: std::marker::PhantomData,
        }
    }
}

impl<T> Future for Pending<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Pending
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
