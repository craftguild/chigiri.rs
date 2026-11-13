//! Exercise Mock lifecycle entry and registration through the monitored worker path.

use chigiri::__streaming::PreparedHttpSubscription;

use super::support::Fixture;

const QUERY: &str = "subscription { changed(topic:\"events\") { id } }";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_setup_uses_the_genuine_private_generation_and_policy() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let (prepared, guard) = fixture.prepare_monitored(QUERY).await;
    match &prepared {
        PreparedHttpSubscription::Ready(_) => {}
        PreparedHttpSubscription::Response(response) => panic!(
            "expected ready setup, got response {}; reports {}; diagnostics {}: {}",
            response.status(),
            fixture.reports.load(std::sync::atomic::Ordering::SeqCst),
            fixture.resolver_diagnostics.lock().unwrap().len(),
            String::from_utf8_lossy(response.as_ref())
        ),
        PreparedHttpSubscription::Failed(error) => panic!("setup failed closed: {error}"),
    }
    let records = fixture.take_subscription_logs();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["kind"], "subscription_setup");
    assert_eq!(records[0]["coordinate"], "Signals.changed");
    assert_eq!(records[0]["outcome"], "registered");
    assert_eq!(records[0]["overflow"], "backpressure");
    assert_eq!(records[0]["subscription_id"], 1);
    assert!(records[0]["request_id"].as_u64().is_some());
    drop(prepared);
    guard.shutdown().unwrap().await.unwrap();
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn entered_setup_failure_has_no_generation_or_policy() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::with_subscription_body("error('private setup failure')");
    let (prepared, guard) = fixture.prepare_monitored(QUERY).await;
    match &prepared {
        PreparedHttpSubscription::Response(_) => {}
        PreparedHttpSubscription::Ready(_) => panic!("failing setup became ready"),
        PreparedHttpSubscription::Failed(error) => panic!("setup failed closed: {error}"),
    }
    let records = fixture.take_subscription_logs();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["kind"], "subscription_setup");
    assert_eq!(records[0]["outcome"], "failed");
    assert!(records[0]["subscription_id"].is_null());
    assert!(records[0]["overflow"].is_null());
    let text = records[0].to_string();
    assert!(!text.contains("private setup failure"));
    drop(prepared);
    guard.shutdown().unwrap().await.unwrap();
    fixture.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validation_rejection_before_entry_has_no_lifecycle_record() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let (prepared, guard) = fixture
        .prepare_monitored("subscription { unavailable }")
        .await;
    match &prepared {
        PreparedHttpSubscription::Response(_) => {}
        PreparedHttpSubscription::Ready(_) => panic!("invalid field became ready"),
        PreparedHttpSubscription::Failed(error) => panic!("validation failed closed: {error}"),
    }
    assert!(fixture.take_subscription_logs().is_empty());
    drop(prepared);
    guard.shutdown().unwrap().await.unwrap();
    fixture.stop().await;
}
