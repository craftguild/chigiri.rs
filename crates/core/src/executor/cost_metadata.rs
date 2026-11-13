//! Passes schema coordinates and bounded argument metadata to cost policies before resolver execution.

use super::cost_budget::{Budget, CostMetadataLimit, signature_bytes};
use crate::Value;
use crate::executable::{CoercedInput, CoercedVariables, SelectedOperation, ValidatedDocument};
use crate::language::ast::{
    Field, InputValueDefinition, Selection, SelectionSet, TypeRef, TypeRefKind, Value as AstValue,
    ValueNode,
};
use crate::schema::SemanticSchema;

/// The closed classification describes how the argument was determined by a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostArgumentPresence {
    Absent,
    Defaulted,
    Provided,
}

/// Argument view used to derive a list multiplier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostArgumentMetadata {
    name: String,
    type_signature: String,
    presence: CostArgumentPresence,
    bounded_integer: Option<u64>,
}

impl CostArgumentMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn type_signature(&self) -> &str {
        &self.type_signature
    }
    pub const fn presence(&self) -> CostArgumentPresence {
        self.presence
    }
    pub const fn bounded_integer(&self) -> Option<u64> {
        self.bounded_integer
    }
}

/// Cost view for one field occurrence after fragment expansion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostFieldMetadata {
    coordinate: String,
    arguments: Vec<CostArgumentMetadata>,
    parent_index: Option<usize>,
}

impl CostFieldMetadata {
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }
    pub fn arguments(&self) -> &[CostArgumentMetadata] {
        &self.arguments
    }
    pub fn argument(&self, name: &str) -> Option<&CostArgumentMetadata> {
        self.arguments.iter().find(|argument| argument.name == name)
    }
    /// Returns the parent field index, if any.
    pub const fn parent_index(&self) -> Option<usize> {
        self.parent_index
    }
}

struct PendingSelection<'a> {
    selection: &'a Selection,
    parent_type: &'a str,
    parent_field: Option<usize>,
}

#[doc(hidden)]
pub fn collect_operation_cost_fields(
    schema: &SemanticSchema,
    document: &ValidatedDocument,
    selected: SelectedOperation<'_>,
    variables: &CoercedVariables,
) -> Vec<CostFieldMetadata> {
    collect_operation_cost_fields_bounded(schema, document, selected, variables, None)
        .expect("Unlimited metadata is included.")
}

/// Owns cost metadata while enforcing shared entry and logical-byte limits.
#[doc(hidden)]
pub fn collect_operation_cost_fields_bounded<'a>(
    schema: &'a SemanticSchema,
    document: &'a ValidatedDocument,
    selected: SelectedOperation<'a>,
    variables: &CoercedVariables,
    maximum_bytes: Option<usize>,
) -> Result<Vec<CostFieldMetadata>, CostMetadataLimit> {
    let Some(root) = operation_root(schema, selected.operation.operation_type) else {
        return Ok(Vec::new());
    };
    let mut budget = Budget::new(maximum_bytes);
    let mut output = Vec::with_capacity(selected.complexity.expanded_fields);
    let mut work = Vec::with_capacity(selected.complexity.expanded_fields);
    push_selections(&mut work, &selected.operation.selection_set, root, None);
    while let Some(pending) = work.pop() {
        match pending.selection {
            Selection::Field(field)
                if super::collect::should_include(&field.directives, variables) =>
            {
                collect_field(
                    schema,
                    field,
                    pending,
                    variables,
                    &mut output,
                    &mut work,
                    &mut budget,
                )?;
            }
            Selection::FragmentSpread(spread)
                if super::collect::should_include(&spread.directives, variables) =>
            {
                if let Some(fragment) = document.fragment(&spread.name.value) {
                    push_selections(
                        &mut work,
                        &fragment.selection_set,
                        &fragment.type_condition.value,
                        pending.parent_field,
                    );
                }
            }
            Selection::InlineFragment(fragment)
                if super::collect::should_include(&fragment.directives, variables) =>
            {
                let parent = fragment
                    .type_condition
                    .as_ref()
                    .map_or(pending.parent_type, |condition| condition.value.as_str());
                push_selections(
                    &mut work,
                    &fragment.selection_set,
                    parent,
                    pending.parent_field,
                );
            }
            Selection::Field(_) | Selection::FragmentSpread(_) | Selection::InlineFragment(_) => {}
        }
    }
    Ok(output)
}

