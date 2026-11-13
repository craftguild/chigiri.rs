//! Test cancellation of resolver futures made unnecessary by non-null propagation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use chigiri_runtime::__private::{ArgumentValues, BoxFuture, ResolvedValue, ResolverDispatch};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{ExecutableSchema, FieldError, FieldResult, GraphQlRequest, Value};

use super::{SCHEMA, block_on};

#[test]
fn non_null_bubbling_drops_unnecessary_pending_futures() {
    let dropped = Arc::new(AtomicBool::new(false));
    let schema = ExecutableSchema::builder(&SCHEMA, CancelDispatch(Arc::clone(&dropped)))
        .build()
        .unwrap();

    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"{ requiredError, echo(value: "pending") }"#),
        (),
    ));

    assert_eq!(response.data, Some(Value::Null));
    assert!(dropped.load(Ordering::SeqCst));
}

struct CancelDispatch(Arc<AtomicBool>);

impl ResolverDispatch<()> for CancelDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        field_name: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if field_name == "requiredError" {
            Box::pin(async { Err(FieldError::public("stop")) })
        } else {
            Box::pin(NeverFuture(Arc::clone(&self.0)))
        }
    }
}

struct NeverFuture(Arc<AtomicBool>);

impl Future for NeverFuture {
    type Output = FieldResult<ResolvedValue>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for NeverFuture {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
