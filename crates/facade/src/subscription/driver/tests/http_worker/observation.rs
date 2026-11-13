//! The original HTTP source task waits for the outside setup authority.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::super::observation::install;
use super::super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State};
use super::support::{VALID, drain, endpoint, pending, profiles, run, runtime};
use crate::transport::streaming::PreparedHttpSubscription;

#[test]
fn http_source_entry_follows_outside_handoff_and_context_rejection_has_no_owner() {
    let runtime = runtime();
    for profile in profiles() {
        for rejected in [false, true] {
            let probe = State::modes(
                vec![SourceStep::Pending],
                SetupMode::Ready,
                EventMode::Object,
                if rejected {
                    FactoryMode::Reject
                } else {
                    FactoryMode::Ready
                },
                FactoryMode::Ready,
                HookMode::Ready,
            );
            let host = install(&probe);
            let (endpoint, _) = endpoint(Arc::clone(&probe));
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let request = run(&runtime, pending(endpoint, profile, VALID, false));
            let prepared = run(&runtime, request.prepare());
            if rejected {
                let PreparedHttpSubscription::Response(response) = prepared else {
                    panic!("context must reject")
                };
                assert_eq!(response.status(), 403);
                assert_eq!(host.preparations.load(Ordering::SeqCst), 0);
                assert_eq!(host.entries.load(Ordering::SeqCst), 0);
                assert!(host.terminals.lock().unwrap().is_empty());
                drop(response);
            } else {
                assert!(matches!(&prepared, PreparedHttpSubscription::Ready(_)));
                assert_eq!(host.preparations.load(Ordering::SeqCst), 1);
                assert_eq!(host.entries.load(Ordering::SeqCst), 1);
                drop(prepared);
            }
            drain(&runtime, guard);
            assert_eq!(host.terminals.lock().unwrap().len(), usize::from(!rejected));
        }
    }
}
