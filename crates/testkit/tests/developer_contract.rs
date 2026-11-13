//! Test adapter-dependent contracts in the canonical developer workflow.

use std::fs;
use std::path::{Path, PathBuf};

use chigiri::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::spi::ExecutableSchema;
use chigiri::{
    DescriptorList, Endpoint, FieldDescriptor, FieldResult, OperationRootsDescriptor,
    SchemaDescriptor, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor, Value,
};
use chigiri_http::{
    BoxFuture as HttpBoxFuture, DeadlineFactory, PrepareError, PreparedContext,
    RequestContext as HttpRequestContext,
};

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static QUERY_FIELDS: &[FieldDescriptor] = &[FieldDescriptor {
    name: "health",
    description: None,
    type_ref: "String!",
    type_descriptor: TypeRefDescriptor::NonNull(&STRING),
    arguments: DescriptorList::empty(),
    directives: DescriptorList::empty(),
    deprecation_reason: None,
}];
static QUERY: TypeDescriptor = TypeDescriptor {
    name: "Query",
    kind: TypeDescriptorKind::Object,
    description: None,
    fields: DescriptorList::new(&[QUERY_FIELDS]),
    interfaces: DescriptorList::empty(),
    possible_types: DescriptorList::empty(),
    enum_values: DescriptorList::empty(),
    input_fields: DescriptorList::empty(),
    directives: DescriptorList::empty(),
    is_one_of: false,
    specified_by_url: None,
};
static TYPES: &[TypeDescriptor] = &[QUERY];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: chigiri::GENERATED_ABI_VERSION,
    schema_digest: chigiri::SchemaDigest::from_bytes([9; 32]),
    description: None,
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: None,
        subscription: None,
    },
    types: DescriptorList::new(&[TYPES]),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::empty(),
};

struct Dispatch;

impl ResolverDispatch<()> for Dispatch {
    fn schema_fingerprint(&self) -> chigiri::__private::ResolverSchemaFingerprint {
        chigiri::__private::descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::from("ok"))) })
    }
}

#[test]
fn same_endpoint_runs_direct_hyper_axum_without_server_api() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();

    let endpoint = Endpoint::builder(schema)
        .context_factory(endpoint_context)
        .build()
        .unwrap();
    let _: chigiri_hyper::HyperService<()> = chigiri_hyper::service(endpoint.clone());
    let _ = chigiri_axum::handler(endpoint);
}

fn endpoint_context(
    _: HttpRequestContext<'_>,
) -> HttpBoxFuture<'_, Result<PreparedContext<(), ()>, PrepareError>> {
    Box::pin(async { Ok(PreparedContext::execution_only(())) })
}

#[test]
fn public_api_has_no_product_types() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    for name in [
        "core",
        "runtime",
        "http",
        "hyper",
        "axum",
        "operation",
        "policy",
        "observability",
        "testkit",
    ] {
        collect_rust_sources(
            &workspace.join("crates").join(name).join("src"),
            &mut sources,
        );
    }
    let forbidden = ["principal", "tenant", "database", "jwt", "cookiejar"];
    for path in sources {
        let source = fs::read_to_string(&path).expect("framework source must be readable");
        for line in source
            .lines()
            .filter(|line| line.trim_start().starts_with("pub "))
        {
            let lowercase = line.to_ascii_lowercase();
            for token in forbidden {
                assert!(
                    !lowercase.contains(token),
                    "public API in {} contains product term {token}",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn endpoint_runs_without_client_tooling() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema).without_context().build().unwrap();
    let response = endpoint
        .execute(
            chigiri::GraphQlRequest::new("{ health }"),
            (),
            &NeverDeadline,
        )
        .await;
    assert!(response.errors.is_empty());

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for forbidden in ["operations", "client-manifest.json", "generated-client"] {
        assert!(!workspace.join(forbidden).exists());
    }
}

struct NeverDeadline;

impl DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> HttpBoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

fn collect_rust_sources(directory: &Path, output: &mut Vec<PathBuf>) {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("crate source directory must be readable") {
            let entry = entry.expect("source directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push(path);
            }
        }
    }
}
