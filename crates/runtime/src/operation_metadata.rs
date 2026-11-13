//! Selects operation metadata for offline catalog governance.

use chigiri_core::executable::{
    CoercedVariables, ComplexitySummary, SelectedOperation, ValidatedDocument,
};
use chigiri_core::executor::{
    CostFieldMetadata, CostMetadataLimit, collect_operation_cost_fields_bounded,
};
use chigiri_core::language::ast::{OperationType, Selection, TypeRef, TypeRefKind};
use chigiri_core::schema::SemanticSchema;
use chigiri_core::{OperationIdentity, ResultShapeDigest};
use sha2::{Digest as _, Sha256};

/// Validated operation identity, structure, and schema result shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedOperation {
    identity: OperationIdentity,
    operation_type: OperationType,
    operation_name: Option<String>,
    complexity: ComplexitySummary,
    result_shape_digest: ResultShapeDigest,
    cost_fields: Vec<CostFieldMetadata>,
}

impl ValidatedOperation {
    pub(crate) fn new(
        identity: OperationIdentity,
        schema: &SemanticSchema,
        document: &ValidatedDocument,
        selected: SelectedOperation<'_>,
        maximum_bytes: Option<usize>,
    ) -> Result<Self, CostMetadataLimit> {
        let cost_fields = collect_operation_cost_fields_bounded(
            schema,
            document,
            selected,
            &CoercedVariables::new(),
            maximum_bytes,
        )?;
        Ok(Self {
            identity,
            operation_type: selected.operation.operation_type,
            operation_name: selected
                .operation
                .name
                .as_ref()
                .map(|name| name.value.clone()),
            complexity: selected.complexity,
            result_shape_digest: result_shape_digest(schema, document, selected),
            cost_fields,
        })
    }

    pub const fn identity(&self) -> &OperationIdentity {
        &self.identity
    }
    pub const fn operation_type(&self) -> OperationType {
        self.operation_type
    }
    pub fn operation_name(&self) -> Option<&str> {
        self.operation_name.as_deref()
    }
    pub const fn complexity(&self) -> ComplexitySummary {
        self.complexity
    }
    pub const fn result_shape_digest(&self) -> ResultShapeDigest {
        self.result_shape_digest
    }
    pub fn cost_fields(&self) -> &[CostFieldMetadata] {
        &self.cost_fields
    }
}

fn result_shape_digest(
    schema: &SemanticSchema,
    document: &ValidatedDocument,
    selected: SelectedOperation<'_>,
) -> ResultShapeDigest {
    let mut digest = Sha256::new();
    hash_str(
        &mut digest,
        operation_kind(selected.operation.operation_type),
    );
    let root = root_type(schema, selected.operation.operation_type).unwrap_or("<missing-root>");
    let mut work = vec![(root, &selected.operation.selection_set)];
    while let Some((parent, selections)) = work.pop() {
        for selection in selections.selections.iter().rev() {
            match selection {
                Selection::Field(field) => {
                    let response_key = field.alias.as_ref().unwrap_or(&field.name).value.as_str();
                    hash_str(&mut digest, "field");
                    hash_str(&mut digest, parent);
                    hash_str(&mut digest, response_key);
                    if field.name.value == "__typename" {
                        hash_str(&mut digest, "String!");
                        continue;
                    }
                    let definition = schema
                        .type_by_name(parent)
                        .and_then(|parent| parent.field(&field.name.value));
                    let Some(definition) = definition else {
                        hash_str(&mut digest, "<missing-field>");
                        continue;
                    };
                    hash_type(&mut digest, &definition.ty);
                    if let Some(nested) = &field.selection_set {
                        work.push((named_type(&definition.ty), nested));
                    }
                }
                Selection::FragmentSpread(spread) => {
                    if let Some(fragment) = document.fragment(&spread.name.value) {
                        hash_str(&mut digest, "fragment");
                        hash_str(&mut digest, &fragment.type_condition.value);
                        work.push((&fragment.type_condition.value, &fragment.selection_set));
                    }
                }
                Selection::InlineFragment(fragment) => {
                    let condition = fragment
                        .type_condition
                        .as_ref()
                        .map_or(parent, |name| name.value.as_str());
                    hash_str(&mut digest, "inline");
                    hash_str(&mut digest, condition);
                    work.push((condition, &fragment.selection_set));
                }
            }
        }
    }
    ResultShapeDigest::from_bytes(digest.finalize().into())
}

fn root_type(schema: &SemanticSchema, operation: OperationType) -> Option<&str> {
    let root = match operation {
        OperationType::Query => schema.roots().query(),
        OperationType::Mutation => schema.roots().mutation(),
        OperationType::Subscription => schema.roots().subscription(),
    }?;
    Some(&root.type_name)
}

const fn operation_kind(operation: OperationType) -> &'static str {
    match operation {
        OperationType::Query => "query",
        OperationType::Mutation => "mutation",
        OperationType::Subscription => "subscription",
    }
}

fn named_type(reference: &TypeRef) -> &str {
    match &reference.kind {
        TypeRefKind::Named(name) => &name.value,
        TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => named_type(inner),
    }
}

fn hash_type(digest: &mut Sha256, reference: &TypeRef) {
    match &reference.kind {
        TypeRefKind::Named(name) => {
            hash_str(digest, "named");
            hash_str(digest, &name.value);
        }
        TypeRefKind::List(inner) => {
            hash_str(digest, "list");
            hash_type(digest, inner);
        }
        TypeRefKind::NonNull(inner) => {
            hash_str(digest, "non-null");
            hash_type(digest, inner);
        }
    }
}

fn hash_str(digest: &mut Sha256, value: &str) {
    digest.update(value.len().to_le_bytes());
    digest.update(value.as_bytes());
}
