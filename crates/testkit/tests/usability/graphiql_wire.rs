//! Test both optional helpers over real HTTP with application-owned listeners and authentication.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::Request,
    middleware::{Next, from_fn},
    response::Response,
};
use bytes::Bytes;
use chigiri::{Endpoint, graphiql::Paths};
use http_body_util::Full;
use hyper::{
    StatusCode,
    server::conn::http1,
    service::{Service, service_fn},
};
use hyper_util::rt::TokioIo;

#[derive(Clone, Copy, Debug)]
pub(super) enum Bridge {
    Axum,
    Hyper,
}

pub(super) struct Running {
    address: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    pub(super) async fn spawn(
        endpoint: Endpoint<()>,
        paths: Paths,
        bridge: Bridge,
        protected: bool,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, mut receive) = tokio::sync::oneshot::channel();
        let task = match bridge {
            Bridge::Axum => {
                let mut router: Router = chigiri_axum::graphiql::routes(endpoint, paths);
                if protected {
                    router = router.layer(from_fn(authenticate));
                }
                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = receive.await;
                        })
                        .await
                        .unwrap();
                })
            }
            Bridge::Hyper => tokio::spawn(async move {
                let service = chigiri_hyper::graphiql::service(endpoint, paths);
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        _ = &mut receive => break,
                        Some(result) = connections.join_next() => { result.unwrap(); }
                        connection = listener.accept() => {
                            let (stream, _) = connection.unwrap();
                            let service = service.clone();
                            connections.spawn(async move {
                                let outer = service_fn(move |request| {
                                    let service = service.clone();
                                    async move {
                                        if protected && !authorized(&request) {
                                            let mut response = hyper::Response::new(Full::new(Bytes::new()));
                                            *response.status_mut() = StatusCode::UNAUTHORIZED;
                                            return Ok(response);
                                        }
                                        service.call(request).await
                                    }
                                });
                                let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), outer).await;
                            });
                        }
                    }
                }
                connections.abort_all();
                while connections.join_next().await.is_some() {}
            }),
        };
        Self {
            address,
            stop,
            task,
        }
    }

    pub(super) async fn stop(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
    }

    pub(super) async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &str,
        body: &str,
    ) -> RawResponse {
        let source = format!(
            "{method} {path} HTTP/1.1\r\nHost: forged.invalid\r\nForwarded: host=other.invalid;proto=https\r\nConnection: close\r\n{headers}\r\n{body}"
        );
        let address = self.address;
        tokio::task::spawn_blocking(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream.write_all(source.as_bytes()).unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            let offset = bytes
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap();
            let mut lines = std::str::from_utf8(&bytes[..offset]).unwrap().split("\r\n");
            let status = lines
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            let headers = lines
                .map(|line| {
                    let (name, value) = line.split_once(':').unwrap();
                    (name.to_ascii_lowercase(), value.trim().to_owned())
                })
                .collect();
            RawResponse {
                status,
                headers,
                body: bytes[offset + 4..].to_vec(),
            }
        })
        .await
        .unwrap()
    }

    pub(super) async fn graphql(&self, path: &str, document: &str, extra: &str) -> RawResponse {
        let body = serde_json::to_string(&serde_json::json!({"query": document})).unwrap();
        let headers = format!(
            "Content-Type: application/json\r\nAccept: application/graphql-response+json\r\nX-Chigiri-Preflight: 1\r\nContent-Length: {}\r\n{extra}",
            body.len()
        );
        self.request("POST", path, &headers, &body).await
    }
}

fn authorized<B>(request: &hyper::Request<B>) -> bool {
    request
        .headers()
        .get("authorization")
        .is_some_and(|value| value == "Bearer test-only")
}

async fn authenticate(request: Request, next: Next) -> Response {
    if authorized(&request) {
        next.run(request).await
    } else {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::UNAUTHORIZED;
        response
    }
}

pub(super) struct RawResponse {
    pub(super) status: u16,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}

impl RawResponse {
    pub(super) fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}
