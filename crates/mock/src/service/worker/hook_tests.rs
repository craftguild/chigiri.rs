//! Verify validated-hook transfer, invalid-slot rejection, state propagation, cancellation, and worker recovery.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use super::{Client, Worker};
use crate::cancellation::Cancellation;
use crate::hooks::{HookSlot, StateContract};
use crate::luau::{Contract, HookCandidate, ScriptSet, static_candidates};
use crate::output::OutputValue;
use crate::runtime::{Execution, HookCall, HookOutcome};
use crate::store::Store;
use crate::value::ValueBudget;

pub(super) fn worker(slot: HookSlot, body: &str) -> Worker {
    let ir = crate::service::tests::ir("type Query { value:Int @chigiri_mock_resolver }");
    let state = StateContract::from_json(
        r#"{"fields":{"count":{"kind":"integer"}},"initial":{"count":1}}"#,
        &mut ValueBudget::default(),
    )
    .unwrap();
    let contract = Contract::with_state(ir.clone(), state).unwrap();
    let hooks = contract
        .hooks(
            vec![HookCandidate {
                slot,
                source: crate::luau::render_hook_module(slot, body).unwrap(),
            }],
            None,
        )
        .unwrap();
    let mut candidates = static_candidates(&ir).unwrap();
    candidates[0].source =
        crate::luau::render_module(&ir, candidates[0].script, "return ctx.state.count").unwrap();
    let scripts = ScriptSet::new(contract, candidates).unwrap();
    Worker::from_checked(scripts, hooks, Store::empty(ir)).unwrap()
}

pub(super) async fn call(client: &Client, slot: HookSlot) -> HookCall {
    let deadline = Instant::now() + Duration::from_secs(5);
    HookCall {
        snapshot: client.snapshot(deadline).await.unwrap(),
        state: client.initial_state(),
        request: None,
        views: crate::api::hooks::phase_fields(slot)
            .into_iter()
            .map(|(name, _)| (name, Arc::new(Value::Object(Object::new()))))
            .collect(),
        deadline,
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget: Default::default(),
        log: Arc::new(|_| {}),
    }
}

#[tokio::test]
async fn hk_04_06_worker_receives_checked_hooks_and_uses_only_confirmed_request_state() {
    let _case = crate::vm_gate::test_case();
    let mut worker = worker(HookSlot::RequestBefore, "return {statePatch={count=41}} ");
    let client = &worker.client;
    assert!(client.enabled(HookSlot::RequestBefore));
    assert!(!client.enabled(HookSlot::FieldBefore));
    let input = call(client, HookSlot::RequestBefore).await;
    let snapshot = input.snapshot.clone();
    let HookOutcome::Continue(Some(state)) =
        client.hook(HookSlot::RequestBefore, input).await.unwrap()
    else {
        panic!("state patch required")
    };
    let execution = Execution {
        state: state.snapshot(),
        request: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget: Default::default(),
    };
    let field = client.ir().scripts()[0].field;
    let result = client
        .execute_with(
            field,
            Object::new(),
            None,
            Some(snapshot.clone()),
            execution,
        )
        .await
        .unwrap();
    assert!(matches!(
        result.output.unwrap(),
        OutputValue::Scalar(Value::Int(41))
    ));
    assert!(snapshot.same_version(&result.snapshot));
    assert_eq!(
        client.initial_state().snapshot().get("count"),
        Some(&Value::Int(1))
    );
    assert!(matches!(
        client.execute(field, Object::new(), None).await.unwrap(),
        OutputValue::Scalar(Value::Int(1))
    ));
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(worker.thread.is_none());
}

#[tokio::test]
async fn hk_14_disabled_hook_never_enters_queue_even_with_expired_or_invalid_call() {
    let _case = crate::vm_gate::test_case();
    let ir = crate::service::tests::ir("type Query { value:Int @chigiri_mock_resolver }");
    let (client, receiver) = super::queue(ir.clone());
    for slot in HookSlot::ALL {
        let input = HookCall {
            snapshot: Store::empty(ir.clone()).snapshot(),
            state: client.initial_state(),
            request: None,
            views: Default::default(),
            deadline: Instant::now() - Duration::from_secs(1),
            cancel: Cancellation::default(),
            shutdown: Cancellation::default(),
            budget: Default::default(),
            log: Arc::new(|_| panic!("log of the hook is deactivated.")),
        };
        assert!(matches!(
            client.hook(slot, input).await.unwrap(),
            HookOutcome::Continue(None)
        ));
        assert!(receiver.is_empty());
    }
}

#[tokio::test]
async fn hk_14_running_hook_observes_request_cancel_and_future_drop_then_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    for slot in HookSlot::ALL {
        cancel_running_hook(slot).await;
    }
}

async fn cancel_running_hook(slot: HookSlot) {
    let mut worker = worker(slot, "ctx.log('debug','entered') while true do end");
    for abort in [false, true] {
        let mut input = call(&worker.client, slot).await;
        let cancel = input.cancel.clone();
        let entered = Arc::new(AtomicBool::new(false));
        let observed = entered.clone();
        input.log = Arc::new(move |_| {
            observed.store(true, Ordering::SeqCst);
        });
        let client = worker.client.clone();
        let task = tokio::spawn(async move { client.hook(slot, input).await });
        let limit = Instant::now() + Duration::from_secs(2);
        while !entered.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < limit,
                "the hook must start before the wait expires"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let started = Instant::now();
        if abort {
            task.abort();
            assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        } else {
            cancel.cancel();
            assert!(task.await.unwrap().is_err());
        }
        let field = worker.client.ir().scripts()[0].field;
        assert!(
            worker
                .client
                .execute(field, Object::new(), None)
                .await
                .is_ok()
        );
        assert!(started.elapsed() < Duration::from_millis(400));
    }
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(worker.thread.is_none());
}
