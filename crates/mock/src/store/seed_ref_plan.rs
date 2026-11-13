//! Reserves a borrowed expansion plan before materializing owned seed values.

use std::borrow::Cow;

use chigiri::core::{Object, Value};

use crate::value::{ValueBudget, ValueError};

pub(super) struct ObjectPlan<'a>(Vec<(Cow<'a, str>, ValuePlan<'a>)>);

pub(super) enum ValuePlan<'a> {
    Borrowed(&'a Value),
    String(&'a str),
    List(Vec<ValuePlan<'a>>),
    Object(ObjectPlan<'a>),
}

pub(super) struct ReservedObject<'a>(ObjectPlan<'a>);

impl<'a> ObjectPlan<'a> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    pub(super) fn push(&mut self, name: Cow<'a, str>, value: ValuePlan<'a>) {
        self.0.push((name, value));
    }

    pub(super) fn reserve(
        self,
        budget: &mut ValueBudget,
        depth: usize,
    ) -> Result<ReservedObject<'a>, ValueError> {
        self.reserve_borrowed(budget, depth)?;
        Ok(ReservedObject(self))
    }

    fn reserve_borrowed(&self, budget: &mut ValueBudget, depth: usize) -> Result<(), ValueError> {
        budget.container(depth)?;
        for (name, value) in &self.0 {
            budget.key(name)?;
            value.reserve(budget, depth + 1)?;
        }
        Ok(())
    }

    fn materialize(self) -> Object {
        self.0
            .into_iter()
            .map(|(name, value)| (name.into_owned(), value.materialize()))
            .collect()
    }
}

impl ValuePlan<'_> {
    fn reserve(&self, budget: &mut ValueBudget, depth: usize) -> Result<(), ValueError> {
        match self {
            Self::Borrowed(value) => budget.reserve_value(value, depth),
            Self::String(text) => budget.string_node(text.len(), depth),
            Self::List(values) => {
                budget.container(depth)?;
                budget.items(values.len())?;
                for value in values {
                    value.reserve(budget, depth + 1)?;
                }
                Ok(())
            }
            Self::Object(fields) => fields.reserve_borrowed(budget, depth),
        }
    }

    fn materialize(self) -> Value {
        match self {
            Self::Borrowed(value) => value.clone(),
            Self::String(text) => Value::from(text),
            Self::List(values) => Value::List(values.into_iter().map(Self::materialize).collect()),
            Self::Object(fields) => Value::Object(fields.materialize()),
        }
    }
}

impl ReservedObject<'_> {
    pub(super) fn materialize(self) -> Object {
        self.0.materialize()
    }
}
