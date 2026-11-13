//! Inject recoverable panics at existing catch boundaries and preserve independent failures over early cancellation.

use super::*;
use crate::limits::request::BudgetFailure;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn panic_overrides_cleanup_cancellation_rolls_back_and_releases_worker_capacity() {
    let _case = crate::vm_gate::test_case();
    for disconnect in [false, true] {
        let fixture = Fixture::new(
            "type Item{name:String!} type Query{count:Int!} type Mutation{add:Int!}",
            "{}",
            &[
                ("Query.count", "return ctx.models.Item:count({})"),
                (
                    "Mutation.add",
                    "ctx.models.Item:create({name='candidate'}) return 1",
                ),
            ],
            &[(
                HookSlot::FieldAfter,
                "ctx.log('info','panic-boundary') return nil",
            )],
        );
        let entered = Arc::new(tokio::sync::Notify::new());
        let observed = entered.clone();
        let (release, held) = std::sync::mpsc::sync_channel(1);
        let held = Mutex::new(held);
        let injected = Arc::new(AtomicBool::new(false));
        let seen = injected.clone();
        let terminals = Arc::new(Mutex::new(Vec::new()));
        let events = terminals.clone();
        let completed = Arc::new(tokio::sync::Notify::new());
        let signal = completed.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_events(move |event| {
                if event
                    .record
                    .as_ref()
                    .is_ok_and(|log| log.message == "panic-boundary")
                {
                    observed.notify_one();
                    let released = held
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_millis(400));
                    if released.is_ok() {
                        seen.store(true, Ordering::Release);
                        panic!("controlled request panic");
                    }
                }
            })
            .with_terminal(move |event| {
                events.lock().unwrap().push(event);
                signal.notify_one();
            });
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("content-type", b"application/json")];
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let mut running = Box::pin(scope.run(fixture.endpoint.handle(
            request,
            BufferedBody::new(serde_json::to_vec(&json!({"query":"mutation{add}"})).unwrap()),
            &Clock,
        )));
        tokio::select! {
            _ = &mut running => panic!("VM must remain pending at the panic boundary"),
            _ = entered.notified() => {}
        }
        // Record cleanup cancellation first, then inject an independent panic whose cause must win.
        scope
            .0
            .execution_budget
            .0
            .clone()
            .fail_with(BudgetFailure::Cancelled);
        scope.0.cancel.cancel();
        if disconnect {
            drop(running);
            assert!(terminals.lock().unwrap().is_empty());
            release.send(()).unwrap();
        } else {
            release.send(()).unwrap();
            let response = running.await;
            assert_eq!(response.status, 500);
            let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
            assert_eq!(body["data"], json!(null));
            assert_eq!(
                body["errors"][0]["extensions"]["code"],
                "MOCK_EXECUTION_FAILED"
            );
            assert!(!body.to_string().contains("controlled request panic"));
        }
        tokio::time::timeout(Duration::from_secs(1), completed.notified())
            .await
            .unwrap();
        assert!(
            injected.load(Ordering::Acquire),
            "a barrier timeout is not a successful panic injection"
        );
        assert_eq!(
            scope.0.execution_budget.failure(),
            Some(BudgetFailure::Execution)
        );
        drop(scope);
        {
            let events = terminals.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].cause, "execution_failed");
            assert_eq!(events[0].transaction, "rolled_back");
            assert_eq!(events[0].status, if disconnect { None } else { Some(500) });
        }
        assert_eq!(
            fixture.query("{count}").await.0,
            json!({"data":{"count":0}})
        );
        assert_eq!(
            fixture.query("mutation{add}").await.0,
            json!({"data":{"add":1}})
        );
        assert_eq!(
            fixture.query("{count}").await.0,
            json!({"data":{"count":1}})
        );
        fixture.stop().await;
    }
}
