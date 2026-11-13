//! Controlled host activation and captured integer sources, independent of a VM.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use chigiri::{SubscriptionOverflow, Value};
use futures_core::Stream;

use crate::events::SourceLimits;
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;
use crate::publication::source::PublicationSource;
use crate::publication::{
    CapturedEvent, DefinitionBuilder, PublicationLimits, PublicationPump, PublicationStore,
    SequenceDefinition,
};
use crate::store::Store;
use crate::value::ValueBudget;

pub(super) struct Fixture {
    pub(super) ir: Arc<MockIr>,
    pub(super) world: Arc<Mutex<PublicationStore>>,
    pub(super) field: FieldId,
}

impl Fixture {
    pub(super) fn new(queued: usize, bindings: usize) -> Self {
        let ir = Arc::new(MockIr::parse(vec![chigiri::core::Source::new(
            "sequence.graphql", "schema {query: Read subscription: Signals} type Read {ping: Int} type Signals {tick: Int! other: Int!}",
        )]).unwrap());
        let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["tick"];
        let limits =
            SourceLimits::new(queued, 256 * 1024, 60_000, 16 * 1024 * 1024, 1024 * 1024).unwrap();
        let store =
            PublicationStore::with_source_limits(Store::empty(ir.clone()), bindings, limits)
                .unwrap();
        Self {
            ir,
            field,
            world: Arc::new(Mutex::new(store)),
        }
    }

    pub(super) fn definition(&self, looping: bool) -> Arc<SequenceDefinition> {
        let (limits, retention) = self.world.lock().unwrap().source_policy();
        let mut builder =
            DefinitionBuilder::new(&self.ir, self.field, 3, 10, looping, limits, &retention)
                .unwrap();
        for value in 1..=3 {
            builder
                .push(&self.ir, &OutputValue::Scalar(value.into()))
                .unwrap();
        }
        builder.seal().unwrap()
    }

    pub(super) fn register(
        &self,
        definition: &Arc<SequenceDefinition>,
        overflow: SubscriptionOverflow,
        origin: Instant,
    ) -> (
        PublicationSource,
        Arc<chigiri::__private::HostSourceDelivery>,
    ) {
        let owner = self.world.lock().unwrap().registration_owner();
        let prepared = owner
            .prepare_sequence(&self.ir, self.field, definition)
            .unwrap();
        let registered = self
            .world
            .lock()
            .unwrap()
            .register_source(prepared, overflow)
            .ok()
            .unwrap()
            .finish_at(origin);
        let delivery = registered.source_delivery();
        (registered.into_parts().0, delivery)
    }

    pub(super) fn pubsub(&self, all_topics: bool) -> PublicationSource {
        let owner = self.world.lock().unwrap().registration_owner();
        let prepared = if all_topics {
            owner.prepare_all_topics(&self.ir, self.field)
        } else {
            owner.prepare(&self.ir, self.field, "events")
        }
        .unwrap();
        self.world
            .lock()
            .unwrap()
            .register_source(prepared, SubscriptionOverflow::Backpressure)
            .ok()
            .unwrap()
            .finish()
            .into_parts()
            .0
    }

    pub(super) fn publish(&self, value: i128) {
        let (owner, staging) = {
            let store = self.world.lock().unwrap();
            (store.capture_owner(), store.staging())
        };
        let buffer = owner.buffer(PublicationLimits::default()).unwrap();
        buffer
            .push(
                &self.ir,
                self.field,
                "events",
                &OutputValue::Scalar(Value::Int(value)),
            )
            .unwrap();
        let prepared = buffer.prepare(&staging).unwrap().unwrap();
        let sequence = self.world.lock().unwrap().topics.commit_sequence().unwrap();
        assert!(staging.stage_retained(prepared.retained, sequence).is_ok());
    }

    pub(super) fn value(&self, event: &CapturedEvent) -> i128 {
        let snapshot = self.world.lock().unwrap().snapshot();
        let OutputValue::Scalar(Value::Int(value)) = event
            .project(snapshot, self.field, &mut ValueBudget::default())
            .unwrap()
        else {
            panic!("integer source value required")
        };
        value
    }

    pub(super) fn drive(&self, pump: &mut PublicationPump, now: Instant) {
        for _ in 0..64 {
            if !pump.step_at(&self.world, now).unwrap() {
                return;
            }
        }
        panic!("bounded pump did not become idle");
    }
}

pub(super) fn poll(
    source: &mut PublicationSource,
) -> Poll<Option<Result<CapturedEvent, crate::luau::ScriptError>>> {
    Pin::new(source).poll_next(&mut Context::from_waker(Waker::noop()))
}

pub(super) fn event(source: &mut PublicationSource) -> CapturedEvent {
    let Poll::Ready(Some(Ok(event))) = poll(source) else {
        panic!("source event required")
    };
    event
}
