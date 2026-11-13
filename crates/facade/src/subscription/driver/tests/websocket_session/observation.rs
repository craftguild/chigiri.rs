//! Actual session callers preserve internal, transport and explicit decisions.

use chigiri_runtime::__private::SubscriptionTermination;

use super::super::observation::install;
use super::super::state::{SourceStep, State};
use super::support::{Fixture, SUBSCRIPTION};
use crate::__streaming::{WebSocketInbound, WebSocketIoError};
use crate::SubscriptionLimits;

#[test]
fn websocket_session_terminal_callers_preserve_typed_winners_and_frozen_drop_totals() {
    for (caller, expected) in [
        (0, SubscriptionTermination::Internal),
        (1, SubscriptionTermination::TransportClosed),
        (2, SubscriptionTermination::TransportClosed),
        (3, SubscriptionTermination::TransportClosed),
        (4, SubscriptionTermination::Internal),
        (5, SubscriptionTermination::Internal),
        (6, SubscriptionTermination::ServerShutdown),
        (7, SubscriptionTermination::ClientCancelled),
        (8, SubscriptionTermination::TransportClosed),
    ] {
        let mut source: Vec<_> = (1..=5).map(SourceStep::Event).collect();
        source.push(SourceStep::Pending);
        let probe = State::new(source);
        let host = install(&probe);
        let mut fixture = Fixture::with_limits(
            probe.clone(),
            SubscriptionLimits {
                max_queued_events: 2,
                ..SubscriptionLimits::default()
            },
        );
        fixture.socket.0.lock().unwrap().ready = false;
        fixture.socket.subscribe("observed", SUBSCRIPTION);
        fixture.until_source_pending(&probe);
        assert_eq!(*host.drops.lock().unwrap(), [1, 2, 3]);
        assert!(host.terminals.lock().unwrap().is_empty());
        match caller {
            0 => drop(fixture.driver.take()),
            1 => fixture.socket.0.lock().unwrap().eof = true,
            2 => fixture
                .socket
                .0
                .lock()
                .unwrap()
                .input
                .push_back(WebSocketInbound::Binary),
            3 => fixture.socket.0.lock().unwrap().input_error = Some(WebSocketIoError::Transport),
            4 => fixture.socket.0.lock().unwrap().input_error = Some(WebSocketIoError::Invariant),
            5 => fixture.driver.as_mut().unwrap().fail_clock_for_test(),
            6 => fixture.driver.as_mut().unwrap().shutdown(),
            7 => fixture
                .socket
                .text(r#"{"id":"observed","type":"complete"}"#),
            8 => fixture.driver.as_mut().unwrap().transport_closed(),
            _ => unreachable!(),
        }
        if caller == 7 {
            fixture.until(|_| !host.terminals.lock().unwrap().is_empty());
        } else if caller != 0 {
            fixture.until_closed();
        }
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [expected],
            "caller {caller}"
        );
        assert_eq!(*host.totals.lock().unwrap(), [3]);
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        fixture.finish();
        assert_eq!(*host.terminals.lock().unwrap(), [expected]);
        assert_eq!(*host.totals.lock().unwrap(), [3]);
        assert_eq!(*host.drops.lock().unwrap(), [1, 2, 3]);
    }
}
