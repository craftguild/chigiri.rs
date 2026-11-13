//! Apply SDK shutdown within the original native HTTP cutoff.

use std::time::Instant;

use chigiri::{Endpoint, ShutdownOptions, ShutdownOutcome};

use super::shutdown::Shutdown;
use crate::diagnostic::Diagnostic;
use crate::service::Client;

pub(super) async fn endpoint(
    endpoint: Endpoint<Client>,
    shutdown: Shutdown,
) -> Result<(), Diagnostic> {
    let deadline = super::connection::http_deadline(shutdown.wait().await);
    let remaining = deadline.saturating_duration_since(Instant::now());
    let options =
        ShutdownOptions::graceful_for(remaining).unwrap_or_else(|_| ShutdownOptions::immediate());
    endpoint.shutdown_on(async {}, options).await;
    if !shutdown.finish_events(deadline).await {
        return Err(Diagnostic::environment(
            "consumer drain exceeded shutdown budget",
        ));
    }
    match endpoint.shutdown_outcome() {
        Some(ShutdownOutcome::Drained) if Instant::now() < deadline => Ok(()),
        Some(ShutdownOutcome::InfrastructureFailure) => Err(Diagnostic::environment(
            "managed GraphQL supervision failed",
        )),
        Some(ShutdownOutcome::CancellationRequested) if Instant::now() < deadline => Err(
            Diagnostic::environment("managed GraphQL cancelled before drain completion"),
        ),
        _ => Err(Diagnostic::environment(
            "managed GraphQL drain exceeded HTTP shutdown budget",
        )),
    }
}
