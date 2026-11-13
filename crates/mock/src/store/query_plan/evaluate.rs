//! Use one short-circuit evaluator for `find` and `count`, with consistent null and string semantics.

use super::{Atom, Operand, Predicate, QueryPlan};
use crate::api::query::Operator;
use crate::store::{Record, work::Work};
use crate::value::{ValueError, exact_number};
use chigiri::core::Value;
use std::cmp::Ordering;

impl QueryPlan<'_> {
    pub fn matches(&self, record: &Record, work: &mut Work) -> Result<bool, ValueError> {
        work.charge(1)?;
        match &self.predicate {
            None => Ok(true),
            Some(predicate) => predicate.evaluate(record, work),
        }
    }
}

impl Predicate<'_> {
    fn evaluate(&self, record: &Record, work: &mut Work) -> Result<bool, ValueError> {
        work.charge(1)?;
        match self {
            Self::All(children) => {
                for child in children {
                    if !child.evaluate(record, work)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Any(children) => {
                for child in children {
                    if child.evaluate(record, work)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Leaf { slot, op, value } => {
                let stored = record.get(*slot).filter(|value| !value.is_null());
                if *op == Operator::IsNull {
                    return Ok(
                        matches!(value, Operand::Scalar(Atom::Boolean(expected)) if *expected == stored.is_none()),
                    );
                }
                let Some(stored) = stored else {
                    return Ok(false);
                };
                let actual = Atom::stored(stored, work)?;
                match value {
                    Operand::List(candidates) => {
                        for expected in candidates {
                            if actual.compare(*expected, work)? == Ordering::Equal {
                                return Ok(true);
                            }
                        }
                        Ok(false)
                    }
                    Operand::Scalar(expected) => {
                        if *op == Operator::Contains {
                            return actual.contains(*expected, work);
                        }
                        let cmp = actual.compare(*expected, work)?;
                        Ok(match op {
                            Operator::Eq => cmp == Ordering::Equal,
                            Operator::Ne => cmp != Ordering::Equal,
                            Operator::Lt => cmp == Ordering::Less,
                            Operator::Lte => cmp != Ordering::Greater,
                            Operator::Gt => cmp == Ordering::Greater,
                            Operator::Gte => cmp != Ordering::Less,
                            _ => return Err(ValueError("invalid query plan")),
                        })
                    }
                }
            }
        }
    }
}

impl<'a> Atom<'a> {
    pub(super) fn stored(value: &'a Value, work: &mut Work) -> Result<Self, ValueError> {
        match value {
            Value::String(value) => Ok(Self::Text(value)),
            Value::Boolean(value) => Ok(Self::Boolean(*value)),
            value => {
                if let Value::BigInt(raw) = value {
                    work.charge(raw.len())?;
                }
                // Do not narrow integers unconditionally; compare using the same exact representation used by storage.
                exact_number(value).map(Self::Number)
            }
        }
    }

    pub(super) fn compare(self, other: Self, work: &mut Work) -> Result<Ordering, ValueError> {
        work.charge(1)?;
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => {
                work.charge(a.len() + b.len())?;
                Ok(a.as_bytes().cmp(b.as_bytes()))
            }
            (Self::Boolean(a), Self::Boolean(b)) => Ok(a.cmp(&b)),
            (Self::Number(a), Self::Number(b)) => {
                a.partial_cmp(&b).ok_or(ValueError("nonfinite comparison"))
            }
            _ => Err(ValueError("inconsistent query scalar")),
        }
    }

    fn contains(self, other: Self, work: &mut Work) -> Result<bool, ValueError> {
        let (Self::Text(haystack), Self::Text(needle)) = (self, other) else {
            return Err(ValueError("contains string required"));
        };
        work.charge(1 + haystack.len() + needle.len())?;
        if needle.is_empty() {
            return Ok(true);
        }
        // Precharge each bounded byte-window comparison; UTF-8 substring matching is byte-equivalent here.
        for window in haystack.as_bytes().windows(needle.len()) {
            work.charge(needle.len())?;
            if window == needle.as_bytes() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
