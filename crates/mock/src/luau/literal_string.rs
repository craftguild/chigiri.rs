//! Decodes literal values using the same byte rules as the pinned Luau parser.

use full_moon::ast::Expression;
use full_moon::tokenizer::{StringLiteralQuoteType, TokenReference, TokenType};
use luaur::ast::records::lexer::Lexer;

pub(super) fn decoded(expression: &Expression) -> Option<(&TokenReference, String)> {
    let Expression::String(token) = expression else {
        return None;
    };
    let TokenType::StringLiteral {
        literal,
        quote_type,
        ..
    } = token.token().token_type()
    else {
        return None;
    };
    let mut bytes = literal.as_bytes().to_vec();
    match quote_type {
        StringLiteralQuoteType::Brackets => Lexer::fixup_multiline_bytes(&mut bytes),
        StringLiteralQuoteType::Single | StringLiteralQuoteType::Double => {
            if !Lexer::fixup_quoted_bytes(&mut bytes) {
                return None;
            }
        }
        _ => return None,
    }
    // Unknown or non-UTF-8 values must not become guessed diagnostic sites.
    Some((token, String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
pub(super) fn singleton(value: &str) -> Option<String> {
    let mut text = super::text::Text::new(super::MAX_GENERATED);
    // A quoted, single-line type literal preserves source line mapping even
    // when the original value used long brackets or escaped physical newlines.
    text.quoted(value).ok()?;
    Some(text.finish())
}
