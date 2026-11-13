//! GraphQL language processing shared by SDL and executable document.

pub mod ast;

mod lexer;
pub(crate) mod numeric;
mod parser;
mod token;

pub use lexer::{
    LEX_INVALID_CHARACTER, LEX_INVALID_ESCAPE, LEX_INVALID_NUMBER, LEX_INVALID_STRING_CHARACTER,
    LEX_INVALID_UNICODE_ESCAPE, LEX_UNTERMINATED_BLOCK_STRING, LEX_UNTERMINATED_STRING, LexOutput,
    lex,
};
pub use parser::{
    PARSE_DOCUMENT_TOO_LARGE, PARSE_EXECUTABLE_DEFINITION, PARSE_NESTING_LIMIT,
    PARSE_TYPE_SYSTEM_DEFINITION, PARSE_UNEXPECTED_TOKEN, ParseOutput, parse_document,
    parse_executable_document, parse_type_system_document,
};
pub use token::{Token, TokenKind};
