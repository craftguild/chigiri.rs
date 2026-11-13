//! Validate all query inputs before resolving names to slots and borrowed operands.

mod evaluate;
mod operand;
mod predicate;
mod sort;

use super::{StoreError, work::Work};
use crate::api::query::{DIRECTIONS, Direction, Operator, SORT_KEYS, ScalarKind, scalar};
use crate::api::shape;
use crate::api::{COUNT_OPTIONS, FIND_LIMIT, FIND_OFFSET, FIND_OPTIONS, IntegerOption};
use crate::ir::{MockIr, ModelId, SlotId};
use crate::value::{ValueBudget, ValueError};
use chigiri::core::{Object, Value};

pub(super) struct QueryPlan<'a> {
    predicate: Option<Predicate<'a>>,
    order: Vec<SortKey>,
    pub offset: usize,
    pub limit: Option<usize>,
}

enum Predicate<'a> {
    All(Vec<Predicate<'a>>),
    Any(Vec<Predicate<'a>>),
    Leaf {
        slot: SlotId,
        op: Operator,
        value: Operand<'a>,
    },
}

enum Operand<'a> {
    Scalar(Atom<'a>),
    List(Vec<Atom<'a>>),
}

#[derive(Clone, Copy)]
enum Atom<'a> {
    Text(&'a str),
    Number(f64),
    Boolean(bool),
}

struct SortKey {
    slot: SlotId,
    descending: bool,
}

struct Compiler<'a, 'b> {
    ir: &'a MockIr,
    model: ModelId,
    budget: &'b mut ValueBudget,
    work: &'b mut Work,
    nodes: usize,
}

impl<'a> QueryPlan<'a> {
    pub fn build(
        ir: &MockIr,
        model: ModelId,
        options: &'a Object,
        count: bool,
        budget: &mut ValueBudget,
        work: &mut Work,
    ) -> Result<Self, StoreError> {
        if ir.models().get(model.0).is_none() {
            return Err(ValueError("unknown model"));
        }
        let mut compiler = Compiler {
            ir,
            model,
            budget,
            work,
            nodes: 0,
        };
        compiler.object(options, 1)?;
        let specs = if count { COUNT_OPTIONS } else { FIND_OPTIONS };
        for (name, value) in options {
            let spec = specs
                .iter()
                .find(|spec| spec.name == name)
                .ok_or(ValueError("unknown query option"))?;
            if value.is_null() && !spec.nullable {
                return Err(ValueError("null query option"));
            }
        }
        let predicate = match options.get("where") {
            None => None,
            Some(value @ Value::Null) => {
                compiler.budget.node(value, 2)?;
                None
            }
            Some(value) => Some(compiler.predicate(value, 1, 2)?),
        };
        // Validate the closed filter before count, sort, or pagination so these
        // operations cannot hide an invalid predicate.
        let order = compiler.order(options.get("orderBy"))?;
        let offset = compiler
            .integer(options.get("offset"), FIND_OFFSET)?
            .unwrap_or(0);
        let limit = compiler.integer(options.get("limit"), FIND_LIMIT)?;
        Ok(Self {
            predicate,
            order,
            offset,
            limit,
        })
    }
}

impl Compiler<'_, '_> {
    fn object(&mut self, value: &Object, depth: usize) -> Result<(), StoreError> {
        self.budget.container(depth)?;
        self.work.charge(1 + value.len())?;
        for (key, _) in value {
            self.budget.key(key)?;
            self.work.charge(key.len())?;
        }
        Ok(())
    }

    fn slot(&mut self, value: &Value) -> Result<(SlotId, ScalarKind), StoreError> {
        let name = text(value)?;
        self.work.charge(1 + name.len())?;
        let slot = *self.ir.models()[self.model.0]
            .slots
            .get(name)
            .ok_or(ValueError("unknown query field"))?;
        let kind = scalar(self.ir, &self.ir.slots()[slot.0])
            .ok_or(ValueError("query requires simple scalar slot"))?;
        Ok((slot, kind))
    }

    fn integer(
        &mut self,
        value: Option<&Value>,
        bounds: IntegerOption,
    ) -> Result<Option<usize>, StoreError> {
        let Some(value) = value else {
            return Ok(bounds.default);
        };
        self.budget.node(value, 2)?;
        let number = crate::value::exact_number(value)?;
        if number < 0.0 || number > bounds.maximum as f64 || number.fract() != 0.0 {
            return Err(ValueError("invalid query integer option"));
        }
        usize::try_from(number as u64)
            .map(Some)
            .map_err(|_| ValueError("query integer does not fit platform"))
    }

    fn order(&mut self, value: Option<&Value>) -> Result<Vec<SortKey>, StoreError> {
        let Some(Value::List(keys)) = value else {
            return if value.is_none() {
                Ok(Vec::new())
            } else {
                Err(ValueError("sort array required"))
            };
        };
        self.budget.container(2)?;
        if keys.len() > SORT_KEYS {
            return Err(ValueError("sort key limit"));
        }
        let mut order: Vec<SortKey> = Vec::new();
        for key in keys {
            let Value::Object(key) = key else {
                return Err(ValueError("sort object required"));
            };
            self.object(key, 3)?;
            if !shape::ORDER.accepts(key) {
                return Err(ValueError("invalid sort keys"));
            }
            let field = key
                .get(shape::FIELD.name)
                .ok_or(ValueError("sort field required"))?;
            let direction = key
                .get(shape::DIRECTION.name)
                .ok_or(ValueError("sort direction required"))?;
            self.budget.node(field, 4)?;
            self.budget.node(direction, 4)?;
            let (slot, _) = self.slot(field)?;
            if order.iter().any(|key| key.slot == slot) {
                return Err(ValueError("duplicate sort field"));
            }
            let direction = text(direction)?;
            let descending = DIRECTIONS
                .iter()
                .find(|(name, _)| *name == direction)
                .ok_or(ValueError("invalid sort direction"))?
                .1
                == Direction::Descending;
            order.push(SortKey { slot, descending });
        }
        Ok(order)
    }
}

fn text(value: &Value) -> Result<&str, StoreError> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(ValueError("query string required")),
    }
}
