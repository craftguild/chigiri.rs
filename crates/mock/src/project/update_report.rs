//! Report placement results and semantic SDL differences without guessing resolver or seed renames.

use std::collections::BTreeSet;

use super::summary::Summary;
use chigiri_host::PortableRelativePath;

/// Placement result for a user file, generated candidate, or proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    /// Create an exclusive candidate for the new coordinate.
    Created,
    /// Replace an unedited user file whose bytes still match the base.
    Replaced,
    /// Adopt user bytes that already match the next generated candidate.
    Adopted,
    /// Preserve a user edit when the generated candidate did not change.
    Kept,
    /// Present the latest candidate as a `.dist` proposal.
    Proposed,
    /// Base, user, and next candidate bytes are identical.
    Unchanged,
    /// Preserve a managed file whose coordinate is no longer in the IR.
    Orphaned,
    /// Remove management metadata for an orphan deleted by the user.
    OrphanRemoved,
    /// Retain an existing `.dist` proposal that differs from the latest candidate.
    StaleProposal,
    /// Create JSONL for the new model.
    SeedCreated,
    /// Preserve an existing JSONL seed without parsing or rewriting it.
    SeedKept,
}

impl UpdateAction {
    /// Stable action label displayed by the CLI.
    pub fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Replaced => "replaced",
            Self::Adopted => "adopted",
            Self::Kept => "kept",
            Self::Proposed => "proposed",
            Self::Unchanged => "unchanged",
            Self::Orphaned => "orphaned",
            Self::OrphanRemoved => "orphan metadata removed",
            Self::StaleProposal => "stale proposal retained",
            Self::SeedCreated => "empty seed created",
            Self::SeedKept => "seed kept",
        }
    }
}

/// Placement event for one logical project file.
#[derive(Clone, Debug)]
pub struct UpdateEvent {
    /// Fixed action.
    pub action: UpdateAction,
    /// Canonical project-relative path.
    pub path: PortableRelativePath,
}

/// Add/remove/change SDL elements. It does not include rename guesses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaChange {
    /// Element kind and canonical name, such as `field/Query.items`.
    pub element: String,
    /// Stable `added`, `removed`, or `changed` classification.
    pub action: &'static str,
}

/// Update report; updating never performs a full validation check.
#[derive(Default)]
pub struct UpdateReport {
    /// Resolver, seed, proposal, and orphan placement events.
    pub events: Vec<UpdateEvent>,
    /// Semantic SDL differences from the previous snapshot.
    pub schema_changes: Vec<SchemaChange>,
    /// Whether the semantic schema changed.
    pub schema_changed: bool,
}

impl UpdateReport {
    pub(super) fn event(&mut self, action: UpdateAction, path: &PortableRelativePath) {
        self.events.push(UpdateEvent {
            action,
            path: path.clone(),
        });
    }

    pub(super) fn compare(&mut self, old: &Summary, next: &Summary) {
        let keys: BTreeSet<_> = old.entries.keys().chain(next.entries.keys()).collect();
        for key in keys {
            let action = match (old.entries.get(key), next.entries.get(key)) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(a), Some(b)) if a != b => "changed",
                _ => continue,
            };
            self.schema_changes.push(SchemaChange {
                element: key.clone(),
                action,
            });
        }
    }
}
