//! Test the boundary between serial mutation roots and normal nested execution.

use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{ArgumentValues, BoxFuture, ResolvedValue, ResolverDispatch};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{ExecutableSchema, FieldError, FieldResult, GraphQlRequest};

use super::concurrency::{PairFuture, PairState};
use super::{Item, SCHEMA, block_on};

#[test]
fn nested_mutation_fields_use_normal_concurrent_execution() {
    let state = Arc::new(Mutex::new(PairState::default()));
    let schema = ExecutableSchema::builder(&SCHEMA, NestedMutationDispatch(Arc::clone(&state)))
        .build()
        .unwrap();

    let response =
        block_on(schema.execute(GraphQlRequest::new("mutation { update { slow fast } }"), ()));

    assert!(response.errors.is_empty());
    assert!(state.lock().unwrap().both_polled);
}

struct NestedMutationDispatch(Arc<Mutex<PairState>>);

impl ResolverDispatch<()> for NestedMutationDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        parent_type: &'a str,
        field_name: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        match (parent_type, field_name) {
            ("Mutation", "update") => Box::pin(async {
                Ok(ResolvedValue::object(
                    "Item",
                    Item {
                        name: "mutation".to_owned(),
                        fail_name: false,
                    },
                ))
            }),
            ("Item", "slow" | "fast") => Box::pin(PairFuture {
                value: field_name.to_owned(),
                state: Arc::clone(&self.0),
            }),
            _ => Box::pin(async { Err(FieldError::public("unknown nested mutation field")) }),
        }
    }
}
