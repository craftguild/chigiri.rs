//! Transport observations used by the differential comparator.

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedResponse {
    pub status: u16,
    pub media_type: String,
    pub headers: BTreeMap<String, Vec<String>>,
    pub body: serde_json::Value,
}

impl ObservedResponse {
    pub fn header_values(&self, name: &str) -> &[String] {
        self.headers.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}
