//! A full dispatch queue rejects the complete candidate, then recovers after drain.

use std::pin::Pin;
use std::time::{Duration, Instant};

use futures_core::Stream;

use super::publication_tests::{fixture, publish, subscription};
use super::tests::{add, execution};
use crate::publication::{CapturedEvent, test_support::usage};

async fn next_event<S>(source: &mut S) -> Option<Result<CapturedEvent, crate::luau::ScriptError>>
where
    S: Stream<Item = Result<CapturedEvent, crate::luau::ScriptError>> + Unpin,
{
    std::future::poll_fn(|context| Pin::new(&mut *source).poll_next(context)).await
}

#[tokio::test]
async fn the_33rd_batch_rolls_back_and_the_same_worker_recovers_after_capacity_returns() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let setup = execution(&worker.client);
    let registered = worker
        .client
        .establish_subscription(
            subscription(&worker.client),
            chigiri::core::Object::new(),
            None,
            setup,
            None,
        )
        .await
        .unwrap();
    let crate::service::hooks::EstablishedParts {
        mut source,
        observation: receipt,
        ..
    } = registered.into_parts().unwrap();
    assert!(receipt.is_none());
    // Even after dispatch advances its cursor, undemanded source inputs retain
    // the original batch reservations through the frozen 32-batch bound.
    for index in 0..32 {
        let execution = execution(&worker.client);
        let client = worker
            .client
            .request_client(true, &execution)
            .await
            .unwrap();
        let id = format!("committed-{index}");
        add(&client, &execution, &id).await;
        publish(&client, "capacity");
        client.prepare_request(execution.deadline).await.unwrap();
        client.commit_request().unwrap();
    }
    assert_eq!(usage(&worker.client.committed.lock().unwrap()).0, 32);
    let rejected = execution(&worker.client);
    let client = worker.client.request_client(true, &rejected).await.unwrap();
    add(&client, &rejected, "must-roll-back").await;
    publish(&client, "capacity");
    assert!(client.prepare_request(rejected.deadline).await.is_err());
    assert!(rejected.budget.failure().is_some());
    assert!(client.commit_request().is_err());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        32
    );
    drop(client);
    // The independent driver must recover without borrowing source demand.
    tokio::time::timeout(Duration::from_secs(2), async {
        while usage(&worker.client.committed.lock().unwrap()) != (0, 0) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let Some(Err(terminal)) = next_event(&mut source).await else {
        panic!("pressure must terminate the blocking source")
    };
    assert_eq!(terminal.phase, "source");
    assert!(next_event(&mut source).await.is_none());
    let recovered = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &recovered)
        .await
        .unwrap();
    add(&client, &recovered, "recovered").await;
    publish(&client, "capacity");
    client.prepare_request(recovered.deadline).await.unwrap();
    client.commit_request().unwrap();
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        33
    );
    drop(client);
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}
