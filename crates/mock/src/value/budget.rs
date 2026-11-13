//! Reserve before conversion and share node, logical-byte, and wrapper charges through one budget.

use chigiri::core::Value;

use super::ValueError;
use crate::limits::request::LimitKind;

/// Maximum value depth, counting the root as depth one.
pub const MAX_DEPTH: usize = 32;
/// Maximum number of items in one list or JSON array.
pub const MAX_ITEMS: usize = 1000;
/// Maximum node count for one shared validation.
pub const MAX_NODES: usize =
    crate::limits::request::policy(crate::limits::request::LimitKind::ValueNodes).hard;
/// Logical byte limit for stored values and conversion work, independent of process RSS.
pub const MAX_BYTES: usize =
    crate::limits::request::policy(crate::limits::request::LimitKind::LogicalBytes).hard;
/// Maximum UTF-8 byte length of one string.
pub const MAX_STRING_BYTES: usize = 4096;

/// A shared conversion budget that remains failed after any limit is exceeded.
#[derive(Debug, Default)]
pub struct ValueBudget {
    nodes: usize,
    bytes: usize,
    scratch_bytes: usize,
    failed: bool,
    pub(crate) work: super::work::Work,
}

impl ValueBudget {
    pub(crate) fn with_work(work: super::work::Work) -> Self {
        Self {
            work,
            ..Self::default()
        }
    }

    pub(crate) fn share_work(&mut self, work: super::work::Work) {
        self.work = work;
    }

    /// Number of nodes charged so far.
    pub const fn nodes(&self) -> usize {
        self.nodes
    }

    /// Logical bytes charged for keys, values, and containers.
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes - self.scratch_bytes
    }

    pub(crate) fn node(&mut self, value: &Value, depth: usize) -> Result<(), ValueError> {
        if depth > MAX_DEPTH {
            return self.fail();
        }
        if self.nodes >= MAX_NODES {
            self.work
                .fail_limit(LimitKind::ValueNodes, self.nodes.saturating_add(1));
            return self.fail();
        }
        let bytes = match value {
            Value::String(text) | Value::BigInt(text) => text.len(),
            Value::Int(_) | Value::Float(_) => 8,
            // Charge one byte so nulls and empty containers still consume logical capacity.
            _ => 1,
        };
        self.reserve(1, bytes)?;
        self.nodes += 1;
        Ok(())
    }

    pub(crate) fn key(&mut self, name: &str) -> Result<(), ValueError> {
        self.charge(name.len())
    }

    /// Measure VM strings first, then reserve their copy budget before creating Rust strings.
    pub(crate) fn string_node(&mut self, bytes: usize, depth: usize) -> Result<(), ValueError> {
        if depth > MAX_DEPTH {
            return self.fail();
        }
        if self.nodes >= MAX_NODES {
            self.work
                .fail_limit(LimitKind::ValueNodes, self.nodes.saturating_add(1));
            return self.fail();
        }
        self.reserve(1, bytes)?;
        self.nodes += 1;
        Ok(())
    }

    /// Reserve UTF-8 bytes before copying a VM key.
    pub(crate) fn key_bytes(&mut self, bytes: usize) -> Result<(), ValueError> {
        self.charge(bytes)
    }

    /// Charge one node and one byte for a container.
    pub(crate) fn container(&mut self, depth: usize) -> Result<(), ValueError> {
        self.node(&Value::Null, depth)
    }

    pub(crate) fn items(&mut self, length: usize) -> Result<(), ValueError> {
        if length > MAX_ITEMS {
            return self.fail();
        }
        self.charge(0)
    }

    fn charge(&mut self, bytes: usize) -> Result<(), ValueError> {
        self.reserve(0, bytes)
    }

    fn reserve(&mut self, nodes: usize, bytes: usize) -> Result<(), ValueError> {
        if self.failed {
            return self.fail();
        }
        if bytes > MAX_BYTES.saturating_sub(self.bytes) {
            self.work
                .fail_limit(LimitKind::LogicalBytes, self.bytes.saturating_add(bytes));
            return self.fail();
        }
        self.work.reserve_value(nodes, bytes)?;
        self.bytes += bytes;
        Ok(())
    }

    fn fail(&mut self) -> Result<(), ValueError> {
        self.failed = true;
        self.work.fail();
        Err(ValueError("value resource limit"))
    }

    pub(crate) fn scratch(&mut self, bytes: usize) -> Result<(), ValueError> {
        self.charge(bytes)?;
        self.scratch_bytes += bytes;
        Ok(())
    }

    /// Reserve a complete subtree copy before changing its structure.
    pub(crate) fn reserve_value(&mut self, value: &Value, depth: usize) -> Result<(), ValueError> {
        self.node(value, depth)?;
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    self.key(key)?;
                    self.reserve_value(value, depth + 1)?;
                }
            }
            Value::List(values) => {
                self.items(values.len())?;
                for value in values {
                    self.reserve_value(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
