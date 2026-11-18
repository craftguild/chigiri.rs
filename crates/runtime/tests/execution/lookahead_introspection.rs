//! Implicit query meta-fields and built-in objects use the actual selection view.
use super::*;
use chigiri_runtime::{FieldContext, Object, SelectedField, SelectionError};
use std::sync::{Arc, Mutex};

static FIELDS: &[FieldDescriptor] = &[
    field("child", "Query", TypeRefDescriptor::Named("Query")),
    field("value", "String", STRING),
];
static TYPES: &[TypeDescriptor] = &[object_type("Query", DescriptorList::new(&[FIELDS]))];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: chigiri_runtime::SchemaDigest::from_bytes([93; 32]),
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

#[derive(Debug)]
struct Observed {
    name: String,
    key: String,
    output: String,
    children: Vec<Self>,
}

fn observe(field: SelectedField<'_>) -> Result<Observed, SelectionError> {
    let output = field.output_type();
    let children = match output {
        "__Schema" | "__Type" | "__Field" | "__InputValue" | "__EnumValue" | "__Directive" => {
            if output == "__Schema" {
                assert!(matches!(
                    field.selected_fields("__Type"),
                    Err(SelectionError::InvalidRuntimeType)
                ));
            }
            field
                .selected_fields(output)?
                .into_iter()
                .map(observe)
                .collect::<Result<_, _>>()?
        }
        _ => {
            assert!(matches!(
                field.selected_fields(output),
                Err(SelectionError::InvalidRuntimeType)
            ));
            Vec::new()
        }
    };
    Ok(Observed {
        name: field.name().into(),
        key: field.response_key().into(),
        output: output.into(),
        children,
    })
}

type Seen = Arc<Mutex<Vec<Result<Vec<Observed>, SelectionError>>>>;
struct Inspect(Seen);

impl ResolverDispatch<()> for Inspect {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        chigiri_runtime::__private::descriptor_fingerprint(&SCHEMA)
    }
    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}
    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        name: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            match name {
                "child" => Ok(ResolvedValue::object("Query", ())),
                "value" => Ok(ResolvedValue::Leaf(Value::from("ok"))),
                _ => panic!("introspection must not invoke application dispatch: {name}"),
            }
        })
    }
    fn resolve_with_context<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.field_name() == "child" {
            assert!(matches!(
                context.selected_fields("__Schema"),
                Err(SelectionError::InvalidRuntimeType)
            ));
            let selected = context
                .selected_fields("Query")
                .and_then(|fields| fields.into_iter().map(observe).collect::<Result<_, _>>());
            self.0.lock().unwrap().push(selected);
        }
        self.resolve(
            context.context(),
            context.parent_type(),
            context.field_name(),
            parent,
            arguments,
        )
    }
}

fn execute(query: &str, variables: Object) -> (Value, Vec<Observed>) {
    let seen = Seen::default();
    let schema = ExecutableSchema::builder(&SCHEMA, Inspect(seen.clone()))
        .introspection(IntrospectionPolicy::Enabled)
        .build()
        .unwrap();
    let response =
        block_on(schema.execute(GraphQlRequest::new(query).with_variables(variables), ()));
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let mut recorded = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(recorded.len(), 1);
    (response.data.unwrap(), recorded.pop().unwrap().unwrap())
}

