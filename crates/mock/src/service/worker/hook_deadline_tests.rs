//! Verify all six slots share input limits, one deadline, and shutdown behavior across queue and worker boundaries.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use chigiri::core::Object;

use super::hook_tests::{call, worker};
use super::jobs::Work;
use crate::cancellation::Cancellation;
use crate::hooks::HookSlot;
use crate::runtime::HookCall;
use crate::store::Store;

#[tokio::test]
async fn hk_14_all_slots_keep_absolute_deadline_queue_16_drop_and_expired_non_admission() {
    let _case = crate::vm_gate::test_case();
    let ir = crate::service::tests::ir("type Query { value:Int @chigiri_mock_resolver }");
    for slot in HookSlot::ALL {
        let (mut client, receiver) = super::queue(ir.clone());
        client.hooks.insert(slot);
        let deadline = Instant::now() + Duration::from_secs(5);
        let snapshot = Store::empty(ir.clone()).snapshot();
        let input = |deadline| HookCall {
            snapshot: snapshot.clone(),
            state: client.initial_state(),
            request: None,
            views: Default::default(),
            deadline,
            cancel: Cancellation::default(),
            shutdown: Cancellation::default(),
            budget: Default::default(),
            log: Arc::new(|_| panic!("the queued VM must not emit a log")),
        };
        let mut context = Context::from_waker(Waker::noop());
        let mut pending = Vec::new();
        for _ in 0..16 {
            let mut future = Box::pin(client.hook(slot, input(deadline)));
            assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
            pending.push(future);
        }
        assert_eq!(receiver.len(), 16);
        let mut overflow = Box::pin(client.hook(slot, input(deadline)));
        assert!(matches!(
            overflow.as_mut().poll(&mut context),
            Poll::Ready(Err(_))
        ));
        assert_eq!(receiver.len(), 16);
        drop(pending);
        for job in receiver.try_iter() {
            assert_eq!(job.deadline, deadline);
            assert!(job.cancel.cancelled());
            let Work::Hook {
                slot: actual, call, ..
            } = job.work
            else {
                panic!("hook job required")
            };
            assert_eq!(actual, slot);
            assert_eq!(call.deadline, deadline);
            assert!(call.cancel.cancelled());
            assert!(call.snapshot.same_version(&snapshot));
        }
        // The last absolute deadline is not replaced by the new 500 ms, and each slot is rejected before entry.
        assert!(
            client
                .hook(slot, input(Instant::now() - Duration::from_secs(1)))
                .await
                .is_err()
        );
        assert!(receiver.is_empty());
        let mut next = Box::pin(client.hook(slot, input(deadline)));
        assert!(matches!(next.as_mut().poll(&mut context), Poll::Pending));
        drop(next);
        assert!(receiver.try_recv().unwrap().cancel.cancelled());
    }
}

#[tokio::test]
async fn hk_14_all_slots_stop_at_remaining_deadline_recover_and_join_on_shutdown() {
    let _case = crate::vm_gate::test_case();
    for slot in HookSlot::ALL {
        let mut worker = worker(slot, "ctx.log('debug','entered') while true do end");
        let mut input = call(&worker.client, slot).await;
        let entered = Arc::new(AtomicBool::new(false));
        let observed = entered.clone();
        input.log = Arc::new(move |_| observed.store(true, Ordering::SeqCst));
        let started = Instant::now();
        input.deadline = started + Duration::from_millis(80);
        assert!(worker.client.hook(slot, input).await.is_err());
        assert!(
            entered.load(Ordering::SeqCst),
            "the real hook must start before the deadline"
        );
        let field = worker.client.ir().scripts()[0].field;
        assert!(
            worker
                .client
                .execute(field, Object::new(), None)
                .await
                .is_ok()
        );
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "{}: should not wait for the VM's 500 ms timeout",
            slot.name()
        );

        let mut input = call(&worker.client, slot).await;
        entered.store(false, Ordering::SeqCst);
        let observed = entered.clone();
        input.log = Arc::new(move |_| observed.store(true, Ordering::SeqCst));
        let client = worker.client.clone();
        let task = tokio::spawn(async move { client.hook(slot, input).await });
        let limit = Instant::now() + Duration::from_secs(2);
        while !entered.load(Ordering::SeqCst) {
            assert!(Instant::now() < limit, "hook start watchdog expired");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let started = Instant::now();
        worker
            .stop(started + Duration::from_millis(400))
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_millis(400));
        assert!(
            worker.thread.is_none(),
            "the worker must finish before its shutdown thread joins"
        );
        assert!(task.await.unwrap().is_err());
        assert!(
            worker
                .client
                .execute(field, Object::new(), None)
                .await
                .is_err()
        );
    }
}
