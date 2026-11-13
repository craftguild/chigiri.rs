//! Validate required fields and resolved graphs in Cargo metadata v1.

pub(super) mod runtime;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{CodegenError, Consumer};

const MAX_ENTRIES: usize = 65_536;

#[derive(Deserialize)]
pub(super) struct Metadata {
    pub(super) packages: Vec<Package>,
    pub(super) workspace_members: Vec<String>,
    pub(super) workspace_root: PathBuf,
    pub(super) target_directory: PathBuf,
    pub(super) build_directory: Option<PathBuf>,
    pub(super) resolve: Option<Resolve>,
    version: u32,
    #[serde(skip)]
    package_index: BTreeMap<String, usize>,
    #[serde(skip)]
    node_index: BTreeMap<String, usize>,
    #[serde(skip)]
    member_index: BTreeSet<String>,
}

#[derive(Deserialize)]
pub(super) struct Package {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) id: String,
    #[serde(deserialize_with = "required_source")]
    pub(super) source: Option<String>,
    pub(super) manifest_path: PathBuf,
    pub(super) targets: Vec<Target>,
    pub(super) features: BTreeMap<String, Vec<String>>,
    pub(super) dependencies: Vec<RequestedDependency>,
}

#[derive(Deserialize)]
pub(super) struct RequestedDependency {
    pub(super) name: String,
    pub(super) rename: Option<String>,
    #[serde(deserialize_with = "required_source")]
    pub(super) kind: Option<String>,
    #[serde(deserialize_with = "required_source")]
    pub(super) source: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Target {
    pub(super) src_path: PathBuf,
}

#[derive(Deserialize)]
pub(super) struct Resolve {
    pub(super) nodes: Vec<Node>,
    #[serde(deserialize_with = "required_source")]
    pub(super) root: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Node {
    pub(super) id: String,
    pub(super) deps: Vec<Dependency>,
}

#[derive(Deserialize)]
pub(super) struct Dependency {
    pub(super) name: String,
    pub(super) pkg: String,
    pub(super) dep_kinds: Vec<DependencyKind>,
}

#[derive(Deserialize)]
pub(super) struct DependencyKind {
    #[serde(deserialize_with = "required_source")]
    pub(super) kind: Option<String>,
}

fn required_source<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

impl Metadata {
    pub(super) fn read(bytes: &[u8]) -> Result<Self, CodegenError> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if value.version != 1
            || value.packages.is_empty()
            || value.packages.len() > MAX_ENTRIES
            || value.workspace_members.len() > MAX_ENTRIES
            || !value.workspace_root.is_absolute()
            || !value.target_directory.is_absolute()
            || value
                .build_directory
                .as_ref()
                .is_some_and(|path| !path.is_absolute())
        {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        for package in &value.packages {
            if !ids.insert(&package.id)
                || package.id.is_empty()
                || package.id.len() > 16 * 1024
                || package
                    .source
                    .as_ref()
                    .is_some_and(|source| source.len() > 16 * 1024)
                || !package.manifest_path.is_absolute()
                || package.manifest_path.file_name() != Some(std::ffi::OsStr::new("Cargo.toml"))
                || package.targets.is_empty()
                || package.targets.len() > MAX_ENTRIES
                || package.features.len() > MAX_ENTRIES
                || package.dependencies.len() > MAX_ENTRIES
                || package.dependencies.iter().any(|dependency| {
                    !matches!(dependency.kind.as_deref(), None | Some("dev" | "build"))
                        || dependency
                            .source
                            .as_ref()
                            .is_some_and(|source| source.len() > 16 * 1024)
                })
            {
                return Err(invalid());
            }
        }
        if value.workspace_members.iter().any(|id| !ids.contains(id)) {
            return Err(invalid());
        }
        if let Some(resolve) = &value.resolve {
            if resolve.nodes.len() > MAX_ENTRIES {
                return Err(invalid());
            }
            let mut nodes = BTreeSet::new();
            let mut edges = 0usize;
            for node in &resolve.nodes {
                if !ids.contains(&node.id) || !nodes.insert(&node.id) {
                    return Err(invalid());
                }
                edges = edges.checked_add(node.deps.len()).ok_or_else(invalid)?;
                if edges > MAX_ENTRIES {
                    return Err(invalid());
                }
                for dep in &node.deps {
                    edges = edges.checked_add(dep.dep_kinds.len()).ok_or_else(invalid)?;
                    if edges > MAX_ENTRIES {
                        return Err(invalid());
                    }
                    if !ids.contains(&dep.pkg)
                        || dep.name.is_empty()
                        || dep.name.len() > 255
                        || dep.dep_kinds.is_empty()
                        || dep.dep_kinds.len() > MAX_ENTRIES
                        || dep.dep_kinds.iter().any(|kind| {
                            !matches!(kind.kind.as_deref(), None | Some("dev" | "build"))
                        })
                    {
                        return Err(invalid());
                    }
                }
            }
            if resolve
                .nodes
                .iter()
                .flat_map(|node| &node.deps)
                .any(|dep| !nodes.contains(&dep.pkg))
            {
                return Err(invalid());
            }
        }
        value.package_index = value
            .packages
            .iter()
            .enumerate()
            .map(|(index, package)| (package.id.clone(), index))
            .collect();
        if let Some(resolve) = &value.resolve {
            value.node_index = resolve
                .nodes
                .iter()
                .enumerate()
                .map(|(index, node)| (node.id.clone(), index))
                .collect();
        }
        value.member_index = value.workspace_members.iter().cloned().collect();
        Ok(value)
    }

