//! Real HTTP checks for typed errors and application-owned context factories.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{AppContext, CONTEXT_DROPS, PENDING_DROPS, header, prepare_context, with_services};
use crate::http_fields::Fields;
use crate::{Types, map_app_error};

static HOOKS: AtomicUsize = AtomicUsize::new(0);

fn groups(calls: Arc<AtomicUsize>) -> generated::Resolvers<AppContext, Types> {
    generated::Resolvers::builder(move |error| {
        calls.fetch_add(1, Ordering::SeqCst);
        map_app_error(error)
    })
    .with_query(Fields)
    .with_mutation(Fields)
    .with_user(Fields)
    .build()
}

#[derive(Clone, Default)]
struct Observer(Arc<Mutex<Vec<(String, bool)>>>);

impl chigiri::ErrorObserver for Observer {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        self.0.lock().unwrap().push((
            event.error_id().unwrap().as_str().to_owned(),
            event.source().downcast_ref::<domain::AppError>().is_some(),
        ));
    }
}

struct SharedServices(Arc<domain::Users>);

impl chigiri::Plugin<AppContext> for SharedServices {
    fn field<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        next: chigiri::FieldNext<'a, AppContext>,
    ) -> chigiri::BoxFuture<'a, chigiri::FieldResult<chigiri::__private::ResolvedValue>> {
        assert!(Arc::ptr_eq(&ctx.context().users, &self.0));
        next.run(ctx)
    }
}

fn response_hook(
    context: chigiri::ResponseHookContext<'_, ()>,
) -> chigiri::BoxFuture<'_, Result<chigiri::ResponseHeaders, chigiri::HookError>> {
    Box::pin(async move {
        HOOKS.fetch_add(1, Ordering::SeqCst);
        assert_eq!(context.state(), &());
        let mut headers = chigiri::ResponseHeaders::new();
        headers
            .append(
                "x-hook-session",
                header(context.request_context(), "x-session"),
            )
            .unwrap();
        Ok(headers)
    })
}

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn post(
    address: std::net::SocketAddr,
    query: &str,
    session: &str,
    mode: &str,
) -> (u16, String, Value) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = json!({"query":query}).to_string();
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(format!(
            "POST /graphql HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/graphql-response+json\r\nx-session: {session}\r\nx-case: {mode}\r\nContent-Length: {}\r\n\r\n{body}", body.len(),
        ).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        stream.take(1024 * 1024).read_to_end(&mut bytes).await.unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let (headers, body) = text.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, headers.to_lowercase(), serde_json::from_str(body).unwrap())
    }).await.expect("task-owned HTTP request exceeded its bound")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ri_04_06_real_http_preserves_errors_and_request_context_lifecycle() {
    let calls = Arc::new(AtomicUsize::new(0));
    // This compiles the named-function recipe against real generated output.
    let _named = chigiri::Endpoint::builder(groups(calls.clone()))
        .context_factory(prepare_context)
        .build()
        .unwrap();
    let users = Arc::new(domain::Users);
    let observer = Observer::default();
    let endpoint = chigiri::Endpoint::builder(groups(calls.clone()))
        .push_plugin(SharedServices(users.clone()))
        .context_factory(with_services(users))
        .response_hook(response_hook)
        .inject(|components| {
            components.http().context_timeout(Duration::from_millis(20));
            components.observability().error_observer(observer.clone());
        })
        .build()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/graphql",
        axum::routing::any(chigiri_axum::handler(endpoint)),
    );
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap()
    }));

    let mut identities = Vec::new();
    for session in ["first", "second"] {
        let (status, headers, body) = post(address, "{ success }", session, "ready").await;
        assert_eq!(status, 200);
        assert!(body.get("errors").is_none(), "{body}");
        let value = body["data"]["success"].as_str().unwrap();
        let parts = value.split(':').collect::<Vec<_>>();
        assert_eq!(parts[0], session);
        assert_eq!(parts[2], "1", "each context must own a fresh cache");
        identities.push(parts[1].to_owned());
        assert!(headers.contains(&format!("x-hook-session: {session}")));
    }
    assert_ne!(identities[0], identities[1]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (status, _, body) = post(
        address,
        "{ missing: user(id: \"missing\") { name } secret: internal }",
        "errors",
        "ready",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"], json!({"missing":null,"secret":null}));
    assert_eq!(body["errors"].as_array().unwrap().len(), 2);
    assert_eq!(body["errors"][0]["message"], "User was not found");
    assert_eq!(body["errors"][0]["extensions"]["code"], "NOT_FOUND");
    assert_eq!(body["errors"][0]["path"], json!(["missing"]));
    assert_eq!(body["errors"][0]["locations"][0]["line"], 1);
    assert_eq!(body["errors"][1]["message"], "Internal server error");
    assert_eq!(body["errors"][1]["path"], json!(["secret"]));
    assert!(!body.to_string().contains("credential"));
    let observed = observer.0.lock().unwrap().clone();
    assert_eq!(observed.len(), 1);
    assert!(observed[0].1);
    assert_eq!(body["errors"][1]["extensions"]["errorId"], observed[0].0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let (_, _, body) = post(
        address,
        "{ u: user(id: \"ada\") { name displayName oops: failure } }",
        "object",
        "ready",
    )
    .await;
    assert_eq!(
        body["data"],
        json!({"u":{"name":"Ada","displayName":"Lovelace Ada","oops":null}})
    );
    assert_eq!(body["errors"][0]["path"], json!(["u", "oops"]));
    let (status, _, body) = post(
        address,
        "mutation { rename(id: \"missing\", name: \"x\") { name } }",
        "mutation",
        "ready",
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"], Value::Null);
    assert_eq!(body["errors"].as_array().unwrap().len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let (_, _, body) = post(address, "{ floats }", "framework", "ready").await;
    assert_eq!(body["data"], json!({"floats":[null]}));
    assert_eq!(body["errors"][0]["path"], json!(["floats", 0]));
    assert_eq!(calls.load(Ordering::SeqCst), 4);

    assert_eq!(HOOKS.load(Ordering::SeqCst), 6);
    assert_eq!(CONTEXT_DROPS.load(Ordering::SeqCst), 6);
    for (mode, expected) in [("reject", 403), ("internal", 500), ("pending", 504)] {
        // Invalid GraphQL proves preparation retains its position before validation.
        let (status, _, body) = post(address, "invalid GraphQL", "preparation", mode).await;
        assert_eq!(status, expected, "{body}");
        assert!(body.get("data").is_none());
        assert!(!body.to_string().contains("credential"));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
    assert_eq!(PENDING_DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(HOOKS.load(Ordering::SeqCst), 6);
    assert_eq!(CONTEXT_DROPS.load(Ordering::SeqCst), 6);
}
