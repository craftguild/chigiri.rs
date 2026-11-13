//! Verifies that read-only context extensions and endpoint dispatch share the same parent value.

use std::sync::{Arc, Mutex};

use chigiri::core::executor::ResolvedValue;
use chigiri::{
    BoxFuture, ContextFactory, Endpoint, FieldContext, FieldNext, FieldResult, GraphQlRequest,
    HttpRequestContext, PathSegment, Plugin, PrepareError, PreparedContext, Value,
};
use chigiri_http::{
    ApplicationMetadata, BufferedBody, HookError, RequestHeader, ResponseHeaders, ResponseHook,
    ResponseHookContext, TransportRequest,
};

use super::support::{Dispatch, NoDeadline, block_on, schema};

#[derive(Clone)]
struct BodyView(Arc<Mutex<Vec<BodyObservation>>>);

type BodyObservation = (Vec<u8>, usize);

impl ContextFactory<(), usize> for BodyView {
    fn create<'a>(
        &'a self,
        context: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), usize>, PrepareError>> {
        Box::pin(async move {
            let bytes = context.raw_body();
            self.0
                .lock()
                .unwrap()
                .push((bytes.to_vec(), bytes.as_ptr() as usize));
            if !bytes.is_empty() {
                assert_eq!(context.request().query, "{ echo }");
                let headers = context.transport().headers();
                assert_eq!(
                    headers
                        .iter()
                        .filter(|h| h.name.eq_ignore_ascii_case("x-duplicate"))
                        .count(),
                    2
                );
            }
            Ok(PreparedContext::new((), bytes.as_ptr() as usize))
        })
    }
}

impl ResponseHook<usize> for BodyView {
    fn call<'a>(
        &'a self,
        response: ResponseHookContext<'a, usize>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        Box::pin(async move {
            assert_eq!(
                response.request_context().raw_body().as_ptr() as usize,
                *response.state()
            );
            Ok(ResponseHeaders::new())
        })
    }
}

#[test]
fn hk_07_context_borrows_exact_bounded_body_across_factory_and_response_hook() {
    let source = schema("type Query { echo(value: Int = 7): Int }");
    let dispatch = Dispatch::new(&source, "Query", Value::Null);
    let seen = BodyView(Arc::default());
    let endpoint = Endpoint::builder(source.bind(dispatch).unwrap())
        .context_factory(seen.clone())
        .response_hook(seen.clone())
        .build()
        .unwrap();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("x-duplicate", b"first"),
        RequestHeader::new("X-Duplicate", b"second"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let body = b"{  \"query\" : \"{ \\u0065cho }\"  }\n";
    let response =
        block_on(endpoint.handle(request, BufferedBody::new(body.to_vec()), &NoDeadline));
    assert_eq!(response.status, 200);
    assert_eq!(seen.0.lock().unwrap()[0].0, body);
    for (bytes, status) in [(b"{".to_vec(), 400), (vec![b' '; 1024 * 1024 + 1], 413)] {
        assert_eq!(
            block_on(endpoint.handle(request, BufferedBody::new(bytes), &NoDeadline)).status,
            status
        );
    }
    assert_eq!(
        seen.0.lock().unwrap().len(),
        1,
        "You don't create context when you refuse to enter."
    );
    let headers = [RequestHeader::new("x-chigiri-preflight", b"1")];
    let request = TransportRequest::new(
        "GET",
        "/graphql",
        Some("query=%7B%20echo%20%7D"),
        &headers,
        None,
        &metadata,
    );
    assert_eq!(
        block_on(endpoint.handle(
            request,
            BufferedBody::new(b"ignored body".to_vec()),
            &NoDeadline
        ))
        .status,
        200
    );
    assert!(seen.0.lock().unwrap()[1].0.is_empty());
}

struct Parents(Arc<Mutex<Vec<ParentObservation>>>);

type ParentObservation = (String, Vec<PathSegment>, Option<Value>);

impl Plugin<()> for Parents {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let parent = context
                .parent()
                .map(|parent| parent.downcast_ref::<Value>().unwrap().clone());
            self.0.lock().unwrap().push((
                format!("{}.{}", context.parent_type(), context.field_name()),
                context.path().to_vec(),
                parent,
            ));
            next.run(context).await
        })
    }
}

#[test]
fn hk_08_field_context_has_exact_dispatch_parent_after_alias_fragment_and_skip() {
    let source = schema("type Query { items: [Item!]! } type Item { value: Int! }");
    let mut dispatch = Dispatch::new(&source, "Item", Value::Int(19));
    dispatch.rows = Some(2);
    let parents = Arc::new(Mutex::new(Vec::new()));
    let endpoint = Endpoint::builder(source.bind(dispatch).unwrap())
        .without_context()
        .push_plugin(Parents(parents.clone()))
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(GraphQlRequest::new("{ alias:items { ...Fields hidden:value @skip(if:true) } } fragment Fields on Item { renamed:value }"), (), &NoDeadline));
    assert!(response.errors.is_empty());
    assert_eq!(
        *parents.lock().unwrap(),
        [
            ("Query.items".into(), vec!["alias".into()], None),
            (
                "Item.value".into(),
                vec!["alias".into(), 0.into(), "renamed".into()],
                Some(Value::Int(19))
            ),
            (
                "Item.value".into(),
                vec!["alias".into(), 1.into(), "renamed".into()],
                Some(Value::Int(19))
            ),
        ]
    );
}
