//! Reserve names and entries for owned cost metadata before allocating copies.

use crate::language::ast::{TypeRef, TypeRefKind};

/// Fixed failure used when complete cost metadata cannot be generated.
#[derive(Clone, Copy, Debug)]
pub struct CostMetadataLimit;

impl std::fmt::Display for CostMetadataLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("operation metadata resource limit")
    }
}
impl std::error::Error for CostMetadataLimit {}

pub(super) struct Budget {
    remaining: Option<usize>,
    nodes: usize,
}

impl Budget {
    pub(super) fn new(bytes: Option<usize>) -> Self {
        Self {
            remaining: bytes,
            nodes: 100_000,
        }
    }
    pub(super) fn claim(&mut self, bytes: usize) -> Result<(), CostMetadataLimit> {
        if let Some(remaining) = &mut self.remaining {
            self.nodes = self.nodes.checked_sub(1).ok_or(CostMetadataLimit)?;
            *remaining = remaining.checked_sub(bytes).ok_or(CostMetadataLimit)?;
        }
        Ok(())
    }
}

pub(super) fn signature_bytes(reference: &TypeRef) -> usize {
    let mut bytes = 0;
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::Named(name) => return bytes + name.value.len(),
            TypeRefKind::List(inner) => {
                bytes += 2;
                current = inner;
            }
            TypeRefKind::NonNull(inner) => {
                bytes += 1;
                current = inner;
            }
        }
    }
}
