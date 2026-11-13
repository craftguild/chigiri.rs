//! Source syntax for directive applications and arguments.

use super::{Name, ValueNode};
use crate::Span;

/// One argument to be delivered to directive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Argument {
    /// The name of the argument.
    pub name: Name,
    /// argument value.
    pub value: ValueNode,
    /// Source span from this argument's name through its value.
    pub span: Span,
}

/// A directive application written at an executable or type-system location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Directive {
    /// The name of the directive except `@`.
    pub name: Name,
    /// Arguments in source order, retaining duplicates for validation diagnostics.
    pub arguments: Vec<Argument>,
    /// Source range from `@` to the end of the argument list.
    pub span: Span,
}

/// Alias that makes the directive nature explicit in caller-facing type names.
pub type DirectiveApplication = Directive;

#[cfg(test)]
mod tests {
    use super::{Argument, Directive};
    use crate::language::ast::{Name, Value, ValueNode};
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(0), start, end)
    }

    #[test]
    fn directives_preserve_argument_source_order_and_origin() {
        let argument = |name: &str, value: bool, start| Argument {
            name: Name::new(name.to_owned(), span(start, start + name.len())),
            value: ValueNode::new(
                Value::Boolean(value),
                span(start + name.len() + 2, start + 8),
            ),
            span: span(start, start + 8),
        };
        let directive = Directive {
            name: Name::new("policy".to_owned(), span(1, 7)),
            arguments: vec![argument("read", true, 8), argument("write", false, 18)],
            span: span(0, 31),
        };

        assert_eq!(directive.arguments[0].name.value, "read");
        assert_eq!(directive.arguments[1].name.value, "write");
        assert_eq!(directive.span.source_id, SourceId::new(0));
    }
}
