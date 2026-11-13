//! Give nested resolvers one fixed snapshot and a shared monotonic budget.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::{Record, Related, Snapshot, StoreError, query, traversal, work::Work};
use crate::ir::{FieldId, ModelId};
use crate::output::{CapturedRecord, OutputValue};
use crate::value::{ValueBudget, ValueError};

/// Read-only context bound to one snapshot and shared budget.
pub struct Reader {
    snapshot: Snapshot,
    values: ValueBudget,
    work: Work,
    failed: bool,
}

impl Snapshot {
    /// Create a read-only context shared by one query or nested resolution.
    pub fn reader(&self) -> Reader {
        let work = Work::default();
        Reader {
            snapshot: self.clone(),
            values: ValueBudget::with_work(work.clone()),
            work,
            failed: false,
        }
    }
}

impl Reader {
    pub(crate) fn share_work(&mut self, work: Work) {
        self.work = work;
        self.values.share_work(self.work.clone());
    }

    pub(crate) fn value_budget(&mut self) -> &mut ValueBudget {
        &mut self.values
    }

    pub(crate) fn execution_guard(
        &mut self,
        cancel: &crate::cancellation::Cancellation,
        shutdown: &crate::cancellation::Cancellation,
        deadline: std::time::Instant,
    ) {
        self.work.guard(cancel, shutdown, deadline);
        self.values.share_work(self.work.clone());
    }

    /// Return this reader's immutable snapshot.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Return the first captured host failure; later failures cannot replace it.
    pub fn fail(&mut self) {
        self.failed = true;
    }

    /// Compile search input, scan records, and collect the selected page.
    pub fn find(
        &mut self,
        model: ModelId,
        options: &Object,
    ) -> Result<Vec<Arc<Record>>, StoreError> {
        self.ready()?;
        let result = query::find(
            &self.snapshot.ir,
            &self.snapshot.state,
            model,
            options,
            &mut self.values,
            &mut self.work,
        );
        self.checked(result)
    }

    /// Count all matches against the same snapshot and budget.
    pub fn count(&mut self, model: ModelId, options: &Object) -> Result<usize, StoreError> {
        self.ready()?;
        let result = query::count(
            &self.snapshot.ir,
            &self.snapshot.state,
            model,
            options,
            &mut self.values,
            &mut self.work,
        );
        self.checked(result)
    }

    /// Share the relation indexes belonging to this snapshot.
    pub fn related(&mut self, field: FieldId, parent: &Record) -> Result<Related, StoreError> {
        self.ready()?;
        let result = traversal::read(
            &self.snapshot.ir,
            &self.snapshot.state,
            field,
            parent,
            &mut self.work,
        );
        self.checked(result)
    }

    /// Resolve one result with the same query budget and bind its output object to this snapshot.
    pub fn output(&mut self, field: FieldId, value: &Value) -> Result<OutputValue, StoreError> {
        self.ready()?;
        let result = crate::output::validate(&self.snapshot, field, value, &mut self.values);
        self.checked(result)
    }

    /// Project a value obtained from a validated stored slot without granting record identity.
    pub(crate) fn stored_output(
        &mut self,
        field: FieldId,
        value: &Value,
    ) -> Result<OutputValue, crate::output::OutputError> {
        self.ready().map_err(|error| {
            crate::output::OutputError::from(error).with_resource(self.work.failure().is_some())
        })?;
        let result =
            crate::output::validate_stored_detailed(&self.snapshot, field, value, &mut self.values);
        self.failed |= result.is_err();
        result
    }

    pub(crate) fn output_provenance(
        &mut self,
        field: FieldId,
        value: &Value,
        provenance: &crate::codec::Provenance,
    ) -> Result<OutputValue, crate::output::OutputError> {
        self.ready().map_err(|error| {
            crate::output::OutputError::from(error).with_resource(self.work.failure().is_some())
        })?;
        let result = crate::output::validate_provenance_detailed(
            &self.snapshot,
            field,
            value,
            provenance,
            &mut self.values,
        );
        self.failed |= result.is_err();
        result
    }

    /// Traverse relations under the same budget and expose only authoritative record references.
    pub(crate) fn related_output(
        &mut self,
        field: FieldId,
        parent: u64,
    ) -> Result<OutputValue, StoreError> {
        self.ready()?;
        let result = traversal::read_row(
            &self.snapshot.ir,
            &self.snapshot.state,
            field,
            parent,
            &mut self.work,
        );
        let related = self.checked(result)?;
        let result =
            crate::output::related::project(&self.snapshot, field, related, &mut self.values);
        self.checked(result)
    }

    /// Traverse from sealed publication-time keys into records from this fresh snapshot.
    pub(crate) fn captured_related_output(
        &mut self,
        field: FieldId,
        parent: &CapturedRecord,
    ) -> Result<OutputValue, StoreError> {
        self.ready()?;
        let result = traversal::read_captured(
            &self.snapshot.ir,
            &self.snapshot.state,
            field,
            parent,
            &mut self.work,
        );
        let related = self.checked(result)?;
        let result =
            crate::output::related::project(&self.snapshot, field, related, &mut self.values);
        self.checked(result)
    }

    fn ready(&mut self) -> Result<(), StoreError> {
        if self.failed {
            Err(ValueError("host failure prevents execution"))
        } else {
            self.work.charge(0)
        }
    }

    fn checked<T>(&mut self, result: Result<T, StoreError>) -> Result<T, StoreError> {
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
