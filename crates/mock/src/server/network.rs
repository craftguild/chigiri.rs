//! Bound native connections while the SDK owns GraphQL supervision.

use std::time::{Duration, Instant};

use chigiri::Endpoint;
use tokio::task::JoinSet;

use super::routing::Routing;
use super::shutdown::Shutdown;
use crate::service::Client;

mod recovery;
use crate::diagnostic::Diagnostic;

pub(super) async fn run(
    listener: tokio::net::TcpListener,
    routing: Routing,
    endpoint: Endpoint<Client>,
    shutdown: Shutdown,
    interrupt: chigiri_host::interrupt::Interrupt,
    maximum_connections: usize,
) -> Result<(), Diagnostic> {
    let mut connections = JoinSet::new();
    let observed = endpoint.clone();
    let mut sdk = Box::pin(super::drain::endpoint(endpoint, shutdown.clone()));
    let mut sdk_done = false;
    let mut accepting = true;
    let mut result = Ok(());
    let mut deadline: Option<Instant> = None;
    let mut signal = Box::pin(async move {
        while !interrupt.requested()
            && observed.shutdown_outcome() != Some(chigiri::ShutdownOutcome::InfrastructureFailure)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let mut signalled = false;
    let completed = recovery::contain(async {
        loop {
            if !accepting && connections.is_empty() && sdk_done {
                break;
            }
            tokio::select! {
                biased;
                _ = &mut signal, if !signalled => {
                    signalled = true;
                    shutdown.begin();
                }
                stopped = &mut sdk, if !sdk_done => {
                    sdk_done = true;
                    accepting = false;
                    deadline = Some(super::connection::http_deadline(shutdown.begin_native()));
                    if let Err(error) = stopped {
                        result = Err(error);
                    }
                }
                stop = shutdown.wait(), if deadline.is_none() => {
                    // SDK notification is polled first. Native admission may
                    // remain open during its drain, within this original cutoff.
                    deadline = Some(super::connection::http_deadline(stop));
                }
                accepted = listener.accept(), if accepting && connections.len() < maximum_connections => {
                    match accepted {
                        Ok((stream, peer)) => {
                            connections.spawn(super::connection::run(
                                stream, peer, routing.clone(), shutdown.clone(),
                            ));
                        }
                        Err(_) => {
                            result = Err(Diagnostic::environment("HTTP listener failed"));
                            shutdown.begin();
                        }
                    }
                }
                joined = connections.join_next(), if !connections.is_empty() => {
                    if joined.is_some_and(|joined| joined.is_err()) {
                        result = Err(Diagnostic::environment("HTTP connection task failed"));
                        shutdown.begin();
                    }
                }
                _ = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                        None => std::future::pending().await,
                    }
                } => {
                    result = Err(Diagnostic::environment("HTTP drain exceeded shutdown budget"));
                    break;
                }
            }
        }
    })
    .await;
    let deadline = shutdown.begin();
    if !completed {
        result = Err(Diagnostic::environment("HTTP network loop failed"));
    }
    if !sdk_done {
        // Poll the same SDK hook through its original remaining deadline,
        // including network unwind. Never install another cleanup interval.
        if let Err(error) = sdk.await {
            result = Err(error);
        }
    }
    shutdown.begin_native();
    // Abort is only a request. Join native future destruction before the VM
    // and writer retire; the SDK retains any unjoined GraphQL physical owners.
    if !recovery::join(&mut connections).await {
        result = Err(Diagnostic::environment("HTTP connection cleanup failed"));
    }
    drop(routing);
    if Instant::now() >= super::connection::http_deadline(deadline) {
        result = Err(Diagnostic::environment(
            "HTTP drain exceeded shutdown budget",
        ));
    }
    result
}

pub(super) fn connection_capacity(
    endpoint: &chigiri::Endpoint<chigiri_mock::service::Client>,
) -> Result<usize, Diagnostic> {
    let configuration = endpoint.configuration();
    checked_capacity(
        configuration
            .subscription_limits()
            .effective()
            .max_active_subscriptions,
        configuration
            .subscription_protocol()
            .effective()
            .max_websocket_connections,
    )
}

fn checked_capacity(active: usize, websocket: usize) -> Result<usize, Diagnostic> {
    chigiri_mock::limits::HTTP_REQUESTS
        .checked_add(active)
        .and_then(|count| count.checked_add(websocket))
        .ok_or_else(|| Diagnostic::environment("connection bookkeeping limit overflow"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn connection_inventory_preserves_finite_capacity_beside_both_stream_quotas() {
        assert_eq!(super::checked_capacity(128, 64).unwrap(), 209);
        assert!(super::checked_capacity(usize::MAX, 1).is_err());
        assert!(super::checked_capacity(1, usize::MAX).is_err());
    }
}
