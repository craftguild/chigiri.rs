//! Emit elapsed-time warnings while awaiting ordinary futures without VM or host calls.

use super::*;
use crate::limits::request::{LimitKind, policy};

#[tokio::test]
async fn elapsed_soft_limit_is_observed_when_the_future_completes_after_the_threshold() {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let received = notices.clone();
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
        .with_limit_warnings(move |_, notice| received.lock().unwrap().push(notice));
    let result = scope
        .run(async {
            // Block synchronously so the timer cannot be polled, then yield on the future side.
            std::thread::sleep(Duration::from_millis(3800));
            42
        })
        .await;
    assert_eq!(result, 42);
    assert!(scope.0.execution_budget.failure().is_none());
    let notices = notices.lock().unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, LimitKind::ElapsedMs);
    assert_eq!(notices[0].origin.phase, "request");
    assert!(notices[0].origin.coordinate.is_none());
    assert!(notices[0].origin.path.is_empty());
    assert!(notices[0].observed >= policy(LimitKind::ElapsedMs).soft);
}

#[tokio::test]
async fn elapsed_soft_limit_notifies_once_while_the_future_is_pending() {
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = Mutex::new(Some(send));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let received = notices.clone();
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap()).with_limit_warnings(
        move |id, notice| {
            received.lock().unwrap().push((id, notice));
            if let Some(send) = send.lock().unwrap().take() {
                let _ = send.send(());
            }
        },
    );
    let deadline = scope.0.deadline;
    let result = tokio::time::timeout(
        Duration::from_secs(6),
        scope.run(async {
            receive.await.unwrap();
            42
        }),
    )
    .await
    .unwrap();
    assert_eq!(result, 42);
    assert_eq!(scope.0.deadline, deadline);
    assert!(scope.0.execution_budget.failure().is_none());
    let notices = notices.lock().unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].0, scope.request_id());
    assert_eq!(notices[0].1.kind, LimitKind::ElapsedMs);
    assert_eq!(notices[0].1.origin.phase, "request");
    assert!(notices[0].1.origin.coordinate.is_none());
    assert!(notices[0].1.origin.path.is_empty());
    let policy = policy(LimitKind::ElapsedMs);
    assert!((policy.soft..policy.hard).contains(&notices[0].1.observed));
}
