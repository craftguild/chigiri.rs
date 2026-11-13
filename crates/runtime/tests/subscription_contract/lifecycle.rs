use std::sync::{Arc, Mutex};
use std::task::Poll;

use chigiri_runtime::__private::{BoxFuture, ResolvedValue};
use chigiri_runtime::{
    FieldContext, FieldNext, FieldResult, LifecycleEvent, LifecycleObserver, LifecyclePhase,
    LifecycleResult, Plugin,
};

use super::fixture::{AppContext, SourceStep, State, builder, count, poll_once, request};

type LifecycleRecord = (LifecyclePhase, LifecycleResult, &'static str);

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<LifecycleRecord>>>);

impl Recorder {
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    fn records(&self) -> Vec<LifecycleRecord> {
        self.0.lock().unwrap().clone()
    }
}

impl LifecycleObserver for Recorder {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.component()));
    }
}

struct PendingAfterSourceCapture;

impl Plugin<AppContext> for PendingAfterSourceCapture {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        next: FieldNext<'a, AppContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            next.run(context).await?;
            std::future::pending().await
        })
    }
}

#[test]
fn cancellation_after_source_capture_closes_each_started_lifecycle_once() {
    let state = State::new(vec![SourceStep::Event(1)]);
    let recorder = Recorder::default();
    let schema = builder(Arc::clone(&state))
        .plugin(PendingAfterSourceCapture)
        .lifecycle_observer(recorder.clone())
        .build()
        .unwrap();
    let context = AppContext::new(0);
    let prepared = schema.prepare_subscription(request()).unwrap();
    recorder.clear();

    let mut future = prepared.start(&context, None);
    assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    assert_eq!(count(&state.counts.setup_calls), 1);
    assert_eq!(count(&state.counts.source_drops), 0);
    drop(future);

    assert_eq!(count(&state.counts.source_drops), 1);
    assert_eq!(
        recorder.records(),
        [
            (
                LifecyclePhase::RequestPlugin,
                LifecycleResult::Started,
                "subscription",
            ),
            (
                LifecyclePhase::OperationAdmission,
                LifecycleResult::Started,
                "chigiri",
            ),
            (
                LifecyclePhase::OperationAdmission,
                LifecycleResult::Succeeded,
                "chigiri",
            ),
            (
                LifecyclePhase::Execution,
                LifecycleResult::Started,
                "chigiri",
            ),
            (
                LifecyclePhase::Execution,
                LifecycleResult::Cancelled,
                "chigiri",
            ),
            (
                LifecyclePhase::RequestPlugin,
                LifecycleResult::Cancelled,
                "subscription",
            ),
        ]
    );
}
