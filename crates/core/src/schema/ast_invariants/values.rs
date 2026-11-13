//! Iteratively validates value tags, names, numeric lexemes, and nesting invariants.

use crate::language::ast::{Value, ValueNode};
use crate::language::numeric::{valid_float, valid_int};

use super::Validator;

impl Validator<'_> {
    pub(super) fn value(&mut self, value: &ValueNode) {
        let mut work = vec![(value, 0_usize)];
        while let Some((current, nesting)) = work.pop() {
            match &current.value {
                Value::Variable(name) => self.name(name, "variable value"),
                Value::Int(raw) if !valid_int(raw) => {
                    self.invalid("Invalid GraphQL IntValue lexeme", current.span);
                }
                Value::Float(raw) if !valid_float(raw) => {
                    self.invalid("Invalid GraphQL FloatValue lexeme", current.span);
                }
                Value::Enum(name) => {
                    self.name(name, "enum value literal");
                    if matches!(name.value.as_str(), "true" | "false" | "null") {
                        self.invalid(
                            "Boolean and null literals must use their dedicated AST variants",
                            name.span,
                        );
                    }
                }
                Value::List(items) => {
                    if nesting >= self.max_value_nesting {
                        self.nesting("Value", current.span);
                    } else {
                        work.extend(items.iter().rev().map(|item| (item, nesting + 1)));
                    }
                }
                Value::Object(fields) => {
                    if nesting >= self.max_value_nesting {
                        self.nesting("Value", current.span);
                    } else {
                        for field in fields {
                            self.name(&field.name, "input object field");
                        }
                        work.extend(fields.iter().rev().map(|field| (&field.value, nesting + 1)));
                    }
                }
                Value::Int(_)
                | Value::Float(_)
                | Value::String(_)
                | Value::Boolean(_)
                | Value::Null => {}
            }
            if !self.can_continue() {
                return;
            }
        }
    }
}
