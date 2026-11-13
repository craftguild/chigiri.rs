//! Starts the Books endpoint with application-owned routing and shutdown.

mod cli;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use chigiri_books::AppContext;
use chigiri_hyper::HyperService;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;

fn main() {
    let address = match cli::parse(std::env::args_os()) {
        Ok(Some(address)) => address,
        Ok(None) => return,
        Err(()) => std::process::exit(2),
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| "cannot create books runtime".into())
        .and_then(|runtime| runtime.block_on(serve(address)));
    if result.is_err() {
        eprintln!("books: server startup or connection failed");
        std::process::exit(1);
    }
}

async fn serve(address: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = chigiri_books::endpoint(chigiri_books::Catalog::seeded())?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let in_flight = Arc::new(AtomicUsize::new(0));
    eprintln!("books GraphQL server: http://{address}/graphql");

    let mut connections = tokio::task::JoinSet::new();
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            result = &mut shutdown => {
                result?;
                break;
            }
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let service = ApplicationService {
                    inner: chigiri_hyper::service(endpoint.clone()),
                    endpoint: endpoint.clone(),
                    in_flight: Arc::clone(&in_flight),
                    peer,
                };
                connections.spawn(async move {
                    http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(stream), service)
                        .await
                });
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                result??;
            }
        }
    }
    while let Some(result) = connections.join_next().await {
        result??;
    }
    Ok(())
}

struct ApplicationService {
    inner: HyperService<AppContext>,
    endpoint: chigiri::Endpoint<AppContext>,
    in_flight: Arc<AtomicUsize>,
    peer: SocketAddr,
}

impl Service<Request<Incoming>> for ApplicationService {
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = chigiri_http::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, mut request: Request<Incoming>) -> Self::Future {
        let request_guard = ApplicationRequest::start(Arc::clone(&self.in_flight));
        if request.uri().path() == "/readiness" {
            let application = self.in_flight.load(Ordering::Acquire).saturating_sub(1);
            let endpoint = self.endpoint.in_flight_requests();
            let body = format!(
                "{{\"applicationInFlight\":{application},\"endpointInFlight\":{endpoint}}}"
            );
            return Box::pin(async move {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Full::new(Bytes::from(body)))
                    .expect("fixed readiness response must be valid");
                drop(request_guard);
                Ok(response)
            });
        }
        if request.uri().path() != "/graphql" {
            return Box::pin(async move {
                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::new()))
                    .expect("fixed 404 response must be valid");
                drop(request_guard);
                Ok(response)
            });
        }
        request
            .extensions_mut()
            .insert(chigiri_hyper::PeerAddr(self.peer));
        let response = self.inner.call(request);
        Box::pin(async move {
            let response = response.await;
            drop(request_guard);
            response
        })
    }
}

struct ApplicationRequest(Arc<AtomicUsize>);

impl ApplicationRequest {
    fn start(in_flight: Arc<AtomicUsize>) -> Self {
        in_flight.fetch_add(1, Ordering::AcqRel);
        Self(in_flight)
    }
}

impl Drop for ApplicationRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
