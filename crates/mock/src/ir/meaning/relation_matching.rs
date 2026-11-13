//! Pair relation declarations by direction, type, and explicit name without relying on unmarked fields or registration order.

use super::{Build, relation_declarations::Declaration};
use crate::schema::SchemaError;

pub(super) fn resolve(
    build: &Build,
    declarations: &[Declaration],
) -> Result<Vec<Option<usize>>, SchemaError> {
    let mut pairs = vec![None; declarations.len()];
    let mut work = build.graph.startup_work().clone();
    for (index, declaration) in declarations.iter().enumerate() {
        let mut candidates = Vec::new();
        for (other, candidate) in declarations.iter().enumerate() {
            work.charge(1)
                .map_err(|_| build.error(declaration.field, "relation resolution work limit"))?;
            if index == other
                || declaration.owner != candidate.target
                || declaration.target != candidate.owner
                || declaration.name != candidate.name
            {
                continue;
            }
            if !declaration.fields.is_empty() && !candidate.fields.is_empty() {
                if declaration.name.is_some() {
                    return Err(build.error(declaration.field, "named relation has two FK owners"));
                }
                continue;
            }
            if declaration.fields.is_empty()
                && candidate.fields.is_empty()
                && !(declaration.list && candidate.list)
            {
                continue;
            }
            candidates.push(other);
        }
        if candidates.len() > 1 {
            return Err(build.error(
                declaration.field,
                "ambiguous relation requires matching names",
            ));
        }
        pairs[index] = candidates.first().copied();
        if pairs[index].is_none() && declaration.fields.is_empty() {
            return Err(build.error(declaration.field, "relation counterpart is unresolved"));
        }
    }
    for (index, pair) in pairs.iter().enumerate() {
        if pair.is_some_and(|other| pairs[other] != Some(index)) {
            return Err(build.error(
                declarations[index].field,
                "relation counterpart is ambiguous",
            ));
        }
    }
    Ok(pairs)
}
