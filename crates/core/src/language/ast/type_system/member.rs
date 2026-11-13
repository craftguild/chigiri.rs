//! Shared syntax for fields, arguments, input fields, and enum values.

use super::super::{Description, Directive, Name, TypeRef, ValueNode};
use crate::Span;

/// The field definition of an object or interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Field name.
    pub name: Name,
    /// Argument definitions in source order.
    pub arguments: Vec<InputValueDefinition>,
    /// The output type reference of the field.
    pub ty: TypeRef,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// The source range from the description prefix or name to the definition end.
    pub span: Span,
}

/// field argument or input object field definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputValueDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// The name of the argument or input field.
    pub name: Name,
    /// input type reference.
    pub ty: TypeRef,
    /// Default value written in the source.
    pub default_value: Option<ValueNode>,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// The source range from the description prefix or name to the definition end.
    pub span: Span,
}

/// Definition of one enum value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumValueDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Enum value name.
    pub name: Name,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// The source range from the description prefix or name to the definition end.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::InputValueDefinition;
    use crate::language::ast::{Name, TypeRef, Value, ValueNode};
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(4), start, end)
    }

    #[test]
    fn input_value_definitions_preserve_default_numeric_lexemes() {
        let definition = InputValueDefinition {
            description: None,
            name: Name::new("limit".to_owned(), span(0, 5)),
            ty: TypeRef::named(Name::new("Int".to_owned(), span(7, 10)), span(7, 10)),
            default_value: Some(ValueNode::new(
                Value::Int("-2147483649".to_owned()),
                span(13, 24),
            )),
            directives: Vec::new(),
            span: span(0, 24),
        };

        assert!(matches!(
            definition.default_value.map(|value| value.value),
            Some(Value::Int(raw)) if raw == "-2147483649"
        ));
    }
}
