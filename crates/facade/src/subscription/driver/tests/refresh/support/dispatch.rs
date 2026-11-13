use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use chigiri_http::BoxFuture;
use chigiri_runtime::__private::{
    ArgumentValues, ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent,
    ResolvedObject, ResolvedValue, ResolverDispatch, ResolverSchemaFingerprint,
    SubscriptionResolverDispatch, descriptor_fingerprint, erase_subscription_source,
    invariant_error,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    FieldContext, FieldError, FieldResult, SubscriptionOverflow, SubscriptionSource, Value,
};
use futures_core::Stream;

use super::context::AppContext;
use super::{Probe, SCHEMA};

pub(super) struct Dispatch(pub(super) Arc<Probe>);

impl ResolverDispatch<AppContext> for Dispatch {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        registry.register("Probe", |_, value| Ok(value));
    }

    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            if (parent_type, field_name) != ("Event", "value") {
                return Err(invariant_error("unexpected refresh coordinate"));
            }
            let event = parent
                .and_then(|parent| parent.downcast_ref::<EventObject>())
                .ok_or_else(|| invariant_error("refresh event parent is absent"))?;
            assert_eq!(context.local.state.load(Ordering::SeqCst), 1);
            assert_eq!(context.local.budget.load(Ordering::SeqCst), 0);
            assert_eq!(*context.local.cache.lock().unwrap(), Some(event.value));
            context
                .service
                .records
                .lock()
                .unwrap()
                .nested_values
                .push(event.value);
            Ok(ResolvedValue::Leaf(Value::Int(i128::from(event.value))))
        })
    }
}

impl SubscriptionResolverDispatch<AppContext> for Dispatch {
    fn subscription_root(&self) -> Option<&'static str> {
        Some("Subscription")
    }

    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.0.setup_calls.fetch_add(1, Ordering::SeqCst);
        let limit = typed_limit(arguments);
        let values = self.0.sources.lock().unwrap().pop_front();
        let service = Arc::clone(&self.0);
        let token = context.context().token.clone();
        Box::pin(async move {
            let limit = limit?;
            let values = values.ok_or_else(|| invariant_error("refresh source is absent"))?;
            service.records.lock().unwrap().setup_tokens.push(token);
            let stream = EventStream {
                values: VecDeque::from(values),
                service: Arc::clone(&service),
            };
            let mapper_service = Arc::clone(&service);
            Ok(erase_subscription_source(
                SubscriptionSource::new(stream).with_overflow(SubscriptionOverflow::Backpressure),
                Args { limit },
                Arc::new(move |_: SourceFailure| {
                    mapper_service.mapper_calls.fetch_add(1, Ordering::SeqCst);
                    FieldError::internal(std::io::Error::other("private source"))
                }),
            ))
        })
    }

    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let context = context.context();
            assert!(Arc::ptr_eq(&context.service, &self.0));
            let args = arguments.downcast_ref::<Args>()?;
            let event = event.downcast_ref::<Event>()?;
            assert_eq!(event.publisher, "publisher");
            let start = (
                context.local.state.load(Ordering::SeqCst),
                context.local.budget.load(Ordering::SeqCst),
                context.local.cache.lock().unwrap().is_none(),
            );
            let revision = self.0.revision.load(Ordering::SeqCst);
            {
                let mut records = self.0.records.lock().unwrap();
                records.root_arguments.push(args.limit);
                records.policy_revisions.push(revision);
                records.local_starts.push(start);
            }
            if self.0.allowed_limit.load(Ordering::SeqCst) != args.limit {
                return Err(FieldError::public("root argument denied").code("FORBIDDEN"));
            }
            assert_eq!(context.local.state.swap(1, Ordering::SeqCst), 0);
            assert_eq!(context.local.budget.swap(0, Ordering::SeqCst), 1);
            *context.local.cache.lock().unwrap() = Some(event.value);
            Ok(ResolvedValue::object(
                "Event",
                EventObject { value: event.value },
            ))
        })
    }
}

fn typed_limit(arguments: ArgumentValues) -> FieldResult<i32> {
    let Some(Value::Int(value)) = arguments
        .into_values()
        .next()
        .and_then(|value| value.into_value())
    else {
        return Err(invariant_error("refresh limit is absent"));
    };
    i32::try_from(value).map_err(|_| invariant_error("refresh limit is outside i32"))
}

struct Args {
    limit: i32,
}

struct Event {
    value: i32,
    publisher: &'static str,
}

struct EventObject {
    value: i32,
}

#[derive(Debug)]
struct SourceFailure;

struct EventStream {
    values: VecDeque<i32>,
    service: Arc<Probe>,
}

impl Stream for EventStream {
    type Item = Result<Event, SourceFailure>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.service.source_polls.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(self.values.pop_front().map(|value| {
            Ok(Event {
                value,
                publisher: "publisher",
            })
        }))
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.service.source_drops.fetch_add(1, Ordering::SeqCst);
    }
}
