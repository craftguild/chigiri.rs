//! Charge JSON bytes before appending each document row and enforce a separate render cap.

use super::{MAX_REFERENCE_BYTES, ReferenceError};
use serde::Serialize;
use std::io::{self, Write};

pub(super) struct Budget {
    remaining: usize,
}

impl Budget {
    pub fn new<T: Serialize>(base: &T) -> Result<Self, ReferenceError> {
        let mut budget = Self {
            remaining: MAX_REFERENCE_BYTES,
        };
        budget.account(base)?;
        Ok(budget)
    }

    pub fn push<T: Serialize>(
        &mut self,
        target: &mut Vec<T>,
        value: T,
    ) -> Result<(), ReferenceError> {
        if !target.is_empty() {
            self.write_all(b",").map_err(|_| ReferenceError::limit())?;
        }
        self.account(&value)?;
        target.push(value);
        Ok(())
    }

    // Reserve the remaining flat-array capacity before appending the row.
    pub fn reserve_array_item<T: Serialize>(
        &self,
        reserved: &mut usize,
        value: &T,
    ) -> Result<(), ReferenceError> {
        let separator = usize::from(*reserved > 2);
        let remaining = self
            .remaining
            .checked_sub(*reserved + separator)
            .ok_or_else(ReferenceError::limit)?;
        let mut trial = Self { remaining };
        trial.account(value)?;
        *reserved += separator + remaining - trial.remaining;
        Ok(())
    }

    fn account<T: Serialize>(&mut self, value: &T) -> Result<(), ReferenceError> {
        serde_json::to_writer(self, value).map_err(|_| ReferenceError::limit())
    }
}

impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("reference limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct Buffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Buffer {
    pub fn new() -> Self {
        Self::with_limit(MAX_REFERENCE_BYTES)
    }
    pub fn with_limit(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("reference output limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mr_21_budget_accounts_exact_json_before_append_and_output_has_its_own_limit() {
        let mut values: Vec<String> = Vec::new();
        let mut budget = Budget::new(&values).unwrap();
        let unicode = ["résumé", "e\u{301}", "🙂"];
        assert_eq!(unicode.map(str::len), [8, 3, 4]);
        for value in unicode.into_iter().chain(["\u{1b}\n\"", "x"]) {
            budget.push(&mut values, value.into()).unwrap();
            assert_eq!(
                MAX_REFERENCE_BYTES - budget.remaining,
                serde_json::to_vec(&values).unwrap().len()
            );
        }
        budget.remaining = 3;
        let before = values.len();
        assert!(budget.push(&mut values, "xx".into()).is_err());
        assert_eq!(before, values.len());
        let mut out = Buffer::new();
        let block = vec![b'x'; 1024];
        for _ in 0..MAX_REFERENCE_BYTES / block.len() {
            out.write_all(&block).unwrap();
        }
        assert!(out.write_all(b"x").is_err());
        assert_eq!(out.finish().len(), MAX_REFERENCE_BYTES);
    }
}
