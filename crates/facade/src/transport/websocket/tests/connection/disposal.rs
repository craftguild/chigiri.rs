//! Endpoint-alive abandonment must release disposed connection metadata.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use super::super::support::ManualDeadlines;
use super::support::{Authorizer, Decision, admitted, endpoint, initialization, runtime};
use crate::transport::websocket::{
    WebSocketAcknowledgementResolution, WebSocketConnectionResolution,
};

#[test]
fn detached_connection_cleanup_covers_every_phase_and_preserves_the_physical_ack() {
    for phase in 0..4 {
        let runtime = runtime();
        let endpoint = endpoint(
            Authorizer {
                calls: Arc::new(AtomicUsize::new(0)),
                decision: Decision::Accept,
            },
            1,
            16 * 1024,
        );
        let guard = endpoint.__register_subscription_runtime().unwrap();
        let deadlines = Box::leak(Box::new(ManualDeadlines::default()));
        let mut held = None;
        if phase == 0 {
            let claimed = endpoint
                .__claim_websocket_upgrade(admitted(&endpoint, deadlines))
                .unwrap();
            drop(
                endpoint
                    .__begin_websocket_connection(claimed, deadlines)
                    .unwrap(),
            );
        } else {
            let mut opening = initialization(&endpoint, deadlines);
            if phase == 1 {
                drop(opening);
            } else {
                {
                    let _entered = runtime.enter();
                    opening
                        .receive_text(r#"{"type":"connection_init","payload":{}}"#)
                        .unwrap();
                }
                let WebSocketConnectionResolution::Acknowledgement(mut ack) =
                    runtime.block_on(opening)
                else {
                    panic!("fixture authorization must accept");
                };
                let (frame, progress) = ack.begin_acknowledgement().unwrap().unwrap();
                progress.written_and_flushed();
                if phase == 2 {
                    held = Some(frame);
                    drop(ack);
                } else {
                    drop(frame);
                    let WebSocketAcknowledgementResolution::Initialized(active) =
                        runtime.block_on(ack)
                    else {
                        panic!("flushed acknowledgement must initialize");
                    };
                    drop(active);
                }
            }
        }
        let cleanup = endpoint.__subscription_cleanup_driver();
        assert_eq!(cleanup.pending(), Ok(1));
        assert!(endpoint.__open_subscription_connection().is_err());
        drop(guard);
        assert_eq!(
            cleanup.pending(),
            Ok(0),
            "completed metadata needs no application reaper poll"
        );
        if phase == 2 {
            assert_eq!(
                held.as_ref().unwrap().as_bytes(),
                br#"{"type":"connection_ack"}"#
            );
            assert!(
                endpoint.__open_subscription_connection().is_err(),
                "the physical ack still retains the original C permit"
            );
        }
        drop(held);
        let replacement = endpoint
            .__open_subscription_connection()
            .expect("Endpoint stays alive after disposed metadata recovery");
        drop(replacement);
    }
}