    pub(super) fn selected(&self, consumer: &Consumer) -> Result<&Package, CodegenError> {
        let selected = self
            .packages
            .iter()
            .filter(|package| {
                package.name == consumer.package && self.member_index.contains(&package.id)
            })
            .collect::<Vec<_>>();
        if selected.len() != 1 {
            return Err(contract(
                "The configured consumer package could not be selected in the workspace.",
            ));
        }
        Ok(selected[0])
    }

    pub(super) fn package(&self, id: &str) -> Result<&Package, CodegenError> {
        self.package_index
            .get(id)
            .and_then(|index| self.packages.get(*index))
            .ok_or_else(invalid)
    }

    pub(super) fn node(&self, id: &str) -> Result<&Node, CodegenError> {
        self.node_index
            .get(id)
            .and_then(|index| self.resolve.as_ref()?.nodes.get(*index))
            .ok_or_else(invalid)
    }

    pub(super) fn direct(
        &self,
        owner: &str,
        alias: &str,
        include_dev: bool,
    ) -> Result<&Package, CodegenError> {
        let matches = self
            .node(owner)?
            .deps
            .iter()
            .filter(|dep| {
                dep.name == alias
                    && dep.dep_kinds.iter().any(|kind| {
                        kind.kind.is_none() || (include_dev && kind.kind.as_deref() == Some("dev"))
                    })
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(contract(
                "The dependency alias under inspection does not resolve to exactly one generated crate.",
            ));
        }
        self.package(&matches[0].pkg)
    }

    pub(super) fn reachable(
        &self,
        root: &str,
        all_targets: bool,
    ) -> Result<BTreeSet<String>, CodegenError> {
        let mut queue = vec![root.to_owned()];
        let mut visited = BTreeSet::new();
        while let Some(id) = queue.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            for dep in &self.node(&id)?.deps {
                if dep.dep_kinds.iter().any(|kind| {
                    kind.kind.is_none()
                        || (all_targets && id == root && kind.kind.as_deref() == Some("dev"))
                }) {
                    queue.push(dep.pkg.clone());
                }
            }
        }
        Ok(visited)
    }
}

pub(super) fn parent(path: &Path) -> Result<&Path, CodegenError> {
    path.parent().ok_or_else(invalid)
}

pub(super) fn invalid() -> CodegenError {
    CodegenError::new(
        "SF4017",
        "Cargo metadata has invalid required fields, a dependency-graph inconsistency, or a resource-limit violation.",
    )
}

pub(super) fn contract(message: &str) -> CodegenError {
    CodegenError::new("SF4015", message)
}
