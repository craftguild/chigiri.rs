//! Build scalar callbacks from owned declarations without retaining schema instances or external codecs.

use chigiri::core::executable::{CustomScalarRegistry, ScalarCoercionError};

use super::{ValueBudget, scalar};
use crate::ir::MockIr;

/// Register every IR custom scalar with the existing coercion registry.
///
/// In addition to the core coercion budget, validate each scalar's representation, depth, and value budget.
pub fn scalar_registry(ir: &MockIr) -> CustomScalarRegistry {
    registry(ir.graph())
}

/// Register only scalars visible to public GraphQL coercion.
pub fn public_scalar_registry(ir: &MockIr) -> CustomScalarRegistry {
    registry(ir.public_graph())
}

fn registry(graph: &crate::ir::SchemaGraph) -> CustomScalarRegistry {
    let mut registry = CustomScalarRegistry::new();
    for node in graph.types() {
        let Some(spec) = &node.scalar else { continue };
        let spec = spec.clone();
        registry.register(node.name.as_ref(), move |_, value| {
            let mut budget = ValueBudget::default();
            let result = budget.node(&value, 1).and_then(|()| {
                if value.is_null() {
                    Ok(())
                } else {
                    scalar::custom(&spec, &value, 1, &mut budget)
                }
            });
            result.map_err(|_| ScalarCoercionError::public("invalid mock scalar value"))?;
            Ok(value)
        });
    }
    registry
}
