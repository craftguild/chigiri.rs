//! Physical source disposal cannot precede the outside terminal decision.

use super::*;
use crate::fixture::SourceFailure;
use chigiri_runtime::__private::{SubscriptionEventResult, SubscriptionSourceState};

#[test]
fn observed_source_end_and_source_failure_retain_physical_source_until_outside_finish() {
    for failed in [false, true] {
        let (schema, mut inner, calls) = fixture();
        inner.events = if failed {
            vec![Err(SourceFailure)]
        } else {
            Vec::new()
        };
        let state = Arc::<State>::default();
        let schema = schema
            .bind_with_subscriptions(Observed {
                inner,
                state: state.clone(),
                omit_receipt: false,
            })
            .unwrap()
            .into_executable();
        let mut prepared = schema.prepare_subscription(request()).unwrap();
        let mut owner = prepared
            .prepare_host_observation(&AppContext(0), 1024)
            .unwrap()
            .unwrap();
        assert!(owner.accept());
        let SubscriptionStart::Ready(mut session) = block_on(prepared.start(&AppContext(0), None))
        else {
            panic!("observed source must start")
        };
        let output = block_on(session.next_source(None));
        let decision = if failed {
            let SubscriptionSourceState::Terminal(response) = output else {
                panic!("source must fail")
            };
            assert_eq!(
                response.decision(),
                SubscriptionTermination::ExecutionFailed
            );
            SubscriptionTermination::ExecutionFailed
        } else {
            assert!(matches!(output, SubscriptionSourceState::Complete));
            SubscriptionTermination::SourceComplete
        };
        assert_eq!(calls.source_drops.load(Ordering::SeqCst), 0);
        owner.finish(decision, 0);
        assert_eq!(*state.terminals.lock().unwrap(), [decision]);
        assert_eq!(calls.source_drops.load(Ordering::SeqCst), 0);
        drop(session);
        assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            calls.source_maps.load(Ordering::SeqCst),
            usize::from(failed)
        );
    }
}

#[test]
fn observed_typed_invariant_and_host_execution_failure_retain_source_without_text_inference() {
    for invariant in [false, true] {
        let (schema, mut inner, calls) = fixture();
        inner.host_failure = !invariant;
        let state = Arc::<State>::default();
        let schema = schema
            .bind_with_subscriptions(Observed {
                inner,
                state: state.clone(),
                omit_receipt: false,
            })
            .unwrap()
            .into_executable();
        let mut prepared = schema.prepare_subscription(request()).unwrap();
        let mut owner = prepared
            .prepare_host_observation(&AppContext(0), 1024)
            .unwrap()
            .unwrap();
        assert!(owner.accept());
        let SubscriptionStart::Ready(mut session) = block_on(prepared.start(&AppContext(0), None))
        else {
            panic!("observed source must start")
        };
        if !invariant {
            assert!(matches!(
                block_on(session.next_source(None)),
                SubscriptionSourceState::EventReady
            ));
        }
        let SubscriptionEventResult::Terminal(response) =
            block_on(session.execute_event(&AppContext(77), None, None))
        else {
            panic!("typed fatal failure must terminate")
        };
        let decision = if invariant {
            SubscriptionTermination::Internal
        } else {
            SubscriptionTermination::ExecutionFailed
        };
        assert_eq!(response.decision(), decision);
        assert_eq!(response.errors[0].message, "Internal server error");
        assert_eq!(calls.source_drops.load(Ordering::SeqCst), 0);
        owner.finish(decision, 0);
        drop(session);
        assert_eq!(calls.source_drops.load(Ordering::SeqCst), 1);
        assert_eq!(*state.terminals.lock().unwrap(), [decision]);
    }
}
