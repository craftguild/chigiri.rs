//! Bound table traversal once per conversion and reuse entries for shared DAG nodes.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use luaur::{Table, Value as LuaValue};

use super::observed_vm_error;
use super::{PathPart, Provenance};
use crate::value::{ValueBudget, ValueError};

type Entries = Rc<Vec<(LuaValue, LuaValue)>>;

/// Scan state local to one Lua decode; cached handles never survive into the next host call.
#[derive(Default)]
pub(crate) struct Scan {
    pub active_tables: BTreeSet<usize>,
    pub provenance: Provenance,
    location: Vec<PathPart>,
    tables: BTreeMap<usize, Entries>,
}

impl Scan {
    pub(super) fn enter(&mut self, part: PathPart) {
        self.location.push(part);
    }

    pub(super) fn leave(&mut self) {
        self.location.pop();
    }

    pub(super) fn record(&mut self, record: super::RecordOrigin) {
        self.provenance.insert(&self.location, record);
    }

    pub fn entries(
        &mut self,
        table: &Table,
        budget: &mut ValueBudget,
        depth: usize,
    ) -> Result<Entries, ValueError> {
        let pointer = table.to_pointer() as usize;
        if let Some(entries) = self.tables.get(&pointer) {
            return Ok(entries.clone());
        }
        let mut entries = Vec::new();
        // Charge live value handles to the same node and byte reservation.
        // VM memory is bounded separately. Charge one logical container for every entry,
        // including holes encountered during raw table iteration.
        for pair in table.pairs::<LuaValue, LuaValue>() {
            budget.container(depth + 1)?;
            entries.push(pair.map_err(|error| observed_vm_error(error, &budget.work))?);
        }
        let entries = Rc::new(entries);
        self.tables.insert(pointer, entries.clone());
        Ok(entries)
    }

    #[cfg(test)]
    pub fn tables(&self) -> usize {
        self.tables.len()
    }
}
