//! Checks generated-output resource budgets before and after allocation.

mod names;
mod values;

use chigiri_core::language::ast::Directive;

use crate::compile::CompiledSchema;
use crate::error::CodegenError;
use crate::writer::{GeneratedFile, GeneratedItem};

const MAX_GENERATED_ITEMS: usize = 100_000;
const MAX_GENERATED_FILES: usize = 65_536;
const MAX_GENERATED_BYTES: usize = 64 * 1024 * 1024;
const SOURCE_EXPANSION_FACTOR: usize = 8;
const BYTES_PER_SCHEMA_UNIT: usize = 8 * 1024;

pub(super) fn preflight(compiled: &CompiledSchema) -> Result<(), CodegenError> {
    names::validate(&compiled.schema)?;
    let source_bytes = compiled.sources.iter().try_fold(0usize, |total, source| {
        total
            .checked_add(source.contents.len())
            .ok_or_else(limit_error)
    })?;
    let mut units = 1usize;
    for ty in compiled.schema.types() {
        units = add(units, directive_units(ty.directives())?)?;
        if ty.name_span().is_none() {
            continue;
        }
        units = add(
            units,
            1 + ty.implemented_interfaces().len() + ty.union_members().len(),
        )?;
        units = add(units, ty.enum_values().len() + ty.input_fields().len())?;
        for field in ty.fields() {
            units = add(units, 1 + field.arguments.len())?;
            units = add(units, directive_units(&field.directives)?)?;
            for argument in &field.arguments {
                units = add(units, directive_units(&argument.directives)?)?;
                if let Some(value) = &argument.default_value {
                    units = add(units, values::value_units(&value.value)?)?;
                }
            }
        }
        for field in ty.input_fields() {
            units = add(units, directive_units(&field.directives)?)?;
            if let Some(value) = &field.default_value {
                units = add(units, values::value_units(&value.value)?)?;
            }
        }
        for value in ty.enum_values() {
            units = add(units, directive_units(&value.directives)?)?;
        }
    }
    for directive in compiled
        .schema
        .directive_definitions()
        .iter()
        .filter(|definition| definition.source_definition().is_some())
    {
        units = add(
            units,
            1 + directive.arguments().len() + directive.locations().len(),
        )?;
        for argument in directive.arguments() {
            if let Some(value) = &argument.default_value {
                units = add(units, values::value_units(value)?)?;
            }
        }
    }
    units = add(units, directive_units(compiled.schema.schema_directives())?)?;
    let estimate = source_bytes
        .checked_mul(SOURCE_EXPANSION_FACTOR)
        .and_then(|bytes| {
            units
                .checked_mul(BYTES_PER_SCHEMA_UNIT)
                .and_then(|extra| bytes.checked_add(extra))
        })
        .ok_or_else(limit_error)?;
    if estimate > MAX_GENERATED_BYTES {
        return Err(limit_error());
    }
    Ok(())
}

fn directive_units(directives: &[Directive]) -> Result<usize, CodegenError> {
    let mut units = directives.len();
    for directive in directives {
        units = add(units, directive.arguments.len())?;
        for argument in &directive.arguments {
            units = add(units, values::value_units(&argument.value.value)?)?;
        }
    }
    Ok(units)
}

fn add(left: usize, right: usize) -> Result<usize, CodegenError> {
    left.checked_add(right).ok_or_else(limit_error)
}

#[derive(Default)]
pub(super) struct RenderBudget {
    items: usize,
    bytes: usize,
}

impl RenderBudget {
    pub(super) fn reserve(&mut self, items: &[GeneratedItem]) -> Result<(), CodegenError> {
        self.items = self
            .items
            .checked_add(items.len())
            .ok_or_else(limit_error)?;
        let bytes = items.iter().try_fold(0usize, |total, item| {
            total.checked_add(item.byte_len()).ok_or_else(limit_error)
        })?;
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(limit_error)?;
        if self.items > MAX_GENERATED_ITEMS || self.bytes > MAX_GENERATED_BYTES {
            return Err(limit_error());
        }
        Ok(())
    }
}

pub(super) fn validate_rendered_files(files: &[GeneratedFile]) -> Result<(), CodegenError> {
    let bytes = files.iter().try_fold(0usize, |total, file| {
        total
            .checked_add(file.contents.len())
            .ok_or_else(limit_error)
    })?;
    if files.len() > MAX_GENERATED_FILES || bytes > MAX_GENERATED_BYTES {
        return Err(limit_error());
    }
    Ok(())
}

fn limit_error() -> CodegenError {
    CodegenError::new(
        "SF4010",
        format!(
            "generated output exceeds resource limits: items={MAX_GENERATED_ITEMS}, files={MAX_GENERATED_FILES}, bytes={MAX_GENERATED_BYTES}"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_GENERATED_BYTES, RenderBudget};
    use crate::writer::GeneratedItem;

    #[test]
    fn generated_byte_budget_rejects_before_advancing_to_the_next_category() {
        let mut budget = RenderBudget {
            items: 0,
            bytes: MAX_GENERATED_BYTES,
        };
        let error = budget
            .reserve(&[GeneratedItem::new("overflow", "const V: usize = 1;")])
            .unwrap_err();
        assert!(error.message().contains("exceeds resource limits"));
    }
}
