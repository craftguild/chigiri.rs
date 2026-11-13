use crate::language::ast::{EnumValueDefinition, FieldDefinition, InputValueDefinition, Name};
use std::fmt;

use super::super::{SemanticDirectiveArgument, SemanticDirectiveDefinition, SemanticType};

/// Schema-coordinate syntax defined by GraphQL September 2025 § 2.14.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaCoordinate {
    /// A named type written as `Type`.
    Type {
        /// Type name.
        type_name: Name,
    },
    /// An input field or enum value written as `Type.member`.
    Member {
        /// Name of the type containing the member.
        type_name: Name,
        /// Member name.
        member_name: Name,
    },
    /// A field argument written as `Type.field(argument:)`.
    FieldArgument {
        /// The name of the object or interface that contains the field.
        type_name: Name,
        /// Field name.
        field_name: Name,
        /// The name of the argument.
        argument_name: Name,
    },
    /// A directive definition written as `@directive`.
    Directive {
        /// Directive name without the leading `@`.
        directive_name: Name,
    },
    /// A directive argument written as `@directive(argument:)`.
    DirectiveArgument {
        /// Directive name without the leading `@`.
        directive_name: Name,
        /// The name of the argument.
        argument_name: Name,
    },
}

/// Semantic schema element resolved from a schema coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedSchemaCoordinate<'schema> {
    /// Named type.
    Type(&'schema SemanticType),
    /// Object or interface field.
    Field(&'schema FieldDefinition),
    /// Input-object field.
    InputField(&'schema InputValueDefinition),
    /// Enum value.
    EnumValue(&'schema EnumValueDefinition),
    /// Object or interface field argument.
    FieldArgument(&'schema InputValueDefinition),
    /// Directive definition.
    Directive(&'schema SemanticDirectiveDefinition),
    /// Directive argument definition.
    DirectiveArgument(&'schema SemanticDirectiveArgument),
}

impl fmt::Display for SchemaCoordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type { type_name } => formatter.write_str(&type_name.value),
            Self::Member {
                type_name,
                member_name,
            } => write!(formatter, "{}.{}", type_name.value, member_name.value),
            Self::FieldArgument {
                type_name,
                field_name,
                argument_name,
            } => write!(
                formatter,
                "{}.{}({}:)",
                type_name.value, field_name.value, argument_name.value
            ),
            Self::Directive { directive_name } => write!(formatter, "@{}", directive_name.value),
            Self::DirectiveArgument {
                directive_name,
                argument_name,
            } => write!(
                formatter,
                "@{}({}:)",
                directive_name.value, argument_name.value
            ),
        }
    }
}
