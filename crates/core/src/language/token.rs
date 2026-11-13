use crate::Span;

/// GraphQL lexical token consumed by the parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    /// The type of token, and the value that is evaluated if necessary.
    pub kind: TokenKind,
    /// The half-open byte range that the token occupies on the source text.
    pub span: Span,
}

impl Token {
    /// Creates a token from its kind and source span.
    pub const fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// GraphQL September 2025 Section 2.1.5 lexical token of the type
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    /// `!`
    Bang,
    /// `$`
    Dollar,
    /// `&`
    Ampersand,
    /// `(`
    ParenLeft,
    /// `)`
    ParenRight,
    /// `...`
    Spread,
    /// `:`
    Colon,
    /// `=`
    Equals,
    /// `@`
    At,
    /// `[`
    BracketLeft,
    /// `]`
    BracketRight,
    /// `{`
    BraceLeft,
    /// `|`
    Pipe,
    /// `}`
    BraceRight,
    /// Name token preserving its ASCII source text.
    Name(String),
    /// Source lexeme of an `IntValue` matching the grammar.
    Int(String),
    /// Source lexeme of a `FloatValue` matching the grammar.
    Float(String),
    /// String value after escape decoding.
    String(String),
    /// Block-string value after indentation normalization.
    BlockString(String),
    /// End of input; its span begins at the byte offset reached by the lexer.
    Eof,
}

impl TokenKind {
    /// Return the value of the name token.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the source lexeme of Int or Float.
    pub fn number_lexeme(&self) -> Option<&str> {
        match self {
            Self::Int(value) | Self::Float(value) => Some(value),
            _ => None,
        }
    }

    /// Usually returns the value of a string or block string.
    pub fn string_value(&self) -> Option<&str> {
        match self {
            Self::String(value) | Self::BlockString(value) => Some(value),
            _ => None,
        }
    }

    /// Creates the token representing the end of the document.
    pub const fn is_eof(&self) -> bool {
        matches!(self, Self::Eof)
    }
}
