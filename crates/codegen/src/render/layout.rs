//! Selects private file paths by SDL owner and operation role.

use std::collections::BTreeMap;

use crate::CodegenError;

use super::model::{RenderModel, field_name};

pub(super) fn owner(name: &str) -> String {
    field_name(name).trim_start_matches("r#").to_owned()
}

pub(super) fn resolver(model: &RenderModel<'_>, name: &str) -> String {
    let roots = model.schema.roots();
    for (role, root) in [
        ("query", roots.query()),
        ("mutation", roots.mutation()),
        ("subscription", roots.subscription()),
    ] {
        if root.is_some_and(|root| root.type_name == name) {
            return format!("resolvers/{role}.rs");
        }
    }
    format!("resolvers/objects/{}.rs", owner(name))
}

pub(super) fn validate(model: &RenderModel<'_>) -> Result<(), CodegenError> {
    let mut paths = BTreeMap::new();
    for ty in model.user_types() {
        let path = format!("types/{}", owner(ty.name()).to_ascii_lowercase());
        if paths.insert(path, ty.name()).is_some() {
            return Err(collision());
        }
    }
    for directive in model
        .schema
        .directive_definitions()
        .iter()
        .filter(|definition| definition.source_definition().is_some())
    {
        let path = format!(
            "directives/{}",
            owner(directive.name()).to_ascii_lowercase()
        );
        if paths.insert(path, directive.name()).is_some() {
            return Err(collision());
        }
    }
    Ok(())
}

fn collision() -> CodegenError {
    CodegenError::new(
        "SF4006",
        "The SDL owner's generated file path collides after normalization.",
    )
}
