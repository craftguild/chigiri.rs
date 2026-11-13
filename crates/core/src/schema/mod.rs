//! Composes multiple SDL documents into one bounded semantic schema.

mod ast_invariants;
mod builtin_imports;
mod builtins;
mod composer;
mod coordinate;
mod definitions;
mod directive_model;
mod directives;
mod extensions;
// Suppress dead-code warnings for the descriptor itself until WP3/WP5 uses it.
#[allow(dead_code)]
pub(crate) mod introspection;
mod members;
mod model;
mod roots;
mod source_order;
mod validation;

use crate::language::ast::Document;
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits};

pub use coordinate::{
    ResolvedSchemaCoordinate, SCHEMA_COORDINATE_CONTAINER_MISSING,
    SCHEMA_COORDINATE_INVALID_CONTAINER, SCHEMA_COORDINATE_SYNTAX,
    SCHEMA_COORDINATE_TRAILING_INPUT, SchemaCoordinate, parse_schema_coordinate,
    resolve_schema_coordinate,
};
pub use directive_model::{
    SemanticDirectiveArgument, SemanticDirectiveDefinition, SemanticDirectiveLocation,
    SemanticTypeRef, SemanticTypeRefKind,
};
pub use model::{
    OperationRoots, RootOperation, SchemaOrigin, SemanticSchema, SemanticType, TypeKind,
};

