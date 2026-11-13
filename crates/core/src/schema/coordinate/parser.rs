use crate::language::ast::Name;
use crate::{Diagnostic, SourceId, Span};

use super::{SCHEMA_COORDINATE_SYNTAX, SCHEMA_COORDINATE_TRAILING_INPUT, SchemaCoordinate};

/// Parses a complete schema coordinate with a grammar that permits no ignored tokens.
pub fn parse_schema_coordinate(
    source_id: SourceId,
    source: &str,
) -> Result<SchemaCoordinate, Diagnostic> {
    CoordinateParser::new(source_id, source).parse()
}

struct CoordinateParser<'a> {
    source_id: SourceId,
    source: &'a str,
    position: usize,
}

impl<'a> CoordinateParser<'a> {
    const fn new(source_id: SourceId, source: &'a str) -> Self {
        Self {
            source_id,
            source,
            position: 0,
        }
    }

    fn parse(mut self) -> Result<SchemaCoordinate, Diagnostic> {
        if self.consume(b'@') {
            self.parse_directive()
        } else {
            self.parse_type_or_member()
        }
    }

    fn parse_directive(&mut self) -> Result<SchemaCoordinate, Diagnostic> {
        let directive_name = self.parse_name("directive name")?;
        if self.at_end() {
            return Ok(SchemaCoordinate::Directive { directive_name });
        }
        if !self.consume(b'(') {
            return Err(self.trailing());
        }
        let argument_name = self.parse_name("directive argument name")?;
        self.expect(b':', "':' after directive argument name")?;
        self.expect(b')', "')' after directive argument")?;
        self.expect_end()?;
        Ok(SchemaCoordinate::DirectiveArgument {
            directive_name,
            argument_name,
        })
    }

    fn parse_type_or_member(&mut self) -> Result<SchemaCoordinate, Diagnostic> {
        let type_name = self.parse_name("type name")?;
        if self.at_end() {
            return Ok(SchemaCoordinate::Type { type_name });
        }
        if !self.consume(b'.') {
            return Err(self.trailing());
        }
        let member_name = self.parse_name("member name")?;
        if self.at_end() {
            return Ok(SchemaCoordinate::Member {
                type_name,
                member_name,
            });
        }
        if !self.consume(b'(') {
            return Err(self.trailing());
        }
        let argument_name = self.parse_name("field argument name")?;
        self.expect(b':', "':' after field argument name")?;
        self.expect(b')', "')' after field argument")?;
        self.expect_end()?;
        Ok(SchemaCoordinate::FieldArgument {
            type_name,
            field_name: member_name,
            argument_name,
        })
    }

    fn parse_name(&mut self, expected: &str) -> Result<Name, Diagnostic> {
        let start = self.position;
        let Some(first) = self.peek() else {
            return Err(self.syntax(format!("expected {expected}")));
        };
        if !is_name_start(first) {
            return Err(self.syntax(format!("expected {expected}")));
        }
        self.position += 1;
        while self.peek().is_some_and(is_name_continue) {
            self.position += 1;
        }
        Ok(Name::new(
            self.source[start..self.position].to_owned(),
            Span::new(self.source_id, start, self.position),
        ))
    }

    fn expect(&mut self, byte: u8, expected: &str) -> Result<(), Diagnostic> {
        if self.consume(byte) {
            Ok(())
        } else {
            Err(self.syntax(format!("expected {expected}")))
        }
    }

    fn expect_end(&self) -> Result<(), Diagnostic> {
        if self.at_end() {
            Ok(())
        } else {
            Err(self.trailing())
        }
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }

    fn at_end(&self) -> bool {
        self.position == self.source.len()
    }

    fn syntax(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(SCHEMA_COORDINATE_SYNTAX, message, self.current_span())
    }

    fn trailing(&self) -> Diagnostic {
        Diagnostic::new(
            SCHEMA_COORDINATE_TRAILING_INPUT,
            "unexpected trailing schema coordinate input",
            Span::new(self.source_id, self.position, self.source.len()),
        )
    }

    fn current_span(&self) -> Span {
        let end = self
            .source
            .get(self.position..)
            .and_then(|rest| rest.chars().next())
            .map_or(self.position, |character| {
                self.position + character.len_utf8()
            });
        Span::new(self.source_id, self.position, end)
    }
}

const fn is_name_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

const fn is_name_continue(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}
