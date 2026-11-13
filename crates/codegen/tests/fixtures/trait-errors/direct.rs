//! Count application conversion at the actual generated dispatch boundary.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri::{GraphQlRequest, PathSegment, Value};

use crate::{AppContext, map_app_error, resolvers};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

pub struct NeverDeadline;

impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

#[derive(Clone)]
struct ErrorRecord {
    error_id: String,
    path: Vec<PathSegment>,
    external: bool,
}

#[derive(Clone, Default)]
struct Observer(Arc<Mutex<Vec<ErrorRecord>>>);

impl chigiri::ErrorObserver for Observer {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        self.0.lock().unwrap().push(ErrorRecord {
            error_id: event.error_id().unwrap().as_str().to_owned(),
            path: event.path().unwrap().to_vec(),
            external: event.source().downcast_ref::<domain::AppError>().is_some(),
        });
    }
}

fn context() -> AppContext {
    AppContext {
        users: Arc::new(domain::Users),
    }
}

fn prepare_context(
    _request: chigiri::HttpRequestContext<'_>,
) -> chigiri::BoxFuture<'_, Result<chigiri::PreparedContext<AppContext, ()>, chigiri::PrepareError>>
{
    Box::pin(async { Ok(chigiri::PreparedContext::execution_only(context())) })
}

fn endpoint(calls: &Arc<AtomicUsize>, observer: Observer) -> chigiri::Endpoint<AppContext> {
    let calls = calls.clone();
    chigiri::Endpoint::builder(resolvers(move |error| {
        calls.fetch_add(1, Ordering::SeqCst);
        map_app_error(error)
    }))
    .inject(|components| {
        components.observability().error_observer(observer);
    })
    .push_plugin(Intercept)
    .context_factory(prepare_context)
    .build()
    .unwrap()
}

struct Intercept;

impl chigiri::Plugin<AppContext> for Intercept {
    fn field<'a>(
        &'a self,
        ctx: chigiri::FieldContext<'a, AppContext>,
        next: chigiri::FieldNext<'a, AppContext>,
    ) -> chigiri::BoxFuture<'a, chigiri::FieldResult<chigiri::__private::ResolvedValue>> {
        if ctx.selection().is_some_and(|s| s.response_key == "blocked") {
            return Box::pin(async {
                Err(chigiri::FieldError::internal(std::io::Error::other(
                    "private middleware",
                )))
            });
        }
        next.run(ctx)
    }
}

#[test]
fn ri_02_04_domain_returns_computed_fields_and_unselected_failures_do_not_map() {
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = endpoint(&calls, Observer::default());
    let response = block_on(endpoint.execute(
        GraphQlRequest::new("{ success user(id: \"ada\") { name displayName } users { name } }"),
        context(),
        &NeverDeadline,
    ));
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let user = Value::Object(
        [
            ("name", Value::String("Ada".to_owned())),
            ("displayName", Value::String("Lovelace Ada".to_owned())),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        response.data,
        Some(Value::Object(
            [
                ("success", Value::String("ready".to_owned())),
                ("user", user),
                (
                    "users",
                    Value::List(vec![Value::Object(
                        [("name", Value::String("Ada".to_owned())),]
                            .into_iter()
                            .collect()
                    )])
                ),
            ]
            .into_iter()
            .collect()
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn ri_04_public_errors_internal_sources_aliases_partial_data_and_bubbling() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observer = Observer::default();
    let endpoint = endpoint(&calls, observer.clone());
    let response = block_on(endpoint.execute(
        GraphQlRequest::new(
            "{ good: success missing: user(id: \"missing\") { name } secret: internal }",
        ),
        context(),
        &NeverDeadline,
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(response.errors.len(), 2);
    assert_eq!(response.errors[0].message, "User was not found");
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&Value::String("NOT_FOUND".to_owned()))
    );
    assert_eq!(
        response.errors[0].path,
        vec![PathSegment::Field("missing".to_owned())]
    );
    assert_eq!(response.errors[0].locations.len(), 1);
    assert_eq!(response.errors[1].message, "Internal server error");
    assert_eq!(
        response.errors[1].path,
        vec![PathSegment::Field("secret".to_owned())]
    );
    assert_eq!(
        response.data,
        Some(Value::Object(
            [
                ("good", Value::String("ready".to_owned())),
                ("missing", Value::Null),
                ("secret", Value::Null),
            ]
            .into_iter()
            .collect()
        ))
    );
    let observed = observer.0.lock().unwrap().clone();
    assert_eq!(observed.len(), 1);
    assert!(
        observed[0].external,
        "original external domain error must be retained"
    );
    assert_eq!(observed[0].path, response.errors[1].path);
    assert_eq!(
        response.errors[1].extensions.get("errorId"),
        Some(&Value::String(observed[0].error_id.clone()))
    );
    assert!(!format!("{response:?}").contains("credential"));

    for (query, path) in [
        (
            "{ u: user(id: \"ada\") { oops: failure } }",
            vec![
                PathSegment::Field("u".to_owned()),
                PathSegment::Field("oops".to_owned()),
            ],
        ),
        (
            "mutation { rename(id: \"missing\", name: \"x\") { name } }",
            vec![PathSegment::Field("rename".to_owned())],
        ),
        (
            "{ required }",
            vec![PathSegment::Field("required".to_owned())],
        ),
    ] {
        let before = calls.load(Ordering::SeqCst);
        let response =
            block_on(endpoint.execute(GraphQlRequest::new(query), context(), &NeverDeadline));
        assert_eq!(response.errors.len(), 1);
        assert_eq!(response.errors[0].path, path);
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        if !query.contains("oops") {
            assert_eq!(response.data, Some(Value::Null));
        }
    }
}

#[test]
fn ri_05_framework_input_invariants_and_list_item_errors_bypass_mapper() {
    use chigiri::__private::{ArgumentValues, ResolverDispatch};
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let groups = resolvers(move |error| {
        counter.fetch_add(1, Ordering::SeqCst);
        map_app_error(error)
    });
    let error = block_on(groups.resolve(
        &context(),
        "Query",
        "user",
        None,
        ArgumentValues::new(vec![]),
    ));
    assert!(error.is_err());
    let error = block_on(groups.resolve(
        &context(),
        "User",
        "name",
        None,
        ArgumentValues::new(vec![]),
    ));
    assert!(error.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let observer = Observer::default();
    let endpoint = endpoint(&calls, observer.clone());
    let response =
        block_on(endpoint.execute(GraphQlRequest::new("{ floats }"), context(), &NeverDeadline));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(response.errors.len(), 1);
    assert_eq!(
        response.errors[0].path,
        vec![
            PathSegment::Field("floats".to_owned()),
            PathSegment::Index(1)
        ]
    );
    assert_eq!(
        response.data,
        Some(Value::Object(
            [(
                "floats",
                Value::List(vec![Value::Float(1.0), Value::Null, Value::Null])
            ),]
            .into_iter()
            .collect()
        ))
    );
    assert_eq!(observer.0.lock().unwrap().len(), 1);

    for query in [
        "{ blocked: success }",
        "{ __schema { queryType { name } } }",
    ] {
        let response =
            block_on(endpoint.execute(GraphQlRequest::new(query), context(), &NeverDeadline));
        assert_eq!(response.errors.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