/// Schema composition received an executable definition.
pub const SCHEMA_EXECUTABLE_DEFINITION: DiagnosticCode = DiagnosticCode::new("SF2001");
/// The document contains multiple schema definitions.
pub const SCHEMA_DUPLICATE_DEFINITION: DiagnosticCode = DiagnosticCode::new("SF2002");
/// A named type duplicates a built-in or existing definition.
pub const SCHEMA_DUPLICATE_TYPE: DiagnosticCode = DiagnosticCode::new("SF2003");
/// A directive name duplicates a built-in or existing definition.
pub const SCHEMA_DUPLICATE_DIRECTIVE_DEFINITION: DiagnosticCode = DiagnosticCode::new("SF2004");
/// An extension has no corresponding base definition.
pub const SCHEMA_EXTENSION_TARGET_MISSING: DiagnosticCode = DiagnosticCode::new("SF2005");
/// An extension's type kind does not match its target.
pub const SCHEMA_EXTENSION_KIND_MISMATCH: DiagnosticCode = DiagnosticCode::new("SF2006");
/// A field, argument, member, or value name is duplicated.
pub const SCHEMA_DUPLICATE_MEMBER: DiagnosticCode = DiagnosticCode::new("SF2007");
/// A schema root operation mapping is duplicated.
pub const SCHEMA_DUPLICATE_ROOT: DiagnosticCode = DiagnosticCode::new("SF2008");
/// A non-repeatable directive is applied more than once at the same location.
pub const SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION: DiagnosticCode = DiagnosticCode::new("SF2009");
/// The schema query root cannot be resolved.
pub const SCHEMA_QUERY_ROOT_MISSING: DiagnosticCode = DiagnosticCode::new("SF2010");
/// A root mapping refers to an undefined named type.
pub const SCHEMA_ROOT_TYPE_MISSING: DiagnosticCode = DiagnosticCode::new("SF2011");
/// A root operation mapping refers to a non-object type.
pub const SCHEMA_ROOT_TYPE_NOT_OBJECT: DiagnosticCode = DiagnosticCode::new("SF2012");
/// Multiple operations refer to the same root object.
pub const SCHEMA_ROOT_TYPES_NOT_DISTINCT: DiagnosticCode = DiagnosticCode::new("SF2013");
/// Legacy diagnostic code for the former subscription-root rejection.
pub const SCHEMA_SUBSCRIPTION_UNSUPPORTED: DiagnosticCode = DiagnosticCode::new("SF2014");
/// A user-defined name uses the introspection-reserved `__` prefix.
pub const SCHEMA_RESERVED_NAME: DiagnosticCode = DiagnosticCode::new("SF2015");
/// A composite type, enum, or input object has an empty final member set.
pub const SCHEMA_EMPTY_MEMBERS: DiagnosticCode = DiagnosticCode::new("SF2016");
/// A type reference refers to an undefined named type.
pub const SCHEMA_UNKNOWN_TYPE: DiagnosticCode = DiagnosticCode::new("SF2017");
/// A field type reference is not an output type.
pub const SCHEMA_INVALID_OUTPUT_TYPE: DiagnosticCode = DiagnosticCode::new("SF2018");
/// The type reference for an argument or input field is not the input type.
pub const SCHEMA_INVALID_INPUT_TYPE: DiagnosticCode = DiagnosticCode::new("SF2019");
/// A union member is not a defined object type.
pub const SCHEMA_INVALID_UNION_MEMBER: DiagnosticCode = DiagnosticCode::new("SF2020");
/// `@oneOf` is applied outside of the input object definition.
pub const SCHEMA_INVALID_ONE_OF_TARGET: DiagnosticCode = DiagnosticCode::new("SF2021");
/// An extension applies `@oneOf` to an existing input object.
pub const SCHEMA_ONE_OF_EXTENSION: DiagnosticCode = DiagnosticCode::new("SF2022");
/// A oneOf input field is non-null.
pub const SCHEMA_ONE_OF_NON_NULL_FIELD: DiagnosticCode = DiagnosticCode::new("SF2023");
/// A oneOf input field has a default value.
pub const SCHEMA_ONE_OF_DEFAULT_FIELD: DiagnosticCode = DiagnosticCode::new("SF2024");
/// `@specifiedBy` is applied to a type other than a custom scalar.
pub const SCHEMA_INVALID_SPECIFIED_BY_TARGET: DiagnosticCode = DiagnosticCode::new("SF2025");
/// `@deprecated` is applied to the required argument or input field.
pub const SCHEMA_DEPRECATED_REQUIRED_INPUT: DiagnosticCode = DiagnosticCode::new("SF2026");
/// An enum value uses the reserved name `true`, `false`, or `null`.
pub const SCHEMA_RESERVED_ENUM_VALUE: DiagnosticCode = DiagnosticCode::new("SF2027");
/// `implements` refers to an undefined type or a non-interface type.
pub const SCHEMA_INVALID_INTERFACE_TARGET: DiagnosticCode = DiagnosticCode::new("SF2028");
/// An interface directly implements itself.
pub const SCHEMA_INTERFACE_SELF_IMPLEMENTATION: DiagnosticCode = DiagnosticCode::new("SF2029");
/// The interface implementation graph contains a cycle.
pub const SCHEMA_INTERFACE_CYCLE: DiagnosticCode = DiagnosticCode::new("SF2030");
/// An implementor omits a transitive interface.
pub const SCHEMA_TRANSITIVE_INTERFACE_MISSING: DiagnosticCode = DiagnosticCode::new("SF2031");
/// An implementing type does not define a required interface field.
pub const SCHEMA_INTERFACE_FIELD_MISSING: DiagnosticCode = DiagnosticCode::new("SF2032");
/// An implementing field does not define a required interface argument.
pub const SCHEMA_INTERFACE_ARGUMENT_MISSING: DiagnosticCode = DiagnosticCode::new("SF2033");
/// An implementing field's argument type does not match the interface field's
/// argument type.
pub const SCHEMA_INTERFACE_ARGUMENT_TYPE_MISMATCH: DiagnosticCode = DiagnosticCode::new("SF2034");
/// An implementing field adds a required argument absent from the interface.
pub const SCHEMA_INTERFACE_EXTRA_REQUIRED_ARGUMENT: DiagnosticCode = DiagnosticCode::new("SF2035");
/// The return type of the implementor field is not a subtype of the Interface field.
pub const SCHEMA_INTERFACE_RETURN_TYPE_MISMATCH: DiagnosticCode = DiagnosticCode::new("SF2036");
/// Only the implementing field is deprecated; the interface field is not.
pub const SCHEMA_INTERFACE_DEPRECATION_MISMATCH: DiagnosticCode = DiagnosticCode::new("SF2037");
/// An input object contains a cycle consisting only of required singular input
/// fields.
pub const SCHEMA_NON_NULL_INPUT_CYCLE: DiagnosticCode = DiagnosticCode::new("SF2038");
/// Input-object field default values form a cycle through field coordinates.
pub const SCHEMA_INPUT_DEFAULT_CYCLE: DiagnosticCode = DiagnosticCode::new("SF2039");
/// A type-system coordinate applies an undefined directive.
pub const SCHEMA_UNKNOWN_DIRECTIVE: DiagnosticCode = DiagnosticCode::new("SF2040");
/// The directive was applied to locations that were not permitted by definition.
pub const SCHEMA_INVALID_DIRECTIVE_LOCATION: DiagnosticCode = DiagnosticCode::new("SF2041");
/// A directive application specifies the same argument more than once.
pub const SCHEMA_DUPLICATE_DIRECTIVE_ARGUMENT: DiagnosticCode = DiagnosticCode::new("SF2042");
/// A directive application specifies an argument absent from its definition.
pub const SCHEMA_UNKNOWN_DIRECTIVE_ARGUMENT: DiagnosticCode = DiagnosticCode::new("SF2043");
/// A required directive argument without a default is missing.
pub const SCHEMA_MISSING_DIRECTIVE_ARGUMENT: DiagnosticCode = DiagnosticCode::new("SF2044");
/// A type-system directive argument contains a variable where a constant value is required.
pub const SCHEMA_DIRECTIVE_ARGUMENT_VARIABLE: DiagnosticCode = DiagnosticCode::new("SF2045");
/// A directive definition declares no locations.
pub const SCHEMA_DIRECTIVE_LOCATION_MISSING: DiagnosticCode = DiagnosticCode::new("SF2046");
/// A non-null type reference directly wraps another non-null type.
pub const SCHEMA_INVALID_TYPE_WRAPPING: DiagnosticCode = DiagnosticCode::new("SF2047");
/// A public AST value violates GraphQL lexer or parser invariants.
pub const SCHEMA_INVALID_AST_SYNTAX: DiagnosticCode = DiagnosticCode::new("SF2048");
/// A programmatically constructed AST exceeds the parser nesting limit.
pub const SCHEMA_AST_NESTING_LIMIT: DiagnosticCode = DiagnosticCode::new("SF2049");
/// The directive definition refers to itself directly or indirectly.
pub const SCHEMA_DIRECTIVE_DEFINITION_CYCLE: DiagnosticCode = DiagnosticCode::new("SF2050");
/// A schema constant value contains a variable.
pub const SCHEMA_CONST_VALUE_VARIABLE: DiagnosticCode = DiagnosticCode::new("SF2051");
/// A null literal was provided for a non-null input.
pub const SCHEMA_CONST_NULL_FOR_NON_NULL: DiagnosticCode = DiagnosticCode::new("SF2052");
/// A constant does not satisfy a built-in scalar's type or range constraints.
pub const SCHEMA_CONST_SCALAR_VALUE: DiagnosticCode = DiagnosticCode::new("SF2053");
/// A constant is not a member of the expected enum.
pub const SCHEMA_CONST_ENUM_VALUE: DiagnosticCode = DiagnosticCode::new("SF2054");
/// An input-object constant is not an object literal.
pub const SCHEMA_CONST_INPUT_OBJECT_VALUE: DiagnosticCode = DiagnosticCode::new("SF2055");
/// An input-object literal specifies an undefined field.
pub const SCHEMA_CONST_UNKNOWN_INPUT_FIELD: DiagnosticCode = DiagnosticCode::new("SF2056");
/// An input-object literal specifies the same field more than once.
pub const SCHEMA_CONST_DUPLICATE_INPUT_FIELD: DiagnosticCode = DiagnosticCode::new("SF2057");
/// An input-object literal omits a required field.
pub const SCHEMA_CONST_MISSING_INPUT_FIELD: DiagnosticCode = DiagnosticCode::new("SF2058");
/// OneOf input does not meet the exactly-one or non-null constraints.
pub const SCHEMA_CONST_ONE_OF_VALUE: DiagnosticCode = DiagnosticCode::new("SF2059");

