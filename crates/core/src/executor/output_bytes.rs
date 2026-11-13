//! Reserves request-wide owned-output bytes before retaining type names, keys, or values.

use std::sync::atomic::Ordering;

use crate::{GraphQlResponse, Object, PathSegment, Value};

use super::engine::Engine;

impl<C> Engine<'_, C> {
    pub(super) fn output_bytes_exhausted(&self) -> bool {
        self.dispatch.runtime_byte_limit().is_some()
            && self.output_bytes.load(Ordering::Relaxed) == usize::MAX
    }

    pub(super) fn output_byte_failure(&self) -> Option<GraphQlResponse> {
        self.output_bytes_exhausted().then(|| GraphQlResponse {
            data: Some(Value::Null),
            errors: self
                .output_limit_error(Vec::new(), Vec::new(), true, None, "execution")
                .errors,
            extensions: Object::new(),
        })
    }

    pub(super) fn claim_output_bytes(&self, bytes: usize) -> bool {
        let Some(maximum) = self.dispatch.runtime_byte_limit() else {
            return true;
        };
        let previous = self
            .output_bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(
                    current
                        .checked_add(bytes)
                        .filter(|next| *next <= maximum)
                        .unwrap_or(usize::MAX),
                )
            })
            .unwrap();
        previous
            .checked_add(bytes)
            .is_some_and(|next| next <= maximum)
            && previous != usize::MAX
    }

    pub(super) fn claim_output_field(&self, parent: &[PathSegment], key: &str) -> bool {
        // Reserve copied path segments and object keys; borrowed schema and
        // document strings do not count toward owned output bytes.
        self.claim_output_bytes(key.len())
            && self.claim_output_bytes(key.len())
            && self.claim_output_path(parent)
    }

    pub(super) fn claim_output_path(&self, path: &[PathSegment]) -> bool {
        path.iter().all(|part| {
            self.claim_output_bytes(match part {
                PathSegment::Field(name) => name.len(),
                PathSegment::Index(_) => 8,
            })
        })
    }

    pub(super) fn claim_output_leaf(&self, value: &Value) -> bool {
        if self.dispatch.runtime_byte_limit().is_none() {
            return true;
        }
        let mut work = vec![(value, 0usize)];
        let mut nodes = 0usize;
        while let Some((value, depth)) = work.pop() {
            nodes += 1;
            if depth > self.limits.max_custom_scalar_output_depth {
                return false;
            }
            let bytes = match value {
                Value::String(value) | Value::BigInt(value) => value.len(),
                Value::Int(_) | Value::Float(_) => 8,
                _ => 1,
            };
            if !self.claim_output_bytes(bytes) {
                return false;
            }
            // Before securing the child handle, we adjust the width, including the waiting time, to the existing node limit.
            let width = match value {
                Value::List(items) => items.len(),
                Value::Object(object) => object.len(),
                _ => 0,
            };
            if width
                > self
                    .limits
                    .max_output_nodes
                    .saturating_sub(nodes + work.len())
            {
                return false;
            }
            match value {
                Value::List(items) => work.extend(items.iter().map(|value| (value, depth + 1))),
                Value::Object(object) => {
                    for (key, value) in object {
                        if !self.claim_output_bytes(key.len()) {
                            return false;
                        }
                        work.push((value, depth + 1));
                    }
                }
                _ => {}
            }
        }
        true
    }
}
