//! Walk document values in source order against their expected types.

use crate::language::ast::{
    Definition, Directive, Document, ExecutableDefinition, Selection, SelectionSet,
};

use super::catalog::{Catalog, root_name};
use super::literal::Inspector;
use super::types::InputType;

pub(super) fn validate_static<'a>(
    schema: &'a crate::schema::SemanticSchema,
    document: &'a Document,
    catalog: &Catalog<'a>,
    inspector: &mut Inspector<'a, '_>,
) -> bool {
    for definition in &document.definitions {
        let Definition::Executable(definition) = definition else {
            continue;
        };
        match definition {
            ExecutableDefinition::Operation(operation) => {
                for variable in &operation.variable_definitions {
                    if let Some(default) = &variable.default_value
                        && !inspector.inspect(default, InputType::from_ast(&variable.ty), false)
                    {
                        return false;
                    }
                    if !directives(catalog, &variable.directives, inspector) {
                        return false;
                    }
                }
                if !directives(catalog, &operation.directives, inspector) {
                    return false;
                }
                if let Some(root) = root_name(schema, operation.operation_type)
                    && !selection_set(catalog, &operation.selection_set, root, inspector)
                {
                    return false;
                }
            }
            ExecutableDefinition::Fragment(fragment) => {
                if !directives(catalog, &fragment.directives, inspector)
                    || !selection_set(
                        catalog,
                        &fragment.selection_set,
                        fragment.type_condition.value.as_str(),
                        inspector,
                    )
                {
                    return false;
                }
            }
        }
    }
    true
}

fn selection_set<'a>(
    catalog: &Catalog<'a>,
    root: &'a SelectionSet,
    root_parent: &'a str,
    inspector: &mut Inspector<'a, '_>,
) -> bool {
    let mut work = root
        .selections
        .iter()
        .rev()
        .map(|selection| (selection, root_parent))
        .collect::<Vec<_>>();
    while let Some((selection, parent)) = work.pop() {
        match selection {
            Selection::Field(field) => {
                for argument in &field.arguments {
                    if let Some(expected) =
                        catalog.field_argument(parent, &field.name.value, &argument.name.value)
                        && !inspector.inspect(&argument.value, expected.ty, expected.has_default)
                    {
                        return false;
                    }
                }
                if !directives(catalog, &field.directives, inspector) {
                    return false;
                }
                if let (Some(nested), Some(rule)) = (
                    field.selection_set.as_ref(),
                    catalog.field_output(parent, &field.name.value),
                ) {
                    for selection in nested.selections.iter().rev() {
                        work.push((selection, rule));
                    }
                }
            }
            Selection::FragmentSpread(spread) => {
                if !directives(catalog, &spread.directives, inspector) {
                    return false;
                }
            }
            Selection::InlineFragment(fragment) => {
                if !directives(catalog, &fragment.directives, inspector) {
                    return false;
                }
                let next = fragment
                    .type_condition
                    .as_ref()
                    .map_or(parent, |condition| condition.value.as_str());
                for selection in fragment.selection_set.selections.iter().rev() {
                    work.push((selection, next));
                }
            }
        }
    }
    true
}

fn directives<'a>(
    catalog: &Catalog<'a>,
    directives: &[Directive],
    inspector: &mut Inspector<'a, '_>,
) -> bool {
    for directive in directives {
        for argument in &directive.arguments {
            let Some(expected) =
                catalog.directive_argument(&directive.name.value, &argument.name.value)
            else {
                continue;
            };
            if !inspector.inspect(&argument.value, expected.ty, expected.has_default) {
                return false;
            }
        }
    }
    true
}
