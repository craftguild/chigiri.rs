//! Actual aggregate abandonment preserves its typed pre-existing authority.

use std::sync::Arc;
use std::task::Poll;

use chigiri_runtime::__private::SubscriptionTermination;

use super::super::super::harness::poll_once;
use super::super::super::observation::install;
use super::super::super::state::{SourceStep, State};
use super::super::support::{drain, profiles, runtime};
use super::support::{attach, drive_until_sources_pending};

#[test]
fn http_aggregate_bare_drop_shutdown_before_poll_and_internal_fault_keep_distinct_causes() {
    let runtime = runtime();
    for profile in profiles() {
        for activated in [false, true] {
            for (fault, expected) in [
                (0, SubscriptionTermination::ServerShutdown),
                (1, SubscriptionTermination::Internal),
                (2, SubscriptionTermination::Internal),
                (3, SubscriptionTermination::ServerShutdown),
            ] {
                let mut source: Vec<_> = (1..=5).map(SourceStep::Event).collect();
                source.push(SourceStep::Pending);
                let probe = State::new(source);
                let host = install(&probe);
                let (mut live, _) = attach(&runtime, Arc::clone(&probe), profile);
                if activated {
                    drive_until_sources_pending(&runtime, &mut live.driver, &probe, 1);
                    assert_eq!(*host.drops.lock().unwrap(), [1, 2, 3]);
                }
                if fault == 0 || fault == 3 {
                    live.registration.close();
                }
                if fault == 2 || fault == 3 {
                    live.driver.as_ref().get_ref().poison_registry_for_test();
                }
                if fault == 2 {
                    let _entered = runtime.enter();
                    assert!(matches!(
                        poll_once(live.driver.as_mut()),
                        Poll::Ready(Err(_))
                    ));
                }
                drop(live.driver);
                assert_eq!(*host.terminals.lock().unwrap(), [expected]);
                assert_eq!(
                    *host.totals.lock().unwrap(),
                    [if activated { 3 } else { 0 }]
                );
                assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
                drop((live.body, live.registration));
                drain(&runtime, live.guard);
                assert_eq!(*host.terminals.lock().unwrap(), [expected]);
                assert_eq!(
                    *host.totals.lock().unwrap(),
                    [if activated { 3 } else { 0 }]
                );
            }
        }
    }
}
