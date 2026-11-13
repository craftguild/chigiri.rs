//! Lazily indexes required input-object field positions.

use std::collections::BTreeMap;

use crate::language::ast::TypeRefKind;
use crate::schema::SemanticType;

#[derive(Default)]
pub(super) struct RequiredFieldCache<'a> {
    indices: BTreeMap<&'a str, Vec<usize>>,
}

impl<'a> RequiredFieldCache<'a> {
    /// Processes each field once and then accounts for required-field multiplicity.
    pub(super) fn indices(&mut self, ty: &'a SemanticType) -> Vec<usize> {
        self.indices
            .entry(ty.name())
            .or_insert_with(|| {
                ty.input_fields()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, field)| {
                        (field.default_value.is_none()
                            && matches!(field.ty.kind, TypeRefKind::NonNull(_)))
                        .then_some(index)
                    })
                    .collect()
            })
            .clone()
    }
}
