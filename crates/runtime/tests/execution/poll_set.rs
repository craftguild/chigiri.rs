//! Test bounded supplemental polling independent of wake notifications.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use chigiri_runtime::__private::{ArgumentValues, BoxFuture, ResolvedValue, ResolverDispatch};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{ExecutableSchema, FieldResult, GraphQlRequest, Value};

use super::{Dispatch, SCHEMA};

#[test]
fn immediately_ready_bounded_query_completes_on_the_first_outer_poll() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();
    let mut future = Box::pin(schema.execute(
        GraphQlRequest::new("{ hello echo(value: \"ready\") item { name } }"),
        (),
    ));
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(future.as_mut().poll(&mut context), Poll::Ready(_)));
}

#[test]
fn pending_child_wakes_the_outer_future_and_completes_on_the_follow_up_poll() {
    let schema = ExecutableSchema::builder(&SCHEMA, PendingOnceDispatch)
        .build()
        .unwrap();
    let mut future = Box::pin(schema.execute(GraphQlRequest::new("{ hello }"), ()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(CountingWake(Arc::clone(&wakes))));
    let mut context = Context::from_waker(&waker);

    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    assert!(wakes.load(Ordering::SeqCst) > 0);
    assert!(matches!(future.as_mut().poll(&mut context), Poll::Ready(_)));
}

struct CountingWake(Arc<AtomicUsize>);

impl Wake for CountingWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingOnceDispatch;

impl ResolverDispatch<()> for PendingOnceDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(PendingOnce { pending: true })
    }
}

struct PendingOnce {
    pending: bool,
}

impl Future for PendingOnce {
    type Output = FieldResult<ResolvedValue>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.pending {
            self.pending = false;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(ResolvedValue::Leaf(Value::from("ready"))))
        }
    }
}
