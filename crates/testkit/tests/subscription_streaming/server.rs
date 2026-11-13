use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[path = "server/http.rs"]
mod http;
#[path = "server/outcome.rs"]
mod outcome;
#[path = "server/websocket.rs"]
mod websocket;

use outcome::ExpectedOutcome;

#[derive(Clone, Copy)]
pub enum Adapter {
    Hyper,
    Axum,
}

pub struct RunningServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<(), String>>>,
    connection_result: mpsc::Receiver<Result<(), String>>,
}

impl RunningServer {
    pub fn start(adapter: Adapter, endpoint: chigiri::Endpoint<()>) -> Self {
        Self::start_with(adapter, endpoint, false, ExpectedOutcome::Complete)
    }

    pub fn start_disconnect(adapter: Adapter, endpoint: chigiri::Endpoint<()>) -> Self {
        Self::start_with(adapter, endpoint, false, ExpectedOutcome::PeerDisconnect)
    }

    pub fn start_pressure(adapter: Adapter, endpoint: chigiri::Endpoint<()>) -> Self {
        Self::start_with(adapter, endpoint, false, ExpectedOutcome::PolicyTerminal)
    }

    pub fn start_websocket(adapter: Adapter, endpoint: chigiri::Endpoint<()>) -> Self {
        Self::start_with(adapter, endpoint, true, ExpectedOutcome::Complete)
    }

    fn start_with(
        adapter: Adapter,
        endpoint: chigiri::Endpoint<()>,
        websocket: bool,
        expected: ExpectedOutcome,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (started, address) = mpsc::sync_channel(1);
        let (completed, connection_result) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("chigiri-loopback-server".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime.block_on(async move {
                    match (adapter, websocket) {
                        (Adapter::Hyper, false) => {
                            http::run_hyper(endpoint, thread_stop, started, expected, completed)
                                .await
                        }
                        (Adapter::Axum, false) => {
                            http::run_axum(endpoint, thread_stop, started, expected, completed)
                                .await
                        }
                        (Adapter::Hyper, true) => {
                            websocket::run_hyper(endpoint, thread_stop, started).await
                        }
                        (Adapter::Axum, true) => {
                            websocket::run_axum(endpoint, thread_stop, started).await
                        }
                    }
                })
            })
            .expect("spawn owned loopback runtime");
        let address = match address.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(address)) => address,
            Ok(Err(error)) => {
                let _ = thread.join();
                panic!("construct loopback service: {error}");
            }
            Err(error) => {
                stop.store(true, Ordering::Release);
                let _ = thread.join();
                panic!("loopback server startup: {error}");
            }
        };
        Self {
            address,
            stop,
            thread: Some(thread),
            connection_result,
        }
    }

    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn wait_for_connection(&self) {
        self.connection_result
            .recv_timeout(Duration::from_secs(5))
            .expect("owned connection outcome before client disconnect")
            .expect("observed the expected connection outcome");
    }

    pub fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        let result = self
            .thread
            .take()
            .expect("owned server thread")
            .join()
            .expect("loopback server thread did not panic");
        result.expect("loopback server drained");
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn bind() -> Result<(tokio::net::TcpListener, SocketAddr), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    Ok((listener, address))
}

fn announce<T>(
    setup: Result<(T, tokio::net::TcpListener, SocketAddr), String>,
    started: mpsc::SyncSender<Result<SocketAddr, String>>,
) -> Result<(T, tokio::net::TcpListener), String> {
    match setup {
        Ok((owners, listener, address)) => {
            started
                .send(Ok(address))
                .map_err(|_| "loopback startup receiver closed".to_owned())?;
            Ok((owners, listener))
        }
        Err(error) => {
            let _ = started.send(Err(error.clone()));
            Err(error)
        }
    }
}

async fn accept(
    listener: &tokio::net::TcpListener,
) -> Result<(tokio::net::TcpStream, SocketAddr), String> {
    tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .map_err(|_| "loopback accept timed out".to_owned())?
        .map_err(|error| error.to_string())
}

async fn wait_for_stop(stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn join_connection(
    mut connection: tokio::task::JoinHandle<Result<(), String>>,
) -> Result<(), String> {
    match tokio::time::timeout(Duration::from_secs(2), &mut connection).await {
        Ok(result) => result.map_err(debug_error).and_then(|result| result),
        Err(_) => {
            connection.abort();
            let _ = connection.await;
            Err("owned loopback connection did not drain before timeout".to_owned())
        }
    }
}

fn debug_error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

async fn scoped_stop(endpoint: &chigiri::Endpoint<()>) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        endpoint.shutdown_on(async {}, chigiri::ShutdownOptions::graceful()),
    )
    .await
    .map_err(|_| "scoped SDK drain exceeded the fixture watchdog".to_owned())?;
    match endpoint.shutdown_outcome() {
        Some(chigiri::ShutdownOutcome::Drained) => Ok(()),
        Some(outcome) => Err(format!("scoped SDK drain selected {outcome:?}")),
        None => Err("scoped SDK stop did not select an outcome".to_owned()),
    }
}
