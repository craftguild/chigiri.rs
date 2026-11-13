//! Strict schema-coordinate grammar from GraphQL September 2025 § 2.14.

mod model;
mod parser;
mod resolver;

pub use model::{ResolvedSchemaCoordinate, SchemaCoordinate};
pub use parser::parse_schema_coordinate;
pub use resolver::resolve_schema_coordinate;

use crate::DiagnosticCode;

/// The input does not match the schema-coordinate grammar.
pub const SCHEMA_COORDINATE_SYNTAX: DiagnosticCode = DiagnosticCode::new("SF2060");
/// Resolves data at a coordinate in the completed schema.
pub const SCHEMA_COORDINATE_TRAILING_INPUT: DiagnosticCode = DiagnosticCode::new("SF2061");
/// There are no non-leaf elements that need to be solved in schema coordinate.
pub const SCHEMA_COORDINATE_CONTAINER_MISSING: DiagnosticCode = DiagnosticCode::new("SF2062");
/// The schema coordinate container is not the type type required.
pub const SCHEMA_COORDINATE_INVALID_CONTAINER: DiagnosticCode = DiagnosticCode::new("SF2063");

#[cfg(test)]
mod tests_parser;
#[cfg(test)]
mod tests_resolver;
