use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use chigiri_books::{Catalog, endpoint};
use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader, TransportRequest,
};
use http_body_util::BodyExt as _;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;

const JSON_HEADERS: &str =
    "Content-Type: application/json\r\nAccept: application/graphql-response+json\r\n";
const ACCEPT: &str = "Accept: application/graphql-response+json\r\nX-Chigiri-Preflight: 1\r\n";

#[tokio::test]
async fn proj_14_project_quick_start_preserves_the_traditional_contract() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = endpoint(Catalog::seeded()).unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = chigiri_hyper::service(endpoint.clone());
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });

    let query = request(
        address,
        post(r#"{"query":"{ books { id title author } }"}"#, JSON_HEADERS),
    )
    .await;
    assert_eq!(query.status, 200);
    assert!(query.body.contains("The Left Hand of Darkness"));
    assert!(query.body.contains(r#""requestCount":1"#));

    let mutation = request(
        address,
        post(
            r#"{"query":"mutation Add($input:AddBookInput!){addBook(input:$input){id title author}}","operationName":"Add","variables":{"input":{"title":"A Wizard of Earthsea","author":"Ursula K. Le Guin"}}}"#,
            JSON_HEADERS,
        ),
    )
    .await;
    assert_eq!(mutation.status, 200);
    assert!(mutation.body.contains("A Wizard of Earthsea"));
    assert!(mutation.body.contains(r#""id":"2""#));

    let bounded_mutation = request(
        address,
        post(
            r#"{"query":"mutation Replace($id:ID!,$input:AddBookInput!){replaceBook(id:$id,input:$input){id title author}}","operationName":"Replace","variables":{"id":"1","input":{"title":"Soak","author":"Chigiri"}}}"#,
            JSON_HEADERS,
        ),
    )
    .await;
    assert_eq!(bounded_mutation.status, 200);
    assert!(bounded_mutation.body.contains(r#""replaceBook":{"id":"1""#));
    assert!(bounded_mutation.body.contains(r#""title":"Soak""#));

    let get_query = request(
        address,
        get(
            "/graphql?query=query%28%24id%3AID%21%29%7Bbook%28id%3A%24id%29%7Btitle%7D%7D&variables=%7B%22id%22%3A%222%22%7D",
            ACCEPT,
        ),
    )
    .await;
    assert_eq!(get_query.status, 200);
    assert!(get_query.body.contains("A Wizard of Earthsea"));

    let request_error = request(address, post(r#"{"query":"{ missing }"}"#, JSON_HEADERS)).await;
    assert_eq!(request_error.status, 422);
    assert!(request_error.body.contains("GRAPHQL_VALIDATION_FAILED"));

    let field_error = request(
        address,
        post(
            r#"{"query":"mutation { addBook(input:{title:\"\",author:\"nobody\"}) { id } }"}"#,
            JSON_HEADERS,
        ),
    )
    .await;
    assert_eq!(field_error.status, 200);
    assert!(field_error.body.contains("BAD_USER_INPUT"));

    let get_mutation = request(
        address,
        get(
            "/graphql?query=mutation%7BaddBook%28input%3A%7Btitle%3A%22x%22%2Cauthor%3A%22y%22%7D%29%7Bid%7D%7D",
            ACCEPT,
        ),
    )
    .await;
    assert_eq!(get_mutation.status, 405);

    let media_error = request(
        address,
        post(
            r#"{"query":"{books{id}}"}"#,
            "Content-Type: application/json\r\nAccept: text/html\r\n",
        ),
    )
    .await;
    assert_eq!(media_error.status, 406);

    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn api_12_short_construction_keeps_traditional_security_contract() {
    let endpoint = endpoint(Catalog::seeded()).unwrap();
    let configuration = endpoint.configuration();
    assert!(!*configuration.introspection_enabled().effective());
    assert_ne!(
        configuration
            .execution_limits()
            .effective()
            .max_output_nodes,
        usize::MAX
    );
    assert_eq!(
        configuration.rate_limit_coverage(),
        chigiri::RateLimitCoverage::Both
    );
    assert!(configuration.safety_overrides().is_empty());

    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server.rs"),
    )
    .unwrap();
    let assembly = source
        .split("let resolvers =")
        .nth(1)
        .unwrap()
        .split("struct BooksTransportKey")
        .next()
        .unwrap();
    assert!(assembly.contains("Endpoint::builder(resolvers)"));
    assert!(assembly.contains(".with_book(BookFields)"));
    assert!(assembly.lines().count() <= 45);
    let neutral = neutral_query(endpoint.clone()).await;

    let axum_request = axum::http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(axum::body::Body::from(query_body()))
        .unwrap();
    let response = chigiri_axum::handler(endpoint.clone())(axum_request).await;
    assert_eq!(response.status(), 200);
    let axum = serde_json::from_slice::<serde_json::Value>(
        &response.into_body().collect().await.unwrap().to_bytes(),
    )
    .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        http1::Builder::new()
            .keep_alive(false)
            .serve_connection(TokioIo::new(stream), chigiri_hyper::service(endpoint))
            .await
            .unwrap();
    });
    let hyper = request(address, post(query_body(), JSON_HEADERS)).await;
    task.await.unwrap();
    assert_eq!(hyper.status, 200);
    let hyper = serde_json::from_str::<serde_json::Value>(&hyper.body).unwrap();

    assert_eq!(neutral["data"], axum["data"]);
    assert_eq!(neutral["data"], hyper["data"]);
}

async fn neutral_query(
    endpoint: chigiri::Endpoint<chigiri_books::AppContext>,
) -> serde_json::Value {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let response = endpoint
        .handle(
            request,
            BufferedBody::new(query_body().as_bytes().to_vec()),
            &TokioDeadline,
        )
        .await;
    assert_eq!(response.status, 200);
    serde_json::from_slice(&response.body).unwrap()
}

struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

fn query_body() -> &'static str {
    r#"{"query":"{ books { id title author } }"}"#
}

struct RawResponse {
    status: u16,
    body: String,
}

async fn request(address: SocketAddr, request: String) -> RawResponse {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        parse_response(&response)
    })
    .await
    .unwrap()
}

fn parse_response(response: &[u8]) -> RawResponse {
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&response[..split]).unwrap();
    let status = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    RawResponse {
        status,
        body: String::from_utf8(response[split + 4..].to_vec()).unwrap(),
    }
}

fn post(body: &str, headers: &str) -> String {
    format!(
        "POST /graphql HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}

fn get(target: &str, headers: &str) -> String {
    format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n")
}
