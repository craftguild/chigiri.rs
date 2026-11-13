//! Exercise source delivery, idle expiry and physical release during a real VM job.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use futures_core::Stream;

use super::Worker;
use crate::events::{EventSettings, SourceLimits};
use crate::hooks::HookSlot;
use crate::luau::{Contract, HookCandidate, ScriptSet, static_candidates};
use crate::publication::{CapturedEvent, test_support};
use crate::store::Store;

struct Release(mpsc::SyncSender<()>);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

fn fixture() -> Worker {
    let ir =
        crate::service::tests::ir("type Query { value: Int } type Subscription { event: String! }");
    let contract = Contract::new(ir.clone()).unwrap();
    let hooks = contract
        .hooks(
            vec![HookCandidate {
                slot: HookSlot::RequestBefore,
                source: crate::luau::render_hook_module(
                    HookSlot::RequestBefore,
                    "ctx.log('debug', 'entered') return nil",
                )
                .unwrap(),
            }],
            None,
        )
        .unwrap();
    let scripts = ScriptSet::new(contract, static_candidates(&ir).unwrap()).unwrap();
    let limits = SourceLimits::new(32, 256 * 1024, 50, 16 * 1024 * 1024, 1024 * 1024).unwrap();
    Worker::from_checked_with_events(
        scripts,
        hooks,
        Store::empty(ir),
        crate::clock::ClockSettings::fixed(100).unwrap(),
        EventSettings::new(limits, Default::default()).unwrap(),
        Vec::new(),
    )
    .unwrap()
}

async fn next(
    source: &mut (impl Stream<Item = Result<CapturedEvent, crate::luau::ScriptError>> + Unpin),
) -> Result<CapturedEvent, crate::luau::ScriptError> {
    tokio::time::timeout(
        Duration::from_secs(1),
        std::future::poll_fn(|cx| Pin::new(&mut *source).poll_next(cx)),
    )
    .await
    .expect("delivery watchdog")
    .expect("source must deliver an item")
}

#[tokio::test]
async fn delivery_and_age_expiry_progress_during_a_blocked_vm_and_both_threads_join() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let ir = worker.client.ir().clone();
    let root = ir.graph().subscription().unwrap();
    let field = ir.graph().types()[root.0].fields["event"];
    let (registered, peer, retention) = {
        let mut store = worker.client.committed.lock().unwrap();
        (
            test_support::register_source(&mut store, &ir, field, "events"),
            test_support::register_source(&mut store, &ir, field, "other"),
            store.source_policy().1,
        )
    };
    let counter = registered.source_delivery();
    let peer_counter = peer.source_delivery();
    let (mut source, _) = registered.into_parts();
    let (mut peer, _) = peer.into_parts();
    let mut input = super::hook_tests::call(&worker.client, HookSlot::RequestBefore).await;
    let cancel = input.cancel.clone();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, released) = mpsc::sync_channel(1);
    let release = Release(release);
    let released = Mutex::new(released);
    let busy = Arc::new(AtomicBool::new(false));
    let active = busy.clone();
    input.log = Arc::new(move |_| {
        active.store(true, Ordering::Release);
        let _ = entered.lock().unwrap().take().unwrap().send(());
        let _ = released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(1));
        active.store(false, Ordering::Release);
    });
    let client = worker.client.clone();
    let callback = tokio::spawn(async move { client.hook(HookSlot::RequestBefore, input).await });
    tokio::time::timeout(Duration::from_secs(1), observed)
        .await
        .unwrap()
        .unwrap();
    test_support::stage_scalar(
        &mut worker.client.committed.lock().unwrap(),
        &ir,
        field,
        "events",
        "executing",
    );
    let held = next(&mut source).await.unwrap();
    assert!(busy.load(Ordering::Acquire));
    assert!(!held.cancellation().unwrap().cancelled());
    test_support::stage_scalar(
        &mut worker.client.committed.lock().unwrap(),
        &ir,
        field,
        "events",
        "waiting",
    );
    // Do not poll the source: the driver must expire its waiting item itself.
    tokio::time::timeout(Duration::from_secs(1), async {
        while !counter.is_stopped() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("idle age watchdog");
    assert!(busy.load(Ordering::Acquire));
    assert!(held.cancellation().unwrap().cancelled());
    assert!(!peer_counter.is_stopped());
    assert_eq!(next(&mut source).await.err().unwrap().phase, "source");
    test_support::stage_scalar(
        &mut worker.client.committed.lock().unwrap(),
        &ir,
        field,
        "other",
        "peer-continues",
    );
    drop(next(&mut peer).await.unwrap());
    assert!(busy.load(Ordering::Acquire));
    // The owned barrier may outlast the real VM deadline. Cancel explicitly
    // instead of requiring that blocked call to complete successfully.
    cancel.cancel();
    drop(release);
    let error = callback
        .await
        .unwrap()
        .err()
        .expect("blocked callback must be cancelled");
    assert!(
        error.cancelled,
        "unexpected blocked callback error: {error:?}"
    );
    let recovered = super::hook_tests::call(&worker.client, HookSlot::RequestBefore).await;
    worker
        .client
        .hook(HookSlot::RequestBefore, recovered)
        .await
        .unwrap();
    worker
        .stop(Instant::now() + Duration::from_secs(1))
        .await
        .unwrap();
    assert!(worker.thread.is_none() && worker.delivery.is_none());
    drop((source, peer, counter, peer_counter));
    assert!(
        retention.used() > 0,
        "executing input is still physically owned"
    );
    drop(held);
    assert_eq!(retention.used(), 0);
}
