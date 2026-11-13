//! Real monotonic generation without demand, independent of apparent Mock time.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use chigiri::core::Object;
use futures_core::Stream;

use crate::events::{EventSettings, SourceLimits};
use crate::ir::{FieldId, ScriptId};
use crate::luau::{Contract, ScriptSet};
use crate::publication::CapturedEvent;
use crate::service::Worker;
use crate::store::Store;
use crate::value::ValueBudget;

fn worker(overflow: &str, looping: bool) -> (Worker, FieldId) {
    let ir = crate::service::tests::ir(
        "schema { query: Read subscription: Signals } type Read { now: Int! } type Signals { tick: Int! }",
    );
    let mut candidates =
        crate::service::tests::candidates(&ir, &[("Read.now", "return assert(os.time())")]);
    let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["tick"];
    let id = ScriptId(
        ir.scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap(),
    );
    let body = format!(
        "local unused=ctx.generator.sequence.Int({{values={{99}},interval_ms=1,loop=true}}) return ctx.generator.sequence.Int({{values={{1,2,3}},interval_ms=20,loop={looping},overflow='{overflow}'}})"
    );
    candidates[id.0].source =
        crate::luau::render_subscription_module(&ir, id, &body, None).unwrap();
    let scripts = ScriptSet::new(Contract::new(ir.clone()).unwrap(), candidates).unwrap();
    let limits = SourceLimits::new(2, 256 * 1024, 10_000, 16 * 1024 * 1024, 1024 * 1024).unwrap();
    let worker = Worker::from_checked_with_events(
        scripts,
        Vec::new(),
        Store::empty(ir),
        crate::clock::ClockSettings::fixed(100).unwrap(),
        EventSettings::new(limits, Default::default()).unwrap(),
        Vec::new(),
    )
    .unwrap();
    (worker, field)
}

async fn source(
    worker: &Worker,
    field: FieldId,
) -> (
    Pin<Box<impl Stream<Item = Result<CapturedEvent, crate::luau::ScriptError>> + Send + 'static>>,
    Arc<chigiri::__private::HostSourceDelivery>,
) {
    let execution = super::support::foreign_execution(&worker.client);
    let registered = worker
        .client
        .establish_subscription(field, Object::new(), None, execution, None)
        .await
        .unwrap();
    let delivery = registered.source_delivery_for_test();
    (Box::pin(registered.into_parts().unwrap().source), delivery)
}

async fn wait_until(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("active sequence watchdog");
}

#[tokio::test]
async fn real_generator_evicts_without_demand_at_fixed_time_and_continues_after_world_timeleap() {
    let _case = crate::vm_gate::test_case();
    let (mut worker, field) = worker("drop_oldest", true);
    let (mut source, counter) = source(&worker, field).await;
    wait_until(|| counter.snapshot().unwrap() >= 2).await;
    assert!(!counter.is_stopped());
    let read =
        worker.client.ir().graph().types()[worker.client.ir().graph().query().0].fields["now"];
    assert!(matches!(
        worker
            .client
            .execute(read, Object::new(), None)
            .await
            .unwrap(),
        crate::output::OutputValue::Scalar(chigiri::Value::Int(100))
    ));
    let before = counter.snapshot().unwrap();
    worker
        .control()
        .run_luau("timeleap.travel_to(1000000, {world=true})")
        .unwrap();
    assert!(matches!(
        worker
            .client
            .execute(read, Object::new(), None)
            .await
            .unwrap(),
        crate::output::OutputValue::Scalar(chigiri::Value::Int(1_000_000))
    ));
    wait_until(|| counter.snapshot().unwrap() > before).await;
    let Poll::Ready(Some(Ok(held))) = source
        .as_mut()
        .poll_next(&mut Context::from_waker(Waker::noop()))
    else {
        panic!("generated value required without new demand work")
    };
    let output = held
        .project(
            worker
                .client
                .snapshot(Instant::now() + Duration::from_secs(1))
                .await
                .unwrap(),
            field,
            &mut ValueBudget::default(),
        )
        .unwrap();
    assert!(matches!(
        output,
        crate::output::OutputValue::Scalar(chigiri::Value::Int(1..=3))
    ));
    assert!(held.dropped_total().unwrap() > 0);
    let (_, retention) = worker.client.committed.lock().unwrap().source_policy();
    worker
        .stop(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();
    assert!(held.cancellation().unwrap().cancelled());
    assert!(counter.is_stopped());
    drop((source, counter));
    assert!(retention.used() > 0);
    drop(held);
    assert_eq!(retention.used(), 0);
}

#[tokio::test]
async fn real_default_generator_stops_at_capacity_and_a_fresh_source_recovers() {
    let _case = crate::vm_gate::test_case();
    let (mut worker, field) = worker("backpressure", true);
    let (mut first, counter) = source(&worker, field).await;
    wait_until(|| counter.is_stopped()).await;
    assert_eq!(counter.snapshot(), Some(0));
    assert!(matches!(
        first
            .as_mut()
            .poll_next(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Some(Err(_)))
    ));
    drop((first, counter));
    let (mut fresh, counter) = source(&worker, field).await;
    let event = tokio::time::timeout(
        Duration::from_secs(1),
        std::future::poll_fn(|cx| fresh.as_mut().poll_next(cx)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(!event.cancellation().unwrap().cancelled());
    assert!(!counter.is_stopped());
    worker
        .stop(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();
    drop((fresh, event, counter));
    assert_eq!(
        worker
            .client
            .committed
            .lock()
            .unwrap()
            .source_policy()
            .1
            .used(),
        0
    );
}
