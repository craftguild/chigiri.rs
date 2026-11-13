//! Existing ABI 8 traits with gated finite work and closed subscription faults.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use subscription_generated as generated;

#[derive(Default)]
pub(super) struct Probe {
    pub(super) calls: AtomicUsize,
    pub(super) dropped: AtomicUsize,
    pub(super) sources: AtomicUsize,
    pub(super) errors: AtomicUsize,
    released: AtomicBool,
    changed: tokio::sync::Notify,
    fail: bool,
    stream_failure_after_head: Option<bool>,
}

impl Probe {
    pub(super) fn new(fail: bool) -> Arc<Self> {
        Arc::new(Self {
            fail,
            ..Self::default()
        })
    }

    pub(super) fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.changed.notify_one();
    }

    pub(super) fn subscription_failure(after_head: bool) -> Arc<Self> {
        Arc::new(Self {
            stream_failure_after_head: Some(after_head),
            ..Self::default()
        })
    }
}

struct Types;
impl generated::SchemaTypes for Types {
    type Tick = ();
}

#[derive(Clone)]
struct Fields(Arc<Probe>);

struct Execution(Arc<Probe>);
impl Drop for Execution {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl generated::QueryResolver<(), Types, chigiri::FieldError> for Fields {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        let _execution = Execution(Arc::clone(&self.0));
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        while !self.0.released.load(Ordering::SeqCst) {
            self.0.changed.notified().await;
        }
        assert!(!self.0.fail, "private managed query infrastructure failure");
        Ok("ready".to_owned())
    }
}

impl generated::MutationResolver<(), Types, chigiri::FieldError> for Fields {
    async fn acknowledge<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("accepted".to_owned())
    }
}

impl generated::SubscriptionResolver<(), Types, chigiri::FieldError> for Fields {
    type TicksEvent = ();
    type FailingTicksEvent = ();

    async fn subscribe_ticks<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<(), chigiri::FieldError>> {
        self.0.sources.fetch_add(1, Ordering::SeqCst);
        assert!(
            self.0.stream_failure_after_head != Some(false),
            "private managed subscription preparation failure"
        );
        if self.0.stream_failure_after_head == Some(true) {
            let probe = Arc::clone(&self.0);
            let owner = Execution(Arc::clone(&probe));
            return Ok(chigiri::SubscriptionSource::new(
                futures_util::stream::once(async move {
                    let _owner = owner;
                    while !probe.released.load(Ordering::SeqCst) {
                        probe.changed.notified().await;
                    }
                    Ok(())
                }),
            ));
        }
        Ok(chigiri::SubscriptionSource::new(
            futures_util::stream::pending(),
        ))
    }

    async fn resolve_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a (),
    ) -> chigiri::FieldResult<()> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            self.0.stream_failure_after_head != Some(true),
            "private managed subscription event failure"
        );
        Ok(())
    }

    async fn subscribe_failing_ticks<'a>(
        &'a self,
        context: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<(), chigiri::FieldError>> {
        <Self as generated::SubscriptionResolver<(), Types, chigiri::FieldError>>::subscribe_ticks(
            self, context,
        )
        .await
    }

    async fn resolve_failing_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a (),
    ) -> chigiri::FieldResult<()> {
        Ok(())
    }
}

impl generated::TickResolver<(), Types, chigiri::FieldError> for Fields {
    async fn value<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a (),
    ) -> chigiri::FieldResult<i32> {
        Ok(0)
    }

    async fn padding<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a (),
    ) -> chigiri::FieldResult<String> {
        Ok(String::new())
    }
}

struct Observe(Arc<Probe>);
impl chigiri::ErrorObserver for Observe {
    fn observe_error(&self, _: chigiri::ErrorEvent<'_>) {
        self.0.errors.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) fn endpoint(probe: Arc<Probe>) -> chigiri::Endpoint<()> {
    let fields = Fields(Arc::clone(&probe));
    let resolvers = generated::Resolvers::<(), Types>::builder(|error: chigiri::FieldError| error)
        .with_query(fields.clone())
        .with_mutation(fields.clone())
        .with_subscription(fields.clone())
        .with_tick(fields)
        .build();
    chigiri::Endpoint::builder(resolvers)
        .without_context()
        .inject(|components| {
            components.observability().error_observer(Observe(probe));
        })
        .build()
        .unwrap()
}
