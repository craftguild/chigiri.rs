//! Application-owned Hyper connections use the same SDK stop future.

use std::convert::Infallible;
use std::io;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::service::{Service, service_fn};
use hyper_util::rt::TokioIo;

pub(super) async fn serve(
    listener: tokio::net::TcpListener,
    graphql: chigiri_hyper::ManagedHyperService<()>,
    other: chigiri_hyper::ManagedHyperService<()>,
    mut hook: chigiri::BoxFuture<'static, ()>,
) -> io::Result<()> {
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let mut connections = tokio::task::JoinSet::new();
    let mut failure = None;
    loop {
        tokio::select! {
            biased;
            _ = &mut hook => break,
            result = connections.join_next(), if !connections.is_empty() => {
                if let Some(error) = joined(result.expect("nonempty native connection set")) {
                    failure = Some(error);
                    break;
                }
            }
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                };
                if connections.len() >= 8 {
                    failure = Some(io::Error::other("native fixture connection bound exceeded"));
                    break;
                }
                let graphql = graphql.clone();
                let other = other.clone();
                let mut shutdown = shutdown.clone();
                connections.spawn(async move {
                    let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                        let graphql = graphql.clone();
                        let other = other.clone();
                        async move {
                            let response = match request.uri().path() {
                                "/graphql" => graphql.call(request).await,
                                "/other-graphql" => other.call(request).await,
                                path => {
                                    let health = path == "/health";
                                    let body = if health { "healthy" } else { "missing" };
                                    let body = Full::new(Bytes::from_static(body.as_bytes()))
                                        .map_err(|never| -> io::Error { match never {} })
                                        .boxed_unsync();
                                    let mut response = hyper::Response::new(body);
                                    if !health {
                                        *response.status_mut() = hyper::StatusCode::NOT_FOUND;
                                    }
                                    return Ok::<_, Infallible>(response);
                                }
                            };
                            Ok(response.unwrap().map(|body| {
                                body.map_err(|error| io::Error::other(error.to_string()))
                                    .boxed_unsync()
                            }))
                        }
                    });
                    let builder = hyper::server::conn::http1::Builder::new();
                    let connection = builder.serve_connection(TokioIo::new(stream), service);
                    tokio::pin!(connection);
                    let result = tokio::select! {
                        result = &mut connection => result,
                        _ = shutdown.changed() => {
                            connection.as_mut().graceful_shutdown();
                            connection.await
                        }
                    };
                    result.map_err(io::Error::other)
                });
            }
        }
    }
    drop(listener);
    stop.send_replace(true);
    while let Some(result) = connections.join_next().await {
        if let Some(error) = joined(result) {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

fn joined(result: Result<io::Result<()>, tokio::task::JoinError>) -> Option<io::Error> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(error) => Some(io::Error::other(error)),
    }
}
