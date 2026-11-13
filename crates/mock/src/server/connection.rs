//! Own the native HTTP exchange while GraphQL sockets remain SDK-owned.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use hyper_util::{rt::TokioIo, service::TowerToHyperService};

use super::routing::Routing;
use super::shutdown::Shutdown;

pub(super) fn http_deadline(deadline: Instant) -> Instant {
    // Preserve the existing half-second recovery reserve for the VM and writer.
    deadline
        .checked_sub(Duration::from_millis(500))
        .unwrap_or(deadline)
}

pub(super) async fn run(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    routing: Routing,
    shutdown: Shutdown,
) {
    let service = routing.connection(peer);
    drop(routing);
    let builder = hyper::server::conn::http1::Builder::new();
    let http = builder
        .serve_connection(TokioIo::new(stream), TowerToHyperService::new(service))
        .with_upgrades();
    tokio::pin!(http);
    let mut deadline: Option<Instant> = None;
    loop {
        tokio::select! {
            _ = &mut http => break,
            stop = shutdown.wait_native(), if deadline.is_none() => {
                deadline = Some(http_deadline(stop));
                http.as_mut().graceful_shutdown();
            }
            _ = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending().await,
                }
            } => break,
        }
    }
    // Drop native I/O here; the SDK retains its own original charged upgrade
    // and protocol session independently of this HTTP connection future.
}

#[cfg(test)]
mod tests;
