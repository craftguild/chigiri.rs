//! Verify custom executable directive locations, coercion, and fragment ancestry.

use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{BoxFuture, ResolvedValue};
use chigiri_runtime::{
    DescriptorList, DirectiveDefinitionDescriptor, ExecutableDirective, ExecutableDirectiveSite,
    ExecutableSchema, FieldContext, FieldNext, FieldResult, GENERATED_ABI_VERSION, GraphQlRequest,
    GraphQlResponse, InputValueDescriptor, Object, OperationContext, OperationNext,
    OperationRootsDescriptor, PathSegment, Plugin, SchemaDescriptor, TypeRefDescriptor, Value,
};

use super::{Dispatch, STRING, TYPES, block_on};

static AUTH_ARGUMENT: InputValueDescriptor = InputValueDescriptor {
    name: "scope",
    description: None,
    type_ref: "String!",
    type_descriptor: TypeRefDescriptor::NonNull(&STRING),
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static AUTH_ARGUMENTS: &[InputValueDescriptor] = &[AUTH_ARGUMENT];
static AUTH_LOCATIONS: &[&str] = &[
    "QUERY",
    "VARIABLE_DEFINITION",
    "FIELD",
    "FRAGMENT_DEFINITION",
    "FRAGMENT_SPREAD",
    "INLINE_FRAGMENT",
];
static DIRECTIVES: &[DirectiveDefinitionDescriptor] = &[DirectiveDefinitionDescriptor {
    name: "auth",
    description: None,
    arguments: DescriptorList::new(&[AUTH_ARGUMENTS]),
    repeatable: false,
    locations: DescriptorList::new(&[AUTH_LOCATIONS]),
}];
static DIRECTIVE_SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: chigiri_runtime::SchemaDigest::from_bytes([3; 32]),
    description: None,
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: Some("Mutation"),
        subscription: None,
    },
    types: DescriptorList::new(&[TYPES]),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::new(&[DIRECTIVES]),
};

#[derive(Default)]
struct Observed {
    operation: Vec<ExecutableDirective>,
    fields: Vec<(String, Vec<ExecutableDirective>)>,
}

struct DirectivePlugin(Arc<Mutex<Observed>>);

impl Plugin<()> for DirectivePlugin {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, ()>,
        next: OperationNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.0.lock().unwrap().operation = context.directives().to_vec();
        next.run(context)
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let PathSegment::Field(response_key) = &context.path()[0] else {
            unreachable!("expected root field path.");
        };
        self.0
            .lock()
            .unwrap()
            .fields
            .push((response_key.clone(), context.directives().to_vec()));
        next.run(context)
    }
}

#[test]
fn exposes_all_executable_locations_as_directive_metadata_with_source_sites() {
    let observed = Arc::new(Mutex::new(Observed::default()));
    let schema = ExecutableSchema::builder(&DIRECTIVE_SCHEMA, Dispatch)
        .plugin(DirectivePlugin(Arc::clone(&observed)))
        .build()
        .unwrap();
    let mut variables = Object::new();
    variables.insert("scope", "runtime");
    let request = GraphQlRequest::new(
        r#"
        query Policy($scope: String! @auth(scope: "variable"))
          @auth(scope: $scope) {
          __typename @auth(scope: "meta")
          plain: hello @auth(scope: "field")
          ...Named @auth(scope: "spread")
          ... @auth(scope: "inline") { inlined: hello }
        }
        fragment Named on Query @auth(scope: "definition") { named: hello }
        "#,
    )
    .with_variables(variables);

    let response = block_on(schema.execute(request, ()));

    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let observed = observed.lock().unwrap();
    assert_operation_sites(&observed.operation);
    assert_field_sites(&observed.fields);
}

fn assert_operation_sites(directives: &[ExecutableDirective]) {
    assert_eq!(directives.len(), 7);
    assert!(has_site(directives, |site| matches!(
        site,
        ExecutableDirectiveSite::VariableDefinition { variable } if variable == "scope"
    )));
    let operation = directives
        .iter()
        .find(|directive| matches!(directive.site(), ExecutableDirectiveSite::Operation))
        .unwrap();
    assert_eq!(
        operation.arguments().get("scope"),
        Some(&Value::from("runtime"))
    );
    assert!(has_site(directives, |site| matches!(
        site,
        ExecutableDirectiveSite::Field { field } if field == "__typename"
    )));
    assert!(has_site(directives, |site| matches!(
        site,
        ExecutableDirectiveSite::FragmentDefinition { fragment } if fragment == "Named"
    )));
    assert!(has_site(directives, |site| matches!(
        site,
        ExecutableDirectiveSite::FragmentSpread { fragment } if fragment == "Named"
    )));
    assert!(has_site(directives, |site| matches!(
        site,
        ExecutableDirectiveSite::InlineFragment
    )));
}

fn assert_field_sites(fields: &[(String, Vec<ExecutableDirective>)]) {
    let plain = named_field(fields, "plain");
    assert!(has_site(plain, |site| matches!(
        site,
        ExecutableDirectiveSite::Field { field } if field == "hello"
    )));
    let named = named_field(fields, "named");
    assert!(has_site(named, |site| matches!(
        site,
        ExecutableDirectiveSite::FragmentSpread { fragment } if fragment == "Named"
    )));
    assert!(has_site(named, |site| matches!(
        site,
        ExecutableDirectiveSite::FragmentDefinition { fragment } if fragment == "Named"
    )));
    let inlined = named_field(fields, "inlined");
    assert!(has_site(inlined, |site| matches!(
        site,
        ExecutableDirectiveSite::InlineFragment
    )));
}

fn named_field<'a>(
    fields: &'a [(String, Vec<ExecutableDirective>)],
    name: &str,
) -> &'a [ExecutableDirective] {
    &fields.iter().find(|(path, _)| path == name).unwrap().1
}

fn has_site(
    directives: &[ExecutableDirective],
    predicate: impl Fn(&ExecutableDirectiveSite) -> bool,
) -> bool {
    directives
        .iter()
        .any(|directive| predicate(directive.site()))
}
