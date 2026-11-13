//! Exact/all-topic channels share bounded queues without retaining topic history.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use futures_core::Stream;

use super::super::source::PublicationSource;
use super::super::{CapturedEvent, PublicationLimits, PublicationPump, PublicationStore};
use super::support::{Fixture, fixture, scalar};
use crate::ir::FieldId;
use crate::store::Store;

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll(
    source: &mut PublicationSource,
) -> Poll<Option<Result<CapturedEvent, crate::luau::ScriptError>>> {
    Pin::new(source).poll_next(&mut Context::from_waker(&Waker::from(Arc::new(Noop))))
}

fn register(
    world: &Arc<Mutex<PublicationStore>>,
    fixture: &Fixture,
    field: FieldId,
    topic: Option<&str>,
) -> PublicationSource {
    let owner = world.lock().unwrap().registration_owner();
    let prepared = match topic {
        Some(topic) => owner.prepare(&fixture.ir, field, topic),
        None => owner.prepare_all_topics(&fixture.ir, field),
    }
    .unwrap();
    world
        .lock()
        .unwrap()
        .register_source(prepared, chigiri::SubscriptionOverflow::Backpressure)
        .ok()
        .unwrap()
        .finish()
        .into_parts()
        .0
}

fn stage(world: &Arc<Mutex<PublicationStore>>, fixture: &Fixture, topic: &str) {
    crate::publication::test_support::stage_scalar(
        &mut world.lock().unwrap(),
        &fixture.ir,
        fixture.event,
        topic,
        topic,
    );
}

fn drive(world: &Arc<Mutex<PublicationStore>>, pump: &mut PublicationPump) {
    for _ in 0..16 {
        pump.step(world).unwrap();
    }
}

#[test]
fn future_topics_broadcast_once_without_replay_cross_type_delivery_or_history() {
    let fixture = fixture();
    let world = Arc::new(Mutex::new(
        PublicationStore::new(Store::empty(fixture.ir.clone()), 3).unwrap(),
    ));
    stage(&world, &fixture, "before");
    let mut all = register(&world, &fixture, fixture.event, None);
    let mut exact = register(&world, &fixture, fixture.event, Some("literal:*"));
    let mut other = register(&world, &fixture, fixture.dto, None);
    let mut pump = PublicationPump::new();
    drive(&world, &mut pump);
    assert!(poll(&mut all).is_pending());
    for topic in ["literal:*", "literal:one"] {
        stage(&world, &fixture, topic);
        drive(&world, &mut pump);
        assert!(matches!(poll(&mut all), Poll::Ready(Some(Ok(_)))));
        if topic == "literal:*" {
            assert!(matches!(poll(&mut exact), Poll::Ready(Some(Ok(_)))));
        } else {
            assert!(poll(&mut exact).is_pending());
        }
        assert!(poll(&mut other).is_pending());
        assert!(poll(&mut all).is_pending());
    }
    for index in 0..64 {
        stage(&world, &fixture, &format!("future:{index}"));
        drive(&world, &mut pump);
        assert!(matches!(poll(&mut all), Poll::Ready(Some(Ok(_)))));
        assert_eq!(world.lock().unwrap().topics.counts(), (3, 3));
    }
    drop(all);
    drop(exact);
    drop(other);
    drive(&world, &mut pump);
    assert_eq!(world.lock().unwrap().topics.counts(), (0, 0));
    assert_eq!(world.lock().unwrap().source_policy().1.used(), 0);
}

#[test]
fn loss_on_a_new_topic_reaches_old_all_topic_bindings_only_for_that_type() {
    let fixture = fixture();
    let world = Arc::new(Mutex::new(
        PublicationStore::new(Store::empty(fixture.ir.clone()), 4).unwrap(),
    ));
    let mut old = register(&world, &fixture, fixture.event, None);
    let mut exact = register(&world, &fixture, fixture.event, Some("different"));
    let mut other = register(&world, &fixture, fixture.dto, None);
    let mut builder = super::super::batch::BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.event, "never-seen", &scalar("lost"))
        .unwrap();
    {
        let mut store = world.lock().unwrap();
        let sequence = store.topics.commit_sequence().unwrap();
        store
            .topics
            .raise_loss(&sequence, &builder.seal().unwrap())
            .unwrap();
    }
    let mut fresh = register(&world, &fixture, fixture.event, None);
    let mut pump = PublicationPump::new();
    drive(&world, &mut pump);
    assert!(matches!(poll(&mut old), Poll::Ready(Some(Err(_)))));
    assert!(poll(&mut exact).is_pending());
    assert!(poll(&mut other).is_pending());
    assert!(poll(&mut fresh).is_pending());
    stage(&world, &fixture, "never-seen");
    drive(&world, &mut pump);
    assert!(matches!(poll(&mut fresh), Poll::Ready(Some(Ok(_)))));
}
