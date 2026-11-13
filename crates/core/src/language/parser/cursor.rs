//! Token cursor and syntax-diagnostic helpers.

use super::{PARSE_UNEXPECTED_TOKEN, Parser};
use crate::Span;
use crate::language::{Token, TokenKind};

impl Parser {
    pub(super) fn peek(&self) -> &Token {
        // Append EOF so normal cursor movement never needs a missing-token fallback.
        self.tokens
            .get(self.position)
            .unwrap_or_else(|| &self.tokens[self.tokens.len() - 1])
    }

    pub(super) fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    pub(super) fn peek_name(&self) -> Option<&str> {
        self.peek_kind().name()
    }

    pub(super) fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind)
    }

    pub(super) fn at_keyword(&self, keyword: &str) -> bool {
        self.peek_name() == Some(keyword)
    }

    pub(super) fn at_eof(&self) -> bool {
        self.peek_kind().is_eof()
    }

    pub(super) fn consume(&mut self, kind: &TokenKind) -> Option<Token> {
        self.at(kind).then(|| self.advance())
    }

    pub(super) fn consume_keyword(&mut self, keyword: &str) -> Option<Token> {
        self.at_keyword(keyword).then(|| self.advance())
    }

    pub(super) fn expect(&mut self, kind: &TokenKind, expected: &str) -> Option<Token> {
        self.consume(kind).or_else(|| {
            self.unexpected(expected);
            None
        })
    }

    pub(super) fn expect_keyword(&mut self, keyword: &str) -> Option<Token> {
        self.consume_keyword(keyword).or_else(|| {
            self.unexpected(keyword);
            None
        })
    }

    pub(super) fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        self.previous_end = token.span.end;
        if !token.kind.is_eof() {
            self.position += 1;
        }
        token
    }

    pub(super) fn position(&self) -> usize {
        self.position
    }

    pub(super) fn current_start(&self) -> usize {
        self.peek().span.start
    }

    pub(super) fn last_end(&self) -> usize {
        self.previous_end
    }

    pub(super) fn span_from(&self, start: usize) -> Span {
        Span::new(self.source_id, start, self.previous_end.max(start))
    }

    pub(super) fn can_continue(&self) -> bool {
        !self.diagnostic_limit_reached && !self.fatal_error
    }

    pub(super) fn unexpected(&mut self, expected: &str) {
        let message = format!("Expected {expected}, found {}", self.token_label());
        self.report(PARSE_UNEXPECTED_TOKEN, message, self.peek().span);
    }

    pub(super) fn ensure_progress(&mut self, initial_position: usize) {
        if self.position == initial_position && self.can_continue() && !self.at_eof() {
            self.advance();
        }
    }

    fn token_label(&self) -> String {
        match self.peek_kind() {
            TokenKind::Name(value)
            | TokenKind::Int(value)
            | TokenKind::Float(value)
            | TokenKind::String(value)
            | TokenKind::BlockString(value) => format!("`{value}`"),
            TokenKind::Eof => "end of document".to_owned(),
            other => format!("`{other:?}`"),
        }
    }
}
