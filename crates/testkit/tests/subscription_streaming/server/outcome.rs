//! Classify original server errors without trusting source Drop as their cause.

use std::error::Error;
use std::io::ErrorKind;
use std::sync::mpsc::SyncSender;

use chigiri::__streaming::{HttpStreamingError, SubscriptionOutboxSignal};

#[derive(Clone, Copy, Debug)]
pub(super) enum ExpectedOutcome {
    Complete,
    PeerDisconnect,
    PolicyTerminal,
}

#[derive(Debug)]
enum ObservedOutcome {
    Complete,
    PeerDisconnect,
    SendStalled,
    Failure(String),
}

pub(super) fn report(
    expected: ExpectedOutcome,
    result: Result<(), hyper::Error>,
    completed: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let result = require(expected, observe(result));
    completed
        .try_send(result.clone())
        .expect("one owned connection report");
    result
}

fn observe(result: Result<(), hyper::Error>) -> ObservedOutcome {
    let Err(error) = result else {
        return ObservedOutcome::Complete;
    };
    let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
    let mut peer_disconnect = error.is_incomplete_message() || error.is_closed();
    while let Some(source) = cause {
        if let Some(streaming) = source.downcast_ref::<HttpStreamingError>() {
            return if *streaming
                == HttpStreamingError::Delivery(SubscriptionOutboxSignal::SendStalled)
            {
                ObservedOutcome::SendStalled
            } else {
                ObservedOutcome::Failure(format!("unexpected common body failure: {streaming:?}"))
            };
        }
        if let Some(io) = source.downcast_ref::<std::io::Error>() {
            peer_disconnect |= matches!(
                io.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
                    | ErrorKind::UnexpectedEof
            );
        }
        cause = source.source();
    }
    if peer_disconnect && !error.is_user() {
        ObservedOutcome::PeerDisconnect
    } else {
        ObservedOutcome::Failure(format!("unexpected HTTP connection failure: {error:?}"))
    }
}

fn require(expected: ExpectedOutcome, observed: ObservedOutcome) -> Result<(), String> {
    match (expected, observed) {
        (ExpectedOutcome::Complete, ObservedOutcome::Complete)
        | (
            ExpectedOutcome::PeerDisconnect,
            ObservedOutcome::Complete | ObservedOutcome::PeerDisconnect,
        )
        // The client independently requires an encoded policy terminal before
        // dropping its socket. A successful connection alone proves no policy.
        | (ExpectedOutcome::PolicyTerminal, ObservedOutcome::Complete) => Ok(()),
        (_, ObservedOutcome::Failure(message)) => Err(message),
        (expected, observed) => Err(format!("expected {expected:?}, observed {observed:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_connection_failures_never_count_as_pressure_or_completion() {
        for expected in [
            ExpectedOutcome::Complete,
            ExpectedOutcome::PeerDisconnect,
            ExpectedOutcome::PolicyTerminal,
        ] {
            assert!(
                require(
                    expected,
                    ObservedOutcome::Failure("injected body/service failure".to_owned())
                )
                .is_err()
            );
        }
        assert!(
            require(
                ExpectedOutcome::PolicyTerminal,
                ObservedOutcome::PeerDisconnect
            )
            .is_err()
        );
        assert!(
            require(
                ExpectedOutcome::PolicyTerminal,
                ObservedOutcome::SendStalled
            )
            .is_err()
        );
        assert!(require(ExpectedOutcome::Complete, ObservedOutcome::SendStalled).is_err());
    }
}