fn collect_field<'a>(
    schema: &'a SemanticSchema,
    field: &'a Field,
    pending: PendingSelection<'a>,
    variables: &CoercedVariables,
    output: &mut Vec<CostFieldMetadata>,
    work: &mut Vec<PendingSelection<'a>>,
    budget: &mut Budget,
) -> Result<(), CostMetadataLimit> {
    let parent = pending.parent_type;
    let definition = schema
        .type_by_name(parent)
        .and_then(|ty| ty.field(&field.name.value));
    budget.claim(parent.len() + 1 + field.name.value.len())?;
    let arguments = definition
        .map(|definition| {
            definition
                .arguments
                .iter()
                .map(|argument| {
                    budget.claim(argument.name.value.len() + signature_bytes(&argument.ty))?;
                    Ok(argument_metadata(argument, field, variables))
                })
                .collect::<Result<Vec<_>, CostMetadataLimit>>()
        })
        .transpose()?
        .unwrap_or_default();
    output.push(CostFieldMetadata {
        coordinate: format!("{parent}.{}", field.name.value),
        arguments,
        parent_index: pending.parent_field,
    });
    if let (Some(nested), Some(definition)) = (&field.selection_set, definition) {
        push_selections(
            work,
            nested,
            named_type(&definition.ty),
            Some(output.len() - 1),
        );
    }
    Ok(())
}

fn push_selections<'a>(
    work: &mut Vec<PendingSelection<'a>>,
    selection_set: &'a SelectionSet,
    parent_type: &'a str,
    parent_field: Option<usize>,
) {
    work.extend(
        selection_set
            .selections
            .iter()
            .rev()
            .map(|selection| PendingSelection {
                selection,
                parent_type,
                parent_field,
            }),
    );
}

fn argument_metadata(
    definition: &InputValueDefinition,
    field: &Field,
    variables: &CoercedVariables,
) -> CostArgumentMetadata {
    let provided = field
        .arguments
        .iter()
        .find(|argument| argument.name.value == definition.name.value)
        .map(|argument| &argument.value);
    let (presence, value) = argument_value(provided, definition.default_value.as_ref(), variables);
    CostArgumentMetadata {
        name: definition.name.value.clone(),
        type_signature: type_signature(&definition.ty),
        presence,
        bounded_integer: (named_type(&definition.ty) == "Int")
            .then(|| value.and_then(non_negative_integer))
            .flatten(),
    }
}

fn argument_value<'a>(
    provided: Option<&'a ValueNode>,
    default: Option<&'a ValueNode>,
    variables: &'a CoercedVariables,
) -> (CostArgumentPresence, Option<BoundedValue<'a>>) {
    match provided.map(|node| &node.value) {
        Some(AstValue::Variable(name)) => match variables.get(&name.value) {
            Some(CoercedInput::Value(value)) => (
                CostArgumentPresence::Provided,
                Some(BoundedValue::Runtime(value)),
            ),
            Some(CoercedInput::Null) => (CostArgumentPresence::Provided, None),
            Some(CoercedInput::Undefined) | None => default_value(default),
        },
        Some(value) => (
            CostArgumentPresence::Provided,
            Some(BoundedValue::Literal(value)),
        ),
        None => default_value(default),
    }
}

fn default_value(default: Option<&ValueNode>) -> (CostArgumentPresence, Option<BoundedValue<'_>>) {
    match default {
        Some(value) => (
            CostArgumentPresence::Defaulted,
            Some(BoundedValue::Literal(&value.value)),
        ),
        None => (CostArgumentPresence::Absent, None),
    }
}

enum BoundedValue<'a> {
    Literal(&'a AstValue),
    Runtime(&'a Value),
}

fn non_negative_integer(value: BoundedValue<'_>) -> Option<u64> {
    match value {
        BoundedValue::Literal(AstValue::Int(value)) => value.parse::<u64>().ok(),
        BoundedValue::Runtime(Value::Int(value)) => u64::try_from(*value).ok(),
        _ => None,
    }
}

fn operation_root(
    schema: &SemanticSchema,
    operation: crate::language::ast::OperationType,
) -> Option<&str> {
    let root = match operation {
        crate::language::ast::OperationType::Query => schema.roots().query(),
        crate::language::ast::OperationType::Mutation => schema.roots().mutation(),
        crate::language::ast::OperationType::Subscription => schema.roots().subscription(),
    }?;
    Some(&root.type_name)
}

fn named_type(reference: &TypeRef) -> &str {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return &name.value,
            TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => current = inner,
        }
    }
}

fn type_signature(reference: &TypeRef) -> String {
    let mut text = String::with_capacity(signature_bytes(reference));
    let mut suffix = Vec::new();
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => {
                text.push_str(&name.value);
                break;
            }
            TypeRefKind::List(inner) => {
                text.push('[');
                suffix.push(']');
                current = inner;
            }
            TypeRefKind::NonNull(inner) => {
                suffix.push('!');
                current = inner;
            }
        }
    }
    text.extend(suffix.into_iter().rev());
    text
}
