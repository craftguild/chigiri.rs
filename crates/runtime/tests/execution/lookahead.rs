//! Effective selection inspection must agree with actual field execution.
use super::*;
use chigiri_runtime::{FieldContext, Object, SelectionError};
use std::sync::{Arc, Mutex};

type RecordedSelection = (String, Vec<(String, String)>);

#[derive(Default)]
struct Seen(Mutex<Vec<RecordedSelection>>);
struct Inspect(Arc<Seen>);

impl ResolverDispatch<()> for Inspect {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        dispatch_fingerprint()
    }
    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}
    fn resolve<'a>(
        &'a self,
        context: &'a (),
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Dispatch.resolve(context, parent_type, field_name, parent, arguments)
    }
    fn resolve_with_context<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.parent_type() == "Query"
            && matches!(context.field_name(), "item" | "node" | "result")
        {
            assert!(matches!(
                context.selected_fields("Query"),
                Err(SelectionError::InvalidRuntimeType)
            ));
            let fields = context.selected_fields("Item").unwrap();
            let selected = fields
                .iter()
                .map(|field| (field.name().into(), field.response_key().into()))
                .collect();
            self.0
                .0
                .lock()
                .unwrap()
                .push((context.selection().unwrap().response_key.into(), selected));
        }
        Dispatch.resolve(
            context.context(),
            context.parent_type(),
            context.field_name(),
            parent,
            arguments,
        )
    }
}

#[test]
fn lookahead_uses_coerced_directives_fragments_aliases_merged_fields_and_abstract_types() {
    let seen = Arc::new(Seen::default());
    let schema = ExecutableSchema::builder(&SCHEMA, Inspect(seen.clone()))
        .build()
        .unwrap();
    for show in [true, false] {
        let mut variables = Object::new();
        variables.insert("show", show);
        let response = block_on(
            schema.execute(
                GraphQlRequest::new(
                    r#"
            query Q($show: Boolean!, $skip: Boolean! = true) {
              selected: item { alias: name @include(if: $show) }
              selected: item { ...More }
              interface: node { ... on Item { name } }
              union: result { __typename ... on Item { value: name } }
              absent: item @skip(if: true) { slow }
            }
            fragment More on Item { fast name @skip(if: $skip) }
        "#,
                )
                .with_variables(variables),
                (),
            ),
        );
        assert!(response.errors.is_empty(), "{:?}", response.errors);
        let mut recorded = std::mem::take(&mut *seen.0.lock().unwrap());
        recorded.sort();
        assert_eq!(recorded.len(), 3);
        let selected = &recorded
            .iter()
            .find(|(alias, _)| alias == "selected")
            .unwrap()
            .1;
        let expected = if show {
            vec![
                ("name".into(), "alias".into()),
                ("fast".into(), "fast".into()),
            ]
        } else {
            vec![("fast".into(), "fast".into())]
        };
        assert_eq!(selected, &expected);
        assert_eq!(
            recorded
                .iter()
                .find(|(alias, _)| alias == "union")
                .unwrap()
                .1,
            vec![
                ("__typename".into(), "__typename".into()),
                ("name".into(), "value".into())
            ]
        );
    }
    let owned_context = String::from("not Copy");
    let manually_constructed = FieldContext::new(&owned_context, "Query", "item");
    let copied = manually_constructed;
    assert!(matches!(
        copied.selected_fields("Item"),
        Err(SelectionError::Unavailable)
    ));
    assert_eq!(manually_constructed.field_name(), "item");
}
