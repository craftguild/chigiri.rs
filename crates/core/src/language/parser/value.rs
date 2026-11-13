//! Recursive grammar for variable-bearing and constant input values.

use super::{PARSE_UNEXPECTED_TOKEN, Parser};
use crate::language::TokenKind;
use crate::language::ast::{ObjectField, StringStyle, StringValue, Value, ValueNode};

impl Parser {
    pub(super) fn parse_const_value(&mut self) -> Option<ValueNode> {
        self.parse_value(false)
    }

    pub(super) fn parse_value(&mut self, allow_variables: bool) -> Option<ValueNode> {
        let start = self.current_start();
        let value = match self.peek_kind() {
            TokenKind::Dollar => return self.parse_variable_value(allow_variables),
            TokenKind::Int(_) => match self.advance().kind {
                TokenKind::Int(raw) => Value::Int(raw),
                _ => return None,
            },
            TokenKind::Float(_) => match self.advance().kind {
                TokenKind::Float(raw) => Value::Float(raw),
                _ => return None,
            },
            TokenKind::String(_) => match self.advance().kind {
                TokenKind::String(value) => {
                    Value::String(StringValue::new(value, StringStyle::Quoted))
                }
                _ => return None,
            },
            TokenKind::BlockString(_) => match self.advance().kind {
                TokenKind::BlockString(value) => {
                    Value::String(StringValue::new(value, StringStyle::Block))
                }
                _ => return None,
            },
            TokenKind::Name(_) => self.parse_named_value()?,
            TokenKind::BracketLeft => return self.parse_list_value(allow_variables),
            TokenKind::BraceLeft => return self.parse_object_value(allow_variables),
            _ => {
                self.unexpected("an input value");
                return None;
            }
        };
        Some(ValueNode::new(value, self.span_from(start)))
    }

    fn parse_variable_value(&mut self, allow_variables: bool) -> Option<ValueNode> {
        let start = self.current_start();
        self.advance();
        let name = self.parse_name("a variable name")?;
        if !allow_variables {
            self.report(
                PARSE_UNEXPECTED_TOKEN,
                "Variables are not allowed in a constant input value",
                self.span_from(start),
            );
            return None;
        }
        Some(ValueNode::new(Value::Variable(name), self.span_from(start)))
    }

    fn parse_named_value(&mut self) -> Option<Value> {
        let name = self.parse_name("an input value")?;
        Some(match name.value.as_str() {
            "true" => Value::Boolean(true),
            "false" => Value::Boolean(false),
            "null" => Value::Null,
            _ => Value::Enum(name),
        })
    }

    fn parse_list_value(&mut self, allow_variables: bool) -> Option<ValueNode> {
        let start = self.current_start();
        self.nested(|parser| {
            parser.advance();
            let mut values = Vec::new();
            while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::BracketRight)
            {
                let initial = parser.position();
                if let Some(value) = parser.parse_value(allow_variables) {
                    values.push(value);
                }
                parser.ensure_progress(initial);
            }
            parser.expect(&TokenKind::BracketRight, "`]`")?;
            Some(ValueNode::new(Value::List(values), parser.span_from(start)))
        })
    }

    fn parse_object_value(&mut self, allow_variables: bool) -> Option<ValueNode> {
        let start = self.current_start();
        self.nested(|parser| {
            parser.advance();
            let mut fields = Vec::new();
            while parser.can_continue() && !parser.at_eof() && !parser.at(&TokenKind::BraceRight) {
                let initial = parser.position();
                if let Some(field) = parser.parse_object_field(allow_variables) {
                    fields.push(field);
                }
                parser.ensure_progress(initial);
            }
            parser.expect(&TokenKind::BraceRight, "`}`")?;
            Some(ValueNode::new(
                Value::Object(fields),
                parser.span_from(start),
            ))
        })
    }

    fn parse_object_field(&mut self, allow_variables: bool) -> Option<ObjectField> {
        let start = self.current_start();
        let name = self.parse_name("an input object field name")?;
        self.expect(&TokenKind::Colon, "`:`")?;
        let value = self.parse_value(allow_variables)?;
        Some(ObjectField {
            name,
            value,
            span: self.span_from(start),
        })
    }
}
