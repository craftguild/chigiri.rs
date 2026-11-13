//! Request-wide budget for default expansion and owned-input value replication.

use std::sync::Mutex;

use crate::language::ast::Value as AstValue;
use crate::{Span, Value};

use super::INPUT_RESOURCE_LIMIT;
use super::engine::{Engine, Source};

/// The coercion work budget shared by one request or start-up test.
///
/// Nodes count each expansion and copy of a value; bytes count the logical size
/// of strings, keys, and numbers. Exhaustion is permanent. This is not a
/// VM-allocator or process-RSS limit.
pub struct InputCoercionBudget {
    remaining: Mutex<Option<(usize, usize)>>,
}

impl InputCoercionBudget {
    /// Create a bounded node/byte budget owned by the caller.
    pub const fn new(nodes: usize, bytes: usize) -> Self {
        Self {
            remaining: Mutex::new(Some((nodes, bytes))),
        }
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.remaining
            .lock()
            .map_or(true, |remaining| remaining.is_none())
    }

    pub(crate) fn claim(&self, nodes: usize, bytes: usize) -> bool {
        let Ok(mut remaining) = self.remaining.lock() else {
            return false;
        };
        let next =
            remaining.and_then(|(n, b)| Some((n.checked_sub(nodes)?, b.checked_sub(bytes)?)));
        *remaining = next;
        next.is_some()
    }

    pub(crate) fn claim_value(&self, value: &Value) -> bool {
        // The iterator stack does not duplicate the entire reference of the wide container, nor does it return any deep object.
        enum Cursor<'a> {
            List(std::slice::Iter<'a, Value>),
            Object(crate::value::ObjectIter<'a>),
        }
        let mut work = vec![Cursor::List(std::slice::from_ref(value).iter())];
        while let Some(cursor) = work.last_mut() {
            let next = match cursor {
                Cursor::List(values) => values.next().map(|value| (0, value)),
                Cursor::Object(fields) => fields.next().map(|(key, value)| (key.len(), value)),
            };
            let Some((key_bytes, value)) = next else {
                work.pop();
                continue;
            };
            let Some(bytes) = key_bytes.checked_add(leaf_bytes(value)) else {
                return false;
            };
            if !self.claim(1, bytes) {
                return false;
            }
            match value {
                Value::List(items) => work.push(Cursor::List(items.iter())),
                Value::Object(fields) => work.push(Cursor::Object(fields.iter())),
                _ => {}
            }
        }
        true
    }
}

impl Engine<'_> {
    pub(super) fn reserve(&mut self, nodes: usize, bytes: usize, span: Option<Span>) -> bool {
        if self.budget.is_none_or(|budget| budget.claim(nodes, bytes)) {
            return true;
        }
        self.report(
            INPUT_RESOURCE_LIMIT,
            "Input coercion resource limit exceeded",
            span,
        );
        self.diagnostic_limit_reached = true;
        false
    }

    pub(super) fn reserve_value(&mut self, value: &Value, span: Option<Span>) -> bool {
        if self.budget.is_none_or(|budget| budget.claim_value(value)) {
            return true;
        }
        self.report(
            INPUT_RESOURCE_LIMIT,
            "Input coercion resource limit exceeded",
            span,
        );
        self.diagnostic_limit_reached = true;
        false
    }
}

pub(super) fn source_bytes(source: &Source<'_>) -> usize {
    match source {
        Source::Raw(Value::Int(value)) => {
            let magnitude = value.unsigned_abs();
            let digits = if magnitude == 0 {
                1
            } else {
                magnitude.ilog10() as usize + 1
            };
            (digits + usize::from(*value < 0)).max(8)
        }
        Source::Raw(value) => leaf_bytes(value),
        Source::Literal(node) => match &node.value {
            AstValue::String(value) => value.value.len(),
            AstValue::Enum(name) => name.value.len(),
            AstValue::Int(raw) => raw.len().max(8),
            AstValue::Float(_) => 8,
            AstValue::Boolean(_) => 1,
            _ => 0,
        },
    }
}

fn leaf_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) | Value::BigInt(value) => value.len(),
        Value::Int(_) | Value::Float(_) => 8,
        Value::Boolean(_) => 1,
        _ => 0,
    }
}
