//! Minimal real owned-schema binding with observable callback boundaries.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_core::executable::CustomScalarRegistry;
use chigiri_core::executor::{ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue};
use chigiri_core::{FieldResult, LifecycleEvent, LifecycleObserver, Source};

use crate::{ExecutableSchema, OwnedResolverDispatch, OwnedSchema};

#[derive(Default)]
pub(super) struct Calls {
    pub(super) scalar: AtomicUsize,
    pub(super) resolver: AtomicUsize,
    pub(super) lifecycle: AtomicUsize,
}

struct Observer(Arc<Calls>);

impl LifecycleObserver for Observer {
    fn observe(&self, _: LifecycleEvent<'_>) {
        self.0.lifecycle.fetch_add(1, Ordering::SeqCst);
    }
}

struct Dispatch {
    schema: OwnedSchema,
    coordinates: Vec<(Arc<str>, Arc<str>)>,
    calls: Arc<Calls>,
}

impl OwnedResolverDispatch<()> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        let calls = Arc::clone(&self.calls);
        registry.register("Probe", move |_, value| {
            calls.scalar.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        });
    }
    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.resolver.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ResolvedValue::Null) })
    }
}

pub(super) fn schema() -> (ExecutableSchema<()>, Arc<Calls>) {
    let owned = OwnedSchema::parse(vec![Source::new(
        "classifier.graphql",
        "scalar Probe type Query { echo(value: Probe): Probe }",
    )])
    .unwrap();
    let calls = Arc::new(Calls::default());
    let dispatch = Dispatch {
        schema: owned.clone(),
        coordinates: vec![(Arc::from("Query"), Arc::from("echo"))],
        calls: Arc::clone(&calls),
    };
    let schema = owned
        .bind(dispatch)
        .unwrap()
        .into_executable()
        .reconfigure()
        .lifecycle_observer(Observer(Arc::clone(&calls)))
        .build();
    calls.scalar.store(0, Ordering::SeqCst);
    calls.lifecycle.store(0, Ordering::SeqCst);
    (schema, calls)
}
