//! Defines the input value syntax shared by the executable document and type system.

use super::{Name, Spanned};
use crate::Span;

/// The string literal is usually a form or a block form.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StringStyle {
    /// `"..."` format.
    Quoted,
    /// `"""..."""` format.
    Block,
}

/// A string value after escape processing or block-string formatting.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StringValue {
    /// Value decoded according to GraphQL rules.
    pub value: String,
    /// The literal form used in the source.
    pub style: StringStyle,
}

impl StringValue {
    /// Creates a decoded string value with its source style.
    pub fn new(value: impl Into<String>, style: StringStyle) -> Self {
        Self {
            value: value.into(),
            style,
        }
    }
}

/// A description used by executable or type-system syntax, together with the
/// complete string-literal source span.
pub type Description = Spanned<StringValue>;

/// Input value paired with the span of its complete syntax.
pub type ValueNode = Spanned<Value>;

/// GraphQL input-value syntax.
///
/// The AST performs neither numeric range checks nor canonicalization, so it
/// can preserve the lexeme before coercion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// Variable name without the leading `$`.
    Variable(Name),
    /// Integer lexeme before coercion.
    Int(String),
    /// Floating-point lexeme before parsing.
    Float(String),
    /// Decoded string value and original literal spelling.
    String(StringValue),
    /// boolean literal.
    Boolean(bool),
    /// null literal.
    Null,
    /// Name the enum value.
    Enum(Name),
    /// List value preserving source order and duplicates.
    List(Vec<ValueNode>),
    /// Object fields in source order, including duplicates.
    Object(Vec<ObjectField>),
}

/// One object field in an input value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectField {
    /// Field name.
    pub name: Name,
    /// field value.
    pub value: ValueNode,
    /// Source span from the field name through the value.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{ObjectField, Value, ValueNode};
    use crate::language::ast::Name;
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(1), start, end)
    }

    #[test]
    fn numeric_values_preserve_lexemes_before_coercion() {
        let integer = ValueNode::new(Value::Int("-2147483649".to_owned()), span(0, 11));
        let float = ValueNode::new(Value::Float("1.250e+3".to_owned()), span(12, 20));

        assert!(matches!(integer.value, Value::Int(ref raw) if raw == "-2147483649"));
        assert!(matches!(float.value, Value::Float(ref raw) if raw == "1.250e+3"));
    }

    #[test]
    fn object_values_preserve_field_order_and_duplicates() {
        let field = |start: usize, value: &str| ObjectField {
            name: Name::new("key".to_owned(), span(start, start + 3)),
            value: ValueNode::new(Value::Int(value.to_owned()), span(start + 5, start + 6)),
            span: span(start, start + 6),
        };
        let value = Value::Object(vec![field(1, "1"), field(9, "2")]);

        let Value::Object(fields) = value else {
            panic!("expected object value.");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.value, fields[1].name.value);
        assert_ne!(fields[0].span, fields[1].span);
    }
}
