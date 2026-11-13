//! Checked accounting for owned metadata retained with a subscription source.

use crate::executable::{CoercedInput, CoercedVariables};
use crate::{Object, Value};

use super::super::{
    ArgumentValues, CostFieldMetadata, ExecutableDirective, ExecutableDirectiveSite,
};
use super::{PreparedOperation, PreparedSubscriptionPlan};

const COLLECTION_ENTRY_BYTES: usize = 16;

/// Checked ADR-0054 Section 9.2 charge for retained strings, bytes, and entries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[doc(hidden)]
pub struct SubscriptionMetadataCharge {
    bytes: usize,
}

impl SubscriptionMetadataCharge {
    /// Starts an empty charge.
    pub const fn new() -> Self {
        Self { bytes: 0 }
    }

    /// Adds exact owned string or byte payload length.
    pub fn add_bytes(&mut self, bytes: usize) -> Option<()> {
        self.bytes = self.bytes.checked_add(bytes)?;
        Some(())
    }

    /// Adds the fixed logical charge for retained collection entries.
    pub fn add_entries(&mut self, entries: usize) -> Option<()> {
        let bytes = entries.checked_mul(COLLECTION_ENTRY_BYTES)?;
        self.add_bytes(bytes)
    }

    /// Adds an owned value without charging fixed-size scalar representations.
    pub fn value(&mut self, value: &Value) -> Option<()> {
        let additional = value_bytes(value)?;
        self.add_bytes(additional)
    }

    /// Adds an owned object, including its entry index and duplicate owned keys.
    pub fn object(&mut self, object: &Object) -> Option<()> {
        let additional = object_bytes(object)?;
        self.add_bytes(additional)
    }

    /// Returns the accumulated exact charge.
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    fn coerced_input(&mut self, value: &CoercedInput) -> Option<()> {
        match value {
            CoercedInput::Value(value) => self.value(value),
            CoercedInput::Undefined | CoercedInput::Null => Some(()),
        }
    }

    fn variables(&mut self, variables: &CoercedVariables) -> Option<()> {
        self.add_entries(variables.iter().len())?;
        for (name, value) in variables.iter() {
            self.add_bytes(name.len())?;
            self.coerced_input(value)?;
        }
        Some(())
    }

    fn dispatch_arguments(&mut self, arguments: &ArgumentValues) -> Option<()> {
        self.add_entries(arguments.iter().len())?;
        for value in arguments.iter() {
            self.coerced_input(value)?;
        }
        Some(())
    }

    fn directives(&mut self, directives: &[ExecutableDirective]) -> Option<()> {
        self.add_entries(directives.len())?;
        for directive in directives {
            self.add_bytes(directive.name().len())?;
            self.object(directive.arguments())?;
            self.directive_site(directive.site())?;
        }
        Some(())
    }

    fn directive_site(&mut self, site: &ExecutableDirectiveSite) -> Option<()> {
        let text = match site {
            ExecutableDirectiveSite::VariableDefinition { variable } => Some(variable),
            ExecutableDirectiveSite::Field { field } => Some(field),
            ExecutableDirectiveSite::FragmentDefinition { fragment }
            | ExecutableDirectiveSite::FragmentSpread { fragment } => Some(fragment),
            ExecutableDirectiveSite::Operation | ExecutableDirectiveSite::InlineFragment => None,
        };
        self.add_bytes(text.map_or(0, String::len))
    }

    fn cost_fields(&mut self, fields: &[CostFieldMetadata]) -> Option<()> {
        self.add_entries(fields.len())?;
        for field in fields {
            self.add_bytes(field.coordinate().len())?;
            self.add_entries(field.arguments().len())?;
            for argument in field.arguments() {
                self.add_bytes(argument.name().len())?;
                self.add_bytes(argument.type_signature().len())?;
            }
        }
        Some(())
    }

    fn operation(&mut self, operation: &PreparedOperation) -> Option<()> {
        self.add_bytes(operation.operation_name.as_deref().map_or(0, str::len))?;
        self.variables(&operation.variables)?;
        self.add_bytes(operation.source.name().len())?;
        self.add_bytes(operation.source.text().len())?;
        self.add_entries(operation.source.line_starts().len())?;
        self.add_bytes(operation.identity.schema_digest().as_bytes().len())?;
        self.add_bytes(operation.identity.key().document_digest().as_bytes().len())?;
        self.add_bytes(
            operation
                .identity
                .key()
                .operation_name()
                .map_or(0, |name| name.as_str().len()),
        )
    }
}

