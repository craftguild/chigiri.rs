use futures_util::stream;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use subscription_generated as generated;

#[path = "fixture/disconnect.rs"]
mod disconnect;
#[path = "fixture/flood.rs"]
mod flood;

pub(crate) use disconnect::DisconnectProbe;
use disconnect::PendingTicksFactory;
use flood::FloodFactory;
pub(crate) use flood::FloodProbe;

pub struct Types;

#[derive(Clone)]
pub struct Tick {
    value: i32,
}

impl generated::SchemaTypes for Types {
    type Tick = Tick;
}

struct QueryFields;
struct MutationFields;

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("hello".to_owned())
    }
}

impl generated::MutationResolver<(), Types, chigiri::FieldError> for MutationFields {
    async fn acknowledge<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("accepted".to_owned())
    }
}

struct SubscriptionFields {
    failure_mode: FailureMode,
    ticks: TicksMode,
}

enum TicksMode {
    Finite,
    Pending(PendingTicksFactory),
    Flood(FloodFactory),
}

#[derive(Clone, Copy)]
enum FailureMode {
    AfterEvent,
    BeforeEvent,
    Initializer,
}

impl generated::SubscriptionResolver<(), Types, chigiri::FieldError> for SubscriptionFields {
    type TicksEvent = Tick;
    type FailingTicksEvent = Tick;

    async fn subscribe_ticks<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<Self::TicksEvent, chigiri::FieldError>>
    {
        match &self.ticks {
            TicksMode::Pending(factory) => {
                return Ok(chigiri::SubscriptionSource::new(factory.source()));
            }
            TicksMode::Flood(factory) => {
                return Ok(chigiri::SubscriptionSource::new(factory.source()));
            }
            TicksMode::Finite => {}
        }
        Ok(chigiri::SubscriptionSource::new(stream::iter([
            Ok(Tick { value: 1 }),
            Ok(Tick { value: 2 }),
        ])))
    }

    async fn resolve_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        event: &'a Self::TicksEvent,
    ) -> chigiri::FieldResult<Tick> {
        Ok(event.clone())
    }

    async fn subscribe_failing_ticks<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<
        chigiri::SubscriptionSource<Self::FailingTicksEvent, chigiri::FieldError>,
    > {
        if matches!(self.failure_mode, FailureMode::Initializer) {
            return Err(
                chigiri::FieldError::public("initializer denied").code("INITIALIZER_DENIED")
            );
        }
        let mut events = Vec::new();
        if matches!(self.failure_mode, FailureMode::AfterEvent) {
            events.push(Ok(Tick { value: 7 }));
        }
        events.push(Err(
            chigiri::FieldError::public("source failed").code("SOURCE_FAILED")
        ));
        Ok(chigiri::SubscriptionSource::new(stream::iter(events)))
    }

    async fn resolve_failing_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        event: &'a Self::FailingTicksEvent,
    ) -> chigiri::FieldResult<Tick> {
        Ok(event.clone())
    }
}

struct TickFields;

impl generated::TickResolver<(), Types, chigiri::FieldError> for TickFields {
    async fn padding<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Tick,
    ) -> chigiri::FieldResult<String> {
        Ok("p".repeat(32 * 1024))
    }

    async fn value<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        parent: &'a Tick,
    ) -> chigiri::FieldResult<i32> {
        Ok(parent.value)
    }
}

fn resolvers(
    mapped_errors: Arc<AtomicUsize>,
    failure_mode: FailureMode,
    ticks: TicksMode,
) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(move |error| {
        mapped_errors.fetch_add(1, Ordering::SeqCst);
        error
    })
    .with_query(QueryFields)
    .with_mutation(MutationFields)
    .with_subscription(SubscriptionFields {
        failure_mode,
        ticks,
    })
    .with_tick(TickFields)
    .build()
}

pub fn endpoint() -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_failure_mode(FailureMode::AfterEvent)
}

pub fn endpoint_with_immediate_source_error() -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_failure_mode(FailureMode::BeforeEvent)
}

pub fn endpoint_with_initializer_error() -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_failure_mode(FailureMode::Initializer)
}

pub fn endpoint_with_pending_ticks() -> (chigiri::Endpoint<()>, Arc<AtomicUsize>, DisconnectProbe) {
    let (pending_ticks, probe) = disconnect::pending_ticks();
    let (endpoint, mapped_errors) =
        endpoint_with_parts(FailureMode::AfterEvent, TicksMode::Pending(pending_ticks));
    (endpoint, mapped_errors, probe)
}

fn endpoint_with_failure_mode(
    failure_mode: FailureMode,
) -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_parts(failure_mode, TicksMode::Finite)
}

fn endpoint_with_parts(
    failure_mode: FailureMode,
    ticks: TicksMode,
) -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_options(
        failure_mode,
        ticks,
        chigiri::AcceptConnectionInit,
        chigiri::SubscriptionLimits::default(),
    )
}

pub fn endpoint_with_authorizer(
    authorizer: impl chigiri::ConnectionInitAuthorizer,
) -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    endpoint_with_options(
        FailureMode::AfterEvent,
        TicksMode::Finite,
        authorizer,
        chigiri::SubscriptionLimits::default(),
    )
}

pub fn endpoint_with_flood() -> (chigiri::Endpoint<()>, Arc<AtomicUsize>, FloodProbe) {
    let (factory, probe) = flood::source();
    let limits = chigiri::SubscriptionLimits {
        max_queued_events: 2,
        max_queue_age: std::time::Duration::from_millis(200),
        send_no_progress_timeout: std::time::Duration::from_secs(2),
        ..chigiri::SubscriptionLimits::default()
    };
    let (endpoint, mapped) = endpoint_with_options(
        FailureMode::AfterEvent,
        TicksMode::Flood(factory),
        chigiri::AcceptConnectionInit,
        limits,
    );
    (endpoint, mapped, probe)
}

fn endpoint_with_options(
    failure_mode: FailureMode,
    ticks: TicksMode,
    authorizer: impl chigiri::ConnectionInitAuthorizer,
    limits: chigiri::SubscriptionLimits,
) -> (chigiri::Endpoint<()>, Arc<AtomicUsize>) {
    let mapped_errors = Arc::new(AtomicUsize::new(0));
    let endpoint =
        chigiri::Endpoint::builder(resolvers(Arc::clone(&mapped_errors), failure_mode, ticks))
            .connection_init_authorizer(authorizer)
            .subscription_limits(limits)
            .without_context()
            .build()
            .expect("subscription fixture endpoint");
    (endpoint, mapped_errors)
}
