//! Wire-level service-bridge tests using an application-owned Hyper listener.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use chigiri::Endpoint;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;

use crate::lifecycle_support::TestContext;

pub struct Running {
    pub address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

pub async fn spawn(endpoint: Endpoint<TestContext>) -> Running {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                break;
            };
            let endpoint = endpoint.clone();
            tokio::spawn(async move {
                let service = PeerService {
                    inner: chigiri_hyper::service(endpoint),
                    peer,
                };
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Running { address, task }
}

struct PeerService<C> {
    inner: chigiri_hyper::HyperService<C>,
    peer: SocketAddr,
}

impl<C> hyper::service::Service<hyper::Request<hyper::body::Incoming>> for PeerService<C>
where
    C: Send + Sync + 'static,
{
    type Response = <chigiri_hyper::HyperService<C> as hyper::service::Service<
        hyper::Request<hyper::body::Incoming>,
    >>::Response;
    type Error = std::convert::Infallible;
    type Future = chigiri_http::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, mut request: hyper::Request<hyper::body::Incoming>) -> Self::Future {
        request
            .extensions_mut()
            .insert(chigiri_hyper::PeerAddr(self.peer));
        self.inner.call(request)
    }
}

pub struct RawResponse {
    pub status: u16,
    pub content_type: Option<String>,
}

pub async fn request(address: SocketAddr, source: String) -> RawResponse {
    exchange(address, source).await
}

pub async fn request_without_request_body(address: SocketAddr, source: String) -> RawResponse {
    exchange(address, source).await
}

async fn exchange(address: SocketAddr, source: String) -> RawResponse {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(source.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        parse_response(&response)
    })
    .await
    .unwrap()
}

fn parse_response(response: &str) -> RawResponse {
    let head = response.split("\r\n\r\n").next().unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let content_type = lines.find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("content-type")
                .then(|| value.trim().to_owned())
        })
    });
    RawResponse {
        status,
        content_type,
    }
}

pub const JSON_HEADERS: &str =
    "Content-Type: application/json\r\nAccept: application/graphql-response+json\r\n";

pub fn post_request() -> String {
    let body = r#"{"query":"{ hello }"}"#;
    format!(
        "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n{JSON_HEADERS}\r\n{body}",
        body.len()
    )
}

pub fn get_request(preflight: bool) -> String {
    let header = if preflight {
        "X-Chigiri-Preflight: 1\r\n"
    } else {
        ""
    };
    format!(
        "GET /graphql?query=%7Bhello%7D HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAccept: application/graphql-response+json\r\n{header}\r\n"
    )
}