/// Schema composition result and bounded diagnostics.
#[derive(Clone, Debug)]
pub struct SchemaBuildOutput {
    /// Returns the semantic schema only when validation has no errors.
    pub schema: Option<SemanticSchema>,
    /// Diagnostics collected up to the composition limit.
    pub diagnostics: DiagnosticBag,
    diagnostics_truncated: bool,
}

impl SchemaBuildOutput {
    /// Attempts to build a complete semantic schema.
    pub const fn is_valid(&self) -> bool {
        self.schema.is_some()
    }

    /// Return whether composition reached the diagnostic limit.
    pub const fn diagnostics_were_truncated(&self) -> bool {
        self.diagnostics_truncated
    }
}

/// Composes type-system documents in input order and definitions in source order.
///
/// Return the semantic schema only when the entire validation pass succeeds.
/// A partial schema is never exposed.
pub fn compose_schema(documents: &[Document], limits: &ExecutionLimits) -> SchemaBuildOutput {
    composer::compose(documents, limits)
}

#[cfg(test)]
mod tests_ast_invariants;
#[cfg(test)]
mod tests_builtin_imports;
#[cfg(test)]
mod tests_builtins;
#[cfg(test)]
mod tests_composition;
#[cfg(test)]
mod tests_const_values_directives;
#[cfg(test)]
mod tests_const_values_objects;
#[cfg(test)]
mod tests_const_values_scalars;
#[cfg(test)]
mod tests_directive_applications;
#[cfg(test)]
mod tests_directive_cycles;
#[cfg(test)]
mod tests_directive_metadata;
#[cfg(test)]
mod tests_enum_values;
#[cfg(test)]
mod tests_errors;
#[cfg(test)]
mod tests_input_cycles;
#[cfg(test)]
mod tests_input_default_cycles;
#[cfg(test)]
mod tests_interfaces_fields;
#[cfg(test)]
mod tests_interfaces_graph;
#[cfg(test)]
mod tests_introspection_descriptors;
#[cfg(test)]
mod tests_introspection_metadata;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_manual_ast;
#[cfg(test)]
mod tests_one_of;
#[cfg(test)]
mod tests_structural;
