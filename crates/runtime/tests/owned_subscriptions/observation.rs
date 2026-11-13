//! Prove preparation-minted transfer gating through the real runtime setup.

#[path = "observation/terminal.rs"]
mod terminal;

use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ErasedSubscription, ErasedSubscriptionArguments,
    ErasedSubscriptionEvent, HostSubscriptionObservation, ResolvedObject, ResolvedValue,
    SubscriptionObservationReceipt, SubscriptionResolverDispatch, SubscriptionStart,
    SubscriptionTermination,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    FieldContext, FieldResult, OwnedResolverDispatch, OwnedSchema, SubscriptionOverflow,
};

use super::fixture::{AppContext, Dispatch, block_on, fixture, request};

#[derive(Default)]
struct State {
    receipt: Mutex<Option<SubscriptionObservationReceipt>>,
    terminals: Mutex<Vec<SubscriptionTermination>>,
}

struct Observed {
    inner: Dispatch,
    state: Arc<State>,
    omit_receipt: bool,
}

struct Host(Arc<State>);
impl HostSubscriptionObservation for Host {
    fn retained_metadata_bytes(&self) -> usize {
        128
    }
    fn flush(&mut self) {}
    fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        overflow == SubscriptionOverflow::Backpressure
    }
    fn drop_oldest(&mut self, _: u64) {}
    fn finish(&mut self, decision: SubscriptionTermination, _: u64) {
        self.0.terminals.lock().unwrap().push(decision);
    }
}

impl OwnedResolverDispatch<AppContext> for Observed {
    fn schema(&self) -> &OwnedSchema {
        self.inner.schema()
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        self.inner.coordinates()
    }
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        self.inner.register_custom_scalars(registry);
    }
    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.inner
            .resolve(context, parent_type, field_name, parent, arguments)
    }
}

impl SubscriptionResolverDispatch<AppContext> for Observed {
    fn event_execution_failed(&self, context: &AppContext) -> bool {
        self.inner.event_execution_failed(context)
    }

    fn subscription_root(&self) -> Option<&str> {
        self.inner.subscription_root()
    }
    fn prepare_observation(
        &self,
        _: &AppContext,
        parent_type: &str,
        field_name: &str,
        receipt: SubscriptionObservationReceipt,
        _: usize,
    ) -> FieldResult<Option<Box<dyn HostSubscriptionObservation>>> {
        assert_eq!(Some(parent_type), self.subscription_root());
        assert_eq!(field_name, "events");
        *self.state.receipt.lock().unwrap() = Some(receipt);
        Ok(Some(Box::new(Host(self.state.clone()))))
    }
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        Box::pin(async move {
            let source = self.inner.subscribe(context, arguments).await?;
            if self.omit_receipt {
                return Ok(source);
            }
            let receipt = self.state.receipt.lock().unwrap().clone().unwrap();
            source.with_host_observation(receipt)
        })
    }
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.inner.resolve_event(context, arguments, event)
    }
}

#[test]
fn ordinary_owned_dispatch_prepares_no_host_owner_even_at_zero_remaining_bytes() {
    let (schema, dispatch, calls) = fixture();
    let schema = schema
        .bind_with_subscriptions(dispatch)
        .unwrap()
        .into_executable();
    let mut prepared = schema.prepare_subscription(request()).unwrap();
    assert!(
        prepared
            .prepare_host_observation(&AppContext(0), 0)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        block_on(prepared.start(&AppContext(0), None)),
        SubscriptionStart::Ready(_)
    ));
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
}

#[test]
fn real_initializer_cannot_run_until_the_minted_owner_is_accepted_outside() {
    let (schema, inner, calls) = fixture();
    let state = Arc::<State>::default();
    let schema = schema
        .bind_with_subscriptions(Observed {
            inner,
            state: state.clone(),
            omit_receipt: false,
        })
        .unwrap()
        .into_executable();
    let mut prepared = schema.prepare_subscription(request()).unwrap();
    let context = AppContext(0);
    let mut owner = prepared
        .prepare_host_observation(&context, 1024)
        .unwrap()
        .unwrap();
    assert!(owner.retained_metadata_bytes() <= 1024);
    assert!(prepared.prepare_host_observation(&context, 1024).is_err());
    let mut start = Box::pin(prepared.start(&context, None));
    let mut task = Context::from_waker(Waker::noop());
    assert!(start.as_mut().poll(&mut task).is_pending());
    assert_eq!(calls.setup.load(Ordering::SeqCst), 0);
    assert!(owner.accept());
    let session = match block_on(start) {
        SubscriptionStart::Ready(session) => session,
        SubscriptionStart::Response(response) => panic!("unexpected setup failure {response:?}"),
    };
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
    assert!(owner.registered(session.host_observation_receipt(), session.overflow()));
    owner.finish(SubscriptionTermination::ServerShutdown, 0);
    drop(session);
    assert_eq!(
        *state.terminals.lock().unwrap(),
        vec![SubscriptionTermination::ServerShutdown]
    );
}

#[test]
fn cancellation_before_entry_and_ignored_metadata_failure_both_close_late_setup() {
    for exhausted in [false, true] {
        let (schema, inner, calls) = fixture();
        let state = Arc::<State>::default();
        let schema = schema
            .bind_with_subscriptions(Observed {
                inner,
                state: state.clone(),
                omit_receipt: false,
            })
            .unwrap()
            .into_executable();
        let mut prepared = schema.prepare_subscription(request()).unwrap();
        if exhausted {
            assert!(
                prepared
                    .prepare_host_observation(&AppContext(0), 0)
                    .is_err()
            );
        } else {
            let mut owner = prepared
                .prepare_host_observation(&AppContext(0), 1024)
                .unwrap()
                .unwrap();
            owner.finish(SubscriptionTermination::ClientCancelled, 0);
        }
        assert!(matches!(
            block_on(prepared.start(&AppContext(0), None)),
            SubscriptionStart::Response(_)
        ));
        assert_eq!(calls.setup.load(Ordering::SeqCst), 0);
        assert!(state.receipt.lock().unwrap().as_ref().unwrap().is_closed());
    }
}

#[test]
fn a_source_without_the_matching_receipt_is_rejected_after_setup_and_released() {
    let (schema, inner, calls) = fixture();
    let state = Arc::<State>::default();
    let schema = schema
        .bind_with_subscriptions(Observed {
            inner,
            state,
            omit_receipt: true,
        })
        .unwrap()
        .into_executable();
    let mut prepared = schema.prepare_subscription(request()).unwrap();
    let mut owner = prepared
        .prepare_host_observation(&AppContext(0), 1024)
        .unwrap()
        .unwrap();
    assert!(owner.accept());
    let SubscriptionStart::Response(response) = block_on(prepared.start(&AppContext(0), None))
    else {
        panic!("a mismatched source receipt must fail closed");
    };
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(calls.setup.load(Ordering::SeqCst), 1);
    assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
}
