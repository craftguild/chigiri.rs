//! Host-owned model identity recorded while a VM value is decoded.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::store::Record;

#[derive(Clone, Debug)]
pub(crate) enum RecordOrigin {
    Live(Arc<Record>),
    Captured(Arc<crate::output::CapturedRecord>),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum PathPart {
    Field(String),
    Index(usize),
}

#[derive(Debug, Default)]
pub(crate) struct Provenance {
    records: BTreeMap<Vec<PathPart>, RecordOrigin>,
}

impl Provenance {
    pub(super) fn insert(&mut self, path: &[PathPart], record: RecordOrigin) {
        self.records.insert(path.to_vec(), record);
    }

    pub(crate) fn record(&self, path: &[PathPart]) -> Option<&RecordOrigin> {
        self.records.get(path)
    }
}
