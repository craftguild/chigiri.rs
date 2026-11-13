//! Derive the closed active-file layout from IR and reject name or case collisions before placement.

use std::collections::BTreeMap;

use super::ProjectError;
use crate::ir::{MockIr, ModelId, ScriptId, ScriptRole};
use chigiri_host::PortableRelativePath;

/// One active resolver; root and nested use of the same coordinate share this entry.
pub struct ResolverPath {
    /// IR-only script identifier, not encoded in the public path.
    pub script: ScriptId,
    /// Canonical GraphQL coordinate.
    pub coordinate: String,
    /// Canonical project-relative path.
    pub path: PortableRelativePath,
}

/// User-owned JSONL seed path for one storage model.
pub struct SeedPath {
    /// Current IR model position.
    pub model: ModelId,
    /// Canonical project-relative path.
    pub path: PortableRelativePath,
}

/// Closed active-file layout derived without globbing or filesystem discovery.
pub struct Layout {
    /// Required resolvers in IR order.
    pub resolvers: Vec<ResolverPath>,
    /// Stored models in IR order.
    pub seeds: Vec<SeedPath>,
    /// Explicit registered modules, including disabled consumers.
    pub consumers: Vec<crate::events::ResolvedConsumer>,
}

impl Layout {
    /// Reject file, parent-directory, and case-folding collisions across the complete layout.
    pub fn new(ir: &MockIr) -> Result<Self, ProjectError> {
        Self::with_events(ir, &crate::events::EventSettings::default())
    }

    /// Include every configured consumer in the same confined owned inventory.
    pub fn with_events(
        ir: &MockIr,
        events: &crate::events::EventSettings,
    ) -> Result<Self, ProjectError> {
        let mut names = Paths::default();
        let mut resolvers = Vec::new();
        for (id, plan) in ir.scripts().iter().enumerate() {
            let field = &ir.graph().fields()[plan.field.0];
            let parent = &ir.graph().types()[field.parent.0].name;
            component(&field.name, ".luau.dist".len())?;
            let owner = match plan.role {
                ScriptRole::QueryRoot { .. } => "query".to_owned(),
                ScriptRole::MutationRoot { .. } => "mutation".to_owned(),
                ScriptRole::SubscriptionRoot { .. } => "subscription".to_owned(),
                ScriptRole::Object => {
                    component(parent, 0)?;
                    format!("objects/{parent}")
                }
            };
            let path = format!("resolvers/{owner}/{}.luau", field.name);
            let path = super::logical(&path)?;
            names.insert(&path)?;
            resolvers.push(ResolverPath {
                script: ScriptId(id),
                coordinate: format!("{parent}.{}", field.name),
                path,
            });
        }
        let mut seeds = Vec::new();
        for (id, model) in ir.models().iter().enumerate() {
            let name = &ir.graph().types()[model.ty.0].name;
            component(name, ".jsonl".len())?;
            let path = format!("seeds/{name}.jsonl");
            let path = super::logical(&path)?;
            names.insert(&path)?;
            seeds.push(SeedPath {
                model: ModelId(id),
                path,
            });
        }
        let consumers = events
            .resolve(ir)
            .map_err(|_| ProjectError::new("layout", "invalid consumer registration"))?;
        for consumer in &consumers {
            names.insert(consumer.path())?;
        }
        Ok(Self {
            resolvers,
            seeds,
            consumers,
        })
    }
}

pub(super) fn component(name: &str, suffix_bytes: usize) -> Result<(), ProjectError> {
    let lower = name.to_ascii_lowercase();
    let reserved = matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|prefix| {
            lower
                .strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        });
    if name.is_empty()
        || name.len() + suffix_bytes > 255
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || reserved
    {
        return Err(ProjectError::new("layout", "unrepresentable project name"));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Paths(BTreeMap<String, (PortableRelativePath, bool)>);

impl Paths {
    pub fn insert(&mut self, path: &PortableRelativePath) -> Result<(), ProjectError> {
        let mut prefix = String::new();
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let directory = components.peek().is_some();
            if let Some((existing, was_directory)) = self.0.get(&prefix.to_ascii_lowercase()) {
                if existing.as_str() != prefix || !directory || !was_directory {
                    return Err(ProjectError::new("layout", "project path collision"));
                }
            } else {
                self.0.insert(
                    prefix.to_ascii_lowercase(),
                    (super::logical(&prefix)?, directory),
                );
            }
        }
        Ok(())
    }
}
