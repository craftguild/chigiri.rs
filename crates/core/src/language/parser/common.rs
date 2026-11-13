//! Shared grammar for names, descriptions, arguments, directives, and type references.

use super::Parser;
use crate::language::TokenKind;
use crate::language::ast::{Argument, Description, Directive, Name, StringStyle, StringValue};
use crate::language::ast::{TypeRef, TypeRefKind};

impl Parser {
    pub(super) fn parse_description(&mut self) -> Option<Description> {
        let token = match self.peek_kind() {
            TokenKind::String(_) | TokenKind::BlockString(_) => self.advance(),
            _ => return None,
        };
        let (value, style) = match token.kind {
            TokenKind::String(value) => (value, StringStyle::Quoted),
            TokenKind::BlockString(value) => (value, StringStyle::Block),
            _ => return None,
        };
        Some(Description::new(StringValue::new(value, style), token.span))
    }

    pub(super) fn parse_name(&mut self, expected: &str) -> Option<Name> {
        match self.peek_kind() {
            TokenKind::Name(_) => {
                let token = self.advance();
                let TokenKind::Name(value) = token.kind else {
                    return None;
                };
                Some(Name::new(value, token.span))
            }
            _ => {
                self.unexpected(expected);
                None
            }
        }
    }

    pub(super) fn parse_arguments(&mut self, is_const: bool) -> Vec<Argument> {
        if self.consume(&TokenKind::ParenLeft).is_none() {
            return Vec::new();
        }
        let mut arguments = Vec::new();
        if self.at(&TokenKind::ParenRight) {
            self.unexpected("one or more arguments");
        }
        while self.can_continue() && !self.at_eof() && !self.at(&TokenKind::ParenRight) {
            let initial = self.position();
            if let Some(argument) = self.parse_argument(is_const) {
                arguments.push(argument);
            }
            self.ensure_progress(initial);
        }
        self.expect(&TokenKind::ParenRight, "`)`");
        arguments
    }

    pub(super) fn parse_directives(&mut self, is_const: bool) -> Vec<Directive> {
        let mut directives = Vec::new();
        while self.can_continue() && self.at(&TokenKind::At) {
            let start = self.current_start();
            self.advance();
            let Some(name) = self.parse_name("a directive name") else {
                self.ensure_progress(self.position());
                continue;
            };
            let arguments = self.parse_arguments(is_const);
            directives.push(Directive {
                name,
                arguments,
                span: self.span_from(start),
            });
        }
        directives
    }

    pub(super) fn parse_type_ref(&mut self) -> Option<TypeRef> {
        let start = self.current_start();
        let nullable = if self.at(&TokenKind::BracketLeft) {
            self.nested(|parser| {
                parser.advance();
                let item = parser.parse_type_ref()?;
                parser.expect(&TokenKind::BracketRight, "`]`")?;
                Some(TypeRef::list(item, parser.span_from(start)))
            })?
        } else {
            let name = self.parse_name("a type name")?;
            TypeRef::named(name, self.span_from(start))
        };
        if self.consume(&TokenKind::Bang).is_none() {
            return Some(nullable);
        }
        let non_null = TypeRef::non_null(nullable, self.span_from(start));
        if self.at(&TokenKind::Bang) {
            self.unexpected("a nullable type after `!`");
            self.advance();
        }
        debug_assert!(matches!(non_null.kind, TypeRefKind::NonNull(_)));
        Some(non_null)
    }

    fn parse_argument(&mut self, is_const: bool) -> Option<Argument> {
        let start = self.current_start();
        let name = self.parse_name("an argument name")?;
        self.expect(&TokenKind::Colon, "`:`")?;
        let value = self.parse_value(!is_const)?;
        Some(Argument {
            name,
            value,
            span: self.span_from(start),
        })
    }
}
