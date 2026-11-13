//! Bound a raw source-definition array before decoding its complete outputs.

use luaur::{Table, Value};

use super::{Codec, JsonShape};
use crate::value::ValueError;

impl Codec {
    pub(crate) fn source_array_len(
        &mut self,
        table: &Table,
        max: usize,
    ) -> Result<usize, ValueError> {
        self.run(|this| {
            if this.shape(table)? == Some(JsonShape::Object) {
                return Err(ValueError("source values require an array"));
            }
            this.budget.container(1)?;
            let mut count = 0;
            let mut highest = 0;
            for pair in table.pairs::<Value, Value>() {
                if count == max {
                    return Err(ValueError("source values capacity"));
                }
                this.budget.work.charge(1)?;
                let (key, _) = pair.map_err(|error| this.vm_error(error))?;
                let index = match key {
                    Value::Integer(index) if (1..=max as i64).contains(&index) => index as usize,
                    Value::Number(index)
                        if index.fract() == 0.0 && (1.0..=max as f64).contains(&index) =>
                    {
                        index as usize
                    }
                    _ => return Err(ValueError("invalid source array index")),
                };
                count += 1;
                this.budget.items(count)?;
                highest = highest.max(index);
            }
            if count == 0 || highest != count {
                return Err(ValueError("empty or sparse source values"));
            }
            Ok(count)
        })
    }
}