impl PreparedOperation {
    /// Returns the exact owned metadata charged for this immutable operation package.
    #[doc(hidden)]
    pub fn retained_metadata_bytes(&self) -> Option<usize> {
        let mut charge = SubscriptionMetadataCharge::new();
        charge.operation(self)?;
        Some(charge.bytes())
    }
}

impl PreparedSubscriptionPlan {
    /// Returns the exact retained core metadata charge, or `None` on overflow.
    ///
    /// Schema, document, and custom-scalar caches are shared request-independent
    /// allocations and are excluded. Arc controls, allocator slack, fixed Rust
    /// headers, numeric fields, flags, and application values are also excluded.
    /// Source name, query text, line indexes, identity digests and name, coerced
    /// variables, both argument copies, root strings and locations, directives,
    /// and cost metadata are included. A separately retained request query or
    /// request object is a distinct allocation that the caller must also charge.
    pub fn retained_metadata_bytes(&self) -> Option<usize> {
        let mut charge = SubscriptionMetadataCharge::new();
        let root = &self.root;
        charge.operation(&root.operation)?;
        charge.add_bytes(root.parent_type.len())?;
        charge.add_bytes(root.field_name.len())?;
        charge.add_bytes(root.response_key.len())?;
        charge.add_entries(root.locations.len())?;
        charge.object(&root.arguments)?;
        charge.directives(&root.directives)?;
        charge.directives(&root.operation_directives)?;
        charge.cost_fields(&root.cost_fields)?;
        charge.dispatch_arguments(&self.dispatch_arguments)?;
        Some(charge.bytes())
    }
}

fn value_bytes(value: &Value) -> Option<usize> {
    let mut charge = SubscriptionMetadataCharge::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(value) | Value::BigInt(value) => charge.add_bytes(value.len())?,
            Value::List(values) => {
                charge.add_entries(values.len())?;
                pending.extend(values);
            }
            Value::Object(object) => add_object(&mut charge, object, &mut pending)?,
            Value::Null | Value::Boolean(_) | Value::Int(_) | Value::Float(_) => {}
        }
    }
    Some(charge.bytes())
}

fn object_bytes(object: &Object) -> Option<usize> {
    let mut charge = SubscriptionMetadataCharge::new();
    let mut pending = Vec::new();
    add_object(&mut charge, object, &mut pending)?;
    while let Some(value) = pending.pop() {
        charge.value(value)?;
    }
    Some(charge.bytes())
}

fn add_object<'a>(
    charge: &mut SubscriptionMetadataCharge,
    object: &'a Object,
    pending: &mut Vec<&'a Value>,
) -> Option<()> {
    // Object retains one ordered entry and one lookup-index entry, and each
    // collection owns its own copy of the key.
    charge.add_entries(object.len())?;
    charge.add_entries(object.len())?;
    for (key, value) in object {
        charge.add_bytes(key.len())?;
        charge.add_bytes(key.len())?;
        pending.push(value);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::SubscriptionMetadataCharge;
    use crate::{Object, Value};

    #[test]
    fn values_charge_exact_owned_strings_and_physical_collection_entries() {
        let mut nested = Object::new();
        nested.insert("k", Value::BigInt("12345".to_owned()));
        let value = Value::List(vec![
            Value::String("abc".to_owned()),
            Value::Int(-7),
            Value::Object(nested),
        ]);
        let mut charge = SubscriptionMetadataCharge::new();

        assert_eq!(charge.value(&value), Some(()));
        // List: 3 entries. Object: ordered + index entries, two key copies.
        assert_eq!(charge.bytes(), 3 * 16 + 3 + 2 * 16 + 2 + 5);
    }

    #[test]
    fn checked_addition_and_entry_multiplication_never_wrap() {
        let mut charge = SubscriptionMetadataCharge::new();
        assert_eq!(charge.add_bytes(usize::MAX), Some(()));
        assert_eq!(charge.add_bytes(1), None);
        assert_eq!(charge.bytes(), usize::MAX);

        let mut charge = SubscriptionMetadataCharge::new();
        assert_eq!(charge.add_entries(usize::MAX), None);
        assert_eq!(charge.bytes(), 0);
    }
}
