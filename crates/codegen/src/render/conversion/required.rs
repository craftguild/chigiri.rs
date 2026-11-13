//! Selects resolver conversion helpers from the semantic type graph.

use std::collections::BTreeSet;

use chigiri_core::schema::TypeKind;

use super::super::model::{RenderModel, named_ref};

pub(super) fn names(model: &RenderModel<'_>) -> BTreeSet<String> {
    let objects = model
        .user_types()
        .filter(|ty| ty.kind() == TypeKind::Object);
    let mut inputs = Vec::new();
    let mut outputs = BTreeSet::new();
    for object in objects {
        for field in object.fields() {
            outputs.insert(named_ref(&field.ty).to_owned());
            inputs.extend(
                field
                    .arguments
                    .iter()
                    .map(|arg| named_ref(&arg.ty).to_owned()),
            );
        }
    }
    let mut required = BTreeSet::new();
    while let Some(name) = inputs.pop() {
        if !required.insert(name.clone()) {
            continue;
        }
        if let Some(input) = model.schema.type_by_name(&name) {
            inputs.extend(
                input
                    .input_fields()
                    .iter()
                    .map(|field| named_ref(&field.ty).to_owned()),
            );
        }
    }
    // Interface and union outputs require conversion for each possible object type.
    for name in &outputs {
        if let Some(composite) = model.schema.type_by_name(name) {
            required.extend(
                model
                    .possible_objects(composite)
                    .iter()
                    .map(|object| object.name().to_owned()),
            );
        }
    }
    required.extend(outputs);
    required
}
