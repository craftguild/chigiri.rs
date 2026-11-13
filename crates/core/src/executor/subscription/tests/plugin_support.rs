use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::executor::{
    ArgumentValues, BoxFuture, FieldContext, FieldNext, OperationContext, OperationNext, Plugin,
    ResolvedValue, SubscriptionEventDispatch, SubscriptionSetupDispatch,
};
use crate::{FieldError, FieldResult, GraphQlResponse, Value};

use super::support::TestContext;

pub(super) struct DropSession {
    drops: Arc<AtomicUsize>,
}

impl DropSession {
    pub fn new(drops: &Arc<AtomicUsize>) -> Self {
        Self {
            drops: Arc::clone(drops),
        }
    }
}

impl Drop for DropSession {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct SessionSetup<S> {
    value: Mutex<Option<S>>,
    pub calls: AtomicUsize,
}

impl<S> SessionSetup<S> {
    pub fn new(value: S) -> Self {
        Self {
            value: Mutex::new(Some(value)),
            calls: AtomicUsize::new(0),
        }
    }
}

impl<S: Send + 'static> SubscriptionSetupDispatch<TestContext, S> for SessionSetup<S> {
    fn subscribe<'a>(
        &'a self,
        _context: FieldContext<'a, TestContext>,
        _arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<S>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let value = self.value.lock().unwrap().take().unwrap();
        Box::pin(async move { Ok(value) })
    }
}

pub(super) struct StaticEvent {
    result: Mutex<Option<FieldResult<ResolvedValue>>>,
    pub calls: AtomicUsize,
}

impl StaticEvent {
    pub fn new(result: FieldResult<ResolvedValue>) -> Self {
        Self {
            result: Mutex::new(Some(result)),
            calls: AtomicUsize::new(0),
        }
    }
}

impl SubscriptionEventDispatch<TestContext> for StaticEvent {
    fn resolve_event<'a>(
        &'a self,
        _context: FieldContext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result.lock().unwrap().take().unwrap();
        Box::pin(async move { result })
    }
}

pub(super) struct OperationShortCircuit(pub GraphQlResponse);

impl Plugin<TestContext> for OperationShortCircuit {
    fn operation<'a>(
        &'a self,
        _context: OperationContext<'a, TestContext>,
        _next: OperationNext<'a, TestContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        let response = self.0.clone();
        Box::pin(async move { response })
    }
}

pub(super) struct OperationExtension;

impl Plugin<TestContext> for OperationExtension {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, TestContext>,
        next: OperationNext<'a, TestContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            let mut response = next.run(context).await;
            response.extensions.insert("operationPlugin", true);
            response
        })
    }
}

pub(super) struct OperationReplaceAfterNext(pub GraphQlResponse);

impl Plugin<TestContext> for OperationReplaceAfterNext {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, TestContext>,
        next: OperationNext<'a, TestContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        let response = self.0.clone();
        Box::pin(async move {
            drop(next.run(context).await);
            response
        })
    }
}

pub(super) struct RootFieldShortCircuit;

impl Plugin<TestContext> for RootFieldShortCircuit {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
        next: FieldNext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.parent_type() != "Subscription" {
            return next.run(context);
        }
        Box::pin(async { Err(FieldError::public("root field rejected").code("ROOT_REJECTED")) })
    }
}

pub(super) struct RootFieldErrorAfterNext;

impl Plugin<TestContext> for RootFieldErrorAfterNext {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
        next: FieldNext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.parent_type() != "Subscription" {
            return next.run(context);
        }
        Box::pin(async move {
            drop(next.run(context).await);
            Err(FieldError::public("root field post-processing failed"))
        })
    }
}

pub(super) struct PendingAfterNext;

impl Plugin<TestContext> for PendingAfterNext {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
        next: FieldNext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.parent_type() != "Subscription" {
            return next.run(context);
        }
        Box::pin(async move {
            let result = next.run(context).await;
            std::future::pending::<()>().await;
            result
        })
    }
}

pub(super) fn extension_response() -> GraphQlResponse {
    let mut response = GraphQlResponse::executed(Value::Null);
    response.extensions.insert("shortCircuit", true);
    response
}
