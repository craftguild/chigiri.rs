//! Borrows the possible-object index from the composed schema.

use std::sync::Arc;

use crate::schema::SemanticSchema;

pub(super) struct PossibleTypes<'schema>(&'schema SemanticSchema);

impl<'schema> PossibleTypes<'schema> {
    pub(super) fn new(schema: &'schema SemanticSchema) -> Self {
        Self(schema)
    }

    pub(super) fn overlap(&self, parent: &str, condition: &str) -> Option<bool> {
        let parent = possible_objects(self.0, parent)?;
        let condition = possible_objects(self.0, condition)?;
        Some(overlap_sorted(&parent, &condition))
    }
}

fn possible_objects(schema: &SemanticSchema, name: &str) -> Option<Arc<[Arc<str>]>> {
    schema.possible_objects(name).cloned().or_else(|| {
        crate::schema::introspection::type_by_name(name)
            .filter(|descriptor| !descriptor.fields.is_empty())
            .map(|descriptor| Arc::from([Arc::from(descriptor.name)]))
    })
}

fn overlap_sorted(left: &[Arc<str>], right: &[Arc<str>]) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}
