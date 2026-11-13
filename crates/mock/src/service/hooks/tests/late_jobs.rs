//! Block VM log delivery, then preserve an independent error found while dropping the receiving future.

use super::*;
use crate::service::hooks::{completion::Failure, session::Session};
use std::future::Future;

#[tokio::test]
async fn late_vm_error_overrides_rejection_and_session_teardown_reports_once() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{value:Int! @chigiri_mock_resolver}",
        "{}",
        &[("Query.value", "return 1")],
        &[(
            HookSlot::RequestBefore,
            "pcall(function() ctx.log('info','entered') ctx.log('info','interrupted') end) local x:any = {} local y:any = 1 return x + y",
        )],
    );
    for disconnect in [false, true] {
        let entered = Arc::new(tokio::sync::Notify::new());
        let observed = entered.clone();
        let (release, held) = std::sync::mpsc::sync_channel(1);
        let held = Mutex::new(held);
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let resumed = released.clone();
        let terminals = Arc::new(Mutex::new(Vec::new()));
        let events = terminals.clone();
        let failures = Arc::new(Mutex::new(Vec::new()));
        let diagnostics = failures.clone();
        let completed = Arc::new(tokio::sync::Notify::new());
        let signal = completed.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_events(move |event| {
                if event
                    .record
                    .as_ref()
                    .is_ok_and(|log| log.message == "entered")
                {
                    observed.notify_one();
                    let received = held
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_millis(400));
                    resumed.store(received.is_ok(), std::sync::atomic::Ordering::Release);
                }
                if let Err(error) = event.record {
                    diagnostics.lock().unwrap().push(error);
                }
            })
            .with_terminal(move |event| {
                events.lock().unwrap().push(event);
                signal.notify_one();
            });
        scope
            .run(async {
                let session = Session::with_http(&fixture.worker.client, false);
                assert!(session.start(&fixture.worker.client, false).await);
                // The hook does not inspect this value; it only needs the read-only request view installed in the VM.
                session
                    .trace
                    .raw
                    .set(Ok(Arc::new(chigiri::Value::Object(Default::default()))))
                    .unwrap();
                let mut hook = Box::pin(session.before(
                    &fixture.worker.client,
                    HookSlot::RequestBefore,
                    Default::default(),
                ));
                tokio::select! {
                    _ = &mut hook => panic!("VM must remain blocked at the log boundary"),
                    _ = entered.notified() => {}
                }
                session.fail(Failure::Rejected(
                    200,
                    chigiri::GraphQlError::new("rejected"),
                ));
                drop(hook);
                if disconnect {
                    drop(session);
                    assert!(terminals.lock().unwrap().is_empty());
                    release.send(()).unwrap();
                } else {
                    let mut response = Box::pin(
                        session.finish(chigiri::GraphQlResponse::executed(chigiri::Value::Null)),
                    );
                    std::future::poll_fn(|cx| {
                        assert!(response.as_mut().poll(cx).is_pending());
                        std::task::Poll::Ready(())
                    })
                    .await;
                    release.send(()).unwrap();
                    let response = response.await;
                    assert_eq!(response.errors[0].message, "Mock execution failed");
                }
                tokio::time::timeout(Duration::from_secs(1), completed.notified())
                    .await
                    .unwrap();
            })
            .await;
        drop(scope);
        assert!(
            released.load(std::sync::atomic::Ordering::Acquire),
            "the test must release the VM barrier"
        );
        let terminals = terminals.lock().unwrap();
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].cause, "execution_failed");
        assert_eq!(terminals[0].transaction, "rolled_back");
        let failures = failures.lock().unwrap();
        assert_eq!(failures.len(), 1, "exactly one future must fail");
        assert_eq!(failures[0].hook, Some(HookSlot::RequestBefore));
        assert_eq!(failures[0].phase, "runtime");
        assert_eq!(failures[0].body, (1, 1));
        assert!(
            failures[0].stack.is_none(),
            "a captured cancellation stack is not an independent error"
        );
    }
    fixture.stop().await;
}
