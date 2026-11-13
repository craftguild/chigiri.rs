//! Verify bounded queue admission, dropped-future cancellation, and production endpoint limits.

use super::*;
use chigiri::core::Object;
use std::future::Future;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

#[tokio::test]
async fn rsm_17_queue_accepts_16_rejects_17_without_wait_and_drops_cancel_jobs() {
    let _test_case = crate::vm_gate::test_case();
    let ir = super::super::tests::ir("type Query { value: Int! @chigiri_mock_resolver }");
    let id = ir.scripts()[0].field;
    let (client, receiver) = queue(ir);
    let mut context = Context::from_waker(Waker::noop());
    let mut pending = Vec::new();
    for _ in 0..16 {
        let mut future = Box::pin(client.execute(id, Object::new(), None));
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        pending.push(future);
    }
    assert_eq!(receiver.len(), 16);
    let mut overflow = Box::pin(client.execute(id, Object::new(), None));
    assert!(matches!(
        overflow.as_mut().poll(&mut context),
        Poll::Ready(Err(_))
    ));
    assert_eq!(receiver.len(), 16);
    drop(pending);
    let jobs: Vec<_> = receiver.try_iter().collect();
    assert_eq!(jobs.len(), 16);
    assert!(jobs.iter().all(|job| job.cancel.cancelled()));
    let mut next = Box::pin(client.execute(id, Object::new(), None));
    assert!(matches!(next.as_mut().poll(&mut context), Poll::Pending));
    drop(next);
    assert!(receiver.try_recv().unwrap().cancel.cancelled());
}

#[test]
fn rsm_20_endpoint_keeps_production_limits_and_five_second_deadlines() {
    let _test_case = crate::vm_gate::test_case();
    let ir = super::super::tests::ir("type Query { value: Int! @chigiri_mock_resolver }");
    let (client, _receiver) = queue(ir);
    let endpoint = super::super::tests::bound(client);
    let config = endpoint.configuration();
    assert_eq!(*config.body_limit().effective(), 1024 * 1024);
    assert_eq!(*config.response_byte_limit().effective(), 8 * 1024 * 1024);
    assert_eq!(
        *config.execution_limits().effective(),
        chigiri::ExecutionLimits::production()
    );
    assert_eq!(
        config.execution_limits().effective().max_output_nodes,
        100_000
    );
    assert_eq!(
        config.execution_limits().effective().max_operation_depth,
        32
    );
    assert_eq!(
        *config.overall_timeout().effective(),
        Duration::from_secs(5)
    );
    assert_eq!(
        *config.execution_timeout().effective(),
        Duration::from_secs(5)
    );
    assert_eq!(
        endpoint.schema_info().source_kind(),
        chigiri::SchemaSourceKind::Owned
    );
    assert_eq!(endpoint.schema_info().generated_abi(), None);
}
