//! Coerce scalar and enum results into response values.

mod builtin;
mod custom;

pub(super) use builtin::{coerce_builtin_scalar, coerce_enum, is_builtin_scalar};
pub(super) use custom::{CustomScalarOutputError, validate_custom_scalar};

#[cfg(test)]
mod tests;