fn assert_fields(actual: &[Observed], expected: &[(&str, &str, &str)]) {
    let actual: Vec<_> = actual
        .iter()
        .map(|field| {
            (
                field.name.as_str(),
                field.key.as_str(),
                field.output.as_str(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

fn children<'a>(fields: &'a [Observed], key: &str) -> &'a [Observed] {
    &fields
        .iter()
        .find(|field| field.key == key)
        .unwrap()
        .children
}

#[test]
fn query_meta_fields_preserve_ordinary_siblings_and_named_output_types() {
    for (name, output, query) in [
        (
            "__typename",
            "String",
            "{ child { ordinary: value selected: __typename } }",
        ),
        (
            "__schema",
            "__Schema",
            "{ child { ordinary: value selected: __schema { queryType { name } } } }",
        ),
        (
            "__type",
            "__Type",
            "{ child { ordinary: value selected: __type(name: \"Query\") { name } } }",
        ),
    ] {
        let (data, fields) = execute(query, Object::new());
        assert_fields(
            &fields,
            &[("value", "ordinary", "String"), (name, "selected", output)],
        );
        let child = data
            .as_object()
            .unwrap()
            .get("child")
            .unwrap()
            .as_object()
            .unwrap();
        assert_eq!(child.get("ordinary"), Some(&Value::from("ok")));
        assert!(child.get("selected").is_some());
    }
}

#[test]
fn introspection_children_preserve_aliases_fragments_directives_and_type_compatibility() {
    for show in [false, true] {
        let mut variables = Object::new();
        variables.insert("show", show);
        let (data, fields) = execute(
            r#"
            query Inspect($show: Boolean!, $skip: Boolean! = true) {
              child {
                ordinary: value
                schema: __schema { ...SchemaFields }
                schema: __schema { queryType { visible: name @include(if: $show) } }
                lookup: __type(name: "Query") @include(if: $show) { name }
                omitted: __type(name: "Query") @skip(if: $skip) { name }
              }
            }
            fragment SchemaFields on __Schema {
              ... on __Schema { kind: __typename }
              queryType {
                fields { name args { name } type { kind ofType { name } } }
                enumValues { name }
                inputFields { name }
              }
              directives { name args { name type { name } } locations }
            }
        "#,
            variables,
        );
        let mut expected = vec![
            ("value", "ordinary", "String"),
            ("__schema", "schema", "__Schema"),
        ];
        if show {
            expected.push(("__type", "lookup", "__Type"));
        }
        assert_fields(&fields, &expected);
        let schema = children(&fields, "schema");
        assert_fields(
            schema,
            &[
                ("__typename", "kind", "String"),
                ("queryType", "queryType", "__Type"),
                ("directives", "directives", "__Directive"),
            ],
        );
        let query_type = children(schema, "queryType");
        let mut expected = vec![
            ("fields", "fields", "__Field"),
            ("enumValues", "enumValues", "__EnumValue"),
            ("inputFields", "inputFields", "__InputValue"),
        ];
        if show {
            expected.push(("name", "visible", "String"));
        }
        assert_fields(query_type, &expected);
        let members = children(query_type, "fields");
        assert_fields(
            members,
            &[
                ("name", "name", "String"),
                ("args", "args", "__InputValue"),
                ("type", "type", "__Type"),
            ],
        );
        assert_fields(children(members, "args"), &[("name", "name", "String")]);
        assert_fields(
            children(members, "type"),
            &[
                ("kind", "kind", "__TypeKind"),
                ("ofType", "ofType", "__Type"),
            ],
        );
        assert_fields(
            children(children(members, "type"), "ofType"),
            &[("name", "name", "String")],
        );
        for key in ["enumValues", "inputFields"] {
            assert_fields(children(query_type, key), &[("name", "name", "String")]);
        }
        let directives = children(schema, "directives");
        assert_fields(
            directives,
            &[
                ("name", "name", "String"),
                ("args", "args", "__InputValue"),
                ("locations", "locations", "__DirectiveLocation"),
            ],
        );
        assert_fields(
            children(directives, "args"),
            &[("name", "name", "String"), ("type", "type", "__Type")],
        );
        assert_fields(
            children(children(directives, "args"), "type"),
            &[("name", "name", "String")],
        );
        let child = data
            .as_object()
            .unwrap()
            .get("child")
            .unwrap()
            .as_object()
            .unwrap();
        assert_eq!(child.get("ordinary"), Some(&Value::from("ok")));
        assert_eq!(child.get("lookup").is_some(), show);
        assert!(child.get("omitted").is_none());
        let schema = child.get("schema").unwrap().as_object().unwrap();
        assert_eq!(schema.get("kind"), Some(&Value::from("__Schema")));
        let query_type = schema.get("queryType").unwrap().as_object().unwrap();
        assert_eq!(query_type.get("visible").is_some(), show);
    }
}
