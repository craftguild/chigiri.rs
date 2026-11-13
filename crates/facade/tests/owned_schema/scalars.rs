//! Test scalar instance isolation, early default rejection, and shared HTTP paths.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri::core::executable::CustomScalarRegistry;
use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
use chigiri::spi::{ApplicationMetadata, BufferedBody, RequestHeader};
use chigiri::{BoxFuture, Endpoint, FieldResult, ScalarCoercionError, TransportRequest, Value};

use super::support::{Dispatch, NoDeadline, block_on, run, schema};

struct ScalarDispatch {
    inner: Dispatch,
    integer: bool,
    extra: Option<&'static str>,
    registrations: Arc<AtomicUsize>,
}

impl OwnedResolverDispatch<()> for ScalarDispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.inner.schema
    }
    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.inner.coordinates
    }
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        self.registrations.fetch_add(1, Ordering::SeqCst);
        let integer = self.integer;
        registry.register("Token", move |_, value| {
            if matches!(
                (&value, integer),
                (Value::Int(_), true) | (Value::String(_), false)
            ) {
                Ok(value)
            } else {
                Err(ScalarCoercionError::public("invalid Token representation"))
            }
        });
        if let Some(extra) = self.extra {
            registry.register(extra, |_, value| Ok(value));
        }
    }
    fn resolve<'a>(
        &'a self,
        ctx: &'a (),
        parent_type: &'a str,
        field: &'a str,
        parent: Option<&'a ResolvedObject>,
        args: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.inner.resolve(ctx, parent_type, field, parent, args)
    }
}

fn dispatch(schema: &OwnedSchema, integer: bool, extra: Option<&'static str>) -> ScalarDispatch {
    ScalarDispatch {
        inner: Dispatch::new(schema, "Query", Value::Null),
        integer,
        extra,
        registrations: Arc::new(AtomicUsize::new(0)),
    }
}

fn endpoint(source: &OwnedSchema, integer: bool) -> Endpoint<()> {
    Endpoint::builder(
        source
            .clone()
            .bind(dispatch(source, integer, None))
            .unwrap(),
    )
    .without_context()
    .build()
    .unwrap()
}

#[test]
fn rsm_20_same_scalar_name_never_shares_coercer_and_reconfigure_keeps_verified_registry() {
    let sdl = "scalar Token type Query { echo(value: Token!): Token! }";
    let source = schema(sdl);
    let resolver = dispatch(&source, true, None);
    let registrations = Arc::clone(&resolver.registrations);
    let integer = Endpoint::builder(source.clone().bind(resolver).unwrap())
        .without_context()
        .build()
        .unwrap();
    let string = endpoint(&schema(sdl), false);
    assert_eq!(registrations.load(Ordering::SeqCst), 1);
    for _ in 0..3 {
        assert!(run(&integer, "{ echo(value: 7) }").errors.is_empty());
        assert!(!run(&integer, "{ echo(value: \"text\") }").errors.is_empty());
        assert!(run(&string, "{ echo(value: \"text\") }").errors.is_empty());
        assert!(!run(&string, "{ echo(value: 7) }").errors.is_empty());
    }
    for extra in ["Unknown", "Int", "String"] {
        assert!(
            source
                .clone()
                .bind(dispatch(&source, true, Some(extra)))
                .is_err()
        );
    }
}

#[test]
fn rsm_20_custom_scalar_defaults_are_checked_before_endpoint_input_exists() {
    for sdl in [
        "scalar Token type Query { echo(value: Token = 7): Token }",
        "scalar Token input Input { value: Token = 7 } type Query { echo(value: Input): Token }",
        "scalar Token directive @other(value: Token = 7) on FIELD type Query { echo: Token }",
    ] {
        let source = schema(sdl);
        assert!(source.clone().bind(dispatch(&source, true, None)).is_ok());
        assert!(source.clone().bind(dispatch(&source, false, None)).is_err());
    }
}

#[test]
fn rsm_20_owned_http_keeps_variable_coercion_status_and_exact_response() {
    let source = schema("scalar Token type Query { echo(value: Token!): Token! }");
    let endpoint = endpoint(&source, true);
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let metadata = ApplicationMetadata::default();
    for (body, status, expected) in [
        (
            br#"{"query":"query($v:Token!){echo(value:$v)}","variables":{"v":7}}"#.as_slice(),
            200,
            Some(br#"{"data":{"echo":7}}"#.as_slice()),
        ),
        (
            br#"{"query":"query($v:Token!){echo(value:$v)}","variables":{"v":"bad"}}"#.as_slice(),
            422,
            None,
        ),
    ] {
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let response =
            block_on(endpoint.handle(request, BufferedBody::new(body.to_vec()), &NoDeadline));
        assert_eq!(response.status, status);
        if let Some(expected) = expected {
            assert_eq!(response.body, expected);
        } else {
            assert!(
                String::from_utf8(response.body)
                    .unwrap()
                    .contains("BAD_USER_INPUT")
            );
        }
    }
}
