//! Defines the source syntax elements shared by multiple GraphQL grammatical areas.

use crate::Span;

/// A value paired with the source span that produced it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Spanned<T> {
    /// The value from the source.
    pub value: T,
    /// The byte range covers the entire value syntax.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Splits the value from its source span.
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }

    /// Maps the value while preserving its source span.
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned::new(map(self.value), self.span)
    }

    /// Borrow the value while preserving its source span.
    pub const fn as_ref(&self) -> Spanned<&T> {
        Spanned::new(&self.value, self.span)
    }
}

/// GraphQL `Name` and the source range of tokens.
pub type Name = Spanned<String>;

/// The type of operation or schema root.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OperationType {
    /// Borrows the contained value.
    Query,
    /// A write operation whose root fields execute serially.
    Mutation,
    /// The event stream operation. v1 only keeps syntax.
    Subscription,
}

/// This is a recursive GraphQL type reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    /// The structure of the type reference.
    pub kind: TypeRefKind,
    /// Returns the span of the complete named/list/non-null type reference.
    pub span: Span,
}

impl TypeRef {
    /// Creates a named type reference.
    pub const fn named(name: Name, span: Span) -> Self {
        Self {
            kind: TypeRefKind::Named(name),
            span,
        }
    }

    /// Creates a list type reference.
    pub fn list(item: Self, span: Span) -> Self {
        Self {
            kind: TypeRefKind::List(Box::new(item)),
            span,
        }
    }

    /// Creates a non-null type reference.
    ///
    /// Whether the wrapper is permitted is diagnosed by the parser or
    /// validation; the AST itself performs no semantic validation.
    pub fn non_null(nullable: Self, span: Span) -> Self {
        Self {
            kind: TypeRefKind::NonNull(Box::new(nullable)),
            span,
        }
    }
}

/// The structure of the type reference hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeRefKind {
    /// named type.
    Named(Name),
    /// list item type.
    List(Box<TypeRef>),
    /// Inner type of a non-null type reference.
    NonNull(Box<TypeRef>),
}

#[cfg(test)]
mod tests {
    use super::{Name, Spanned, TypeRef, TypeRefKind};
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(3), start, end)
    }

    #[test]
    fn spanned_map_keeps_the_source_range() {
        let source = Spanned::new("User".to_owned(), span(4, 8));
        let length = source.map(|name| name.len());

        assert_eq!(length.value, 4);
        assert_eq!(length.span, span(4, 8));
    }

    #[test]
    fn nested_type_refs_preserve_each_layer_source_range() {
        let name = Name::new("User".to_owned(), span(2, 6));
        let named = TypeRef::named(name, span(2, 6));
        let non_null = TypeRef::non_null(named, span(2, 7));
        let list = TypeRef::list(non_null, span(1, 8));

        let TypeRefKind::List(item) = list.kind else {
            panic!("expected a list type.");
        };
        assert_eq!(item.span, span(2, 7));
        assert!(matches!(item.kind, TypeRefKind::NonNull(_)));
        assert_eq!(list.span, span(1, 8));
    }
}
