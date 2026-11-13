//! Discovers SDL sources from a schema directory in stable order.

mod captured;

pub(crate) use captured::discover_captured;

const MAX_SOURCE_FILES: usize = 1_024;
const MAX_TOTAL_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRECTORY_DEPTH: usize = 64;
const MAX_DIRECTORY_ENTRIES: usize = 65_536;

/// Schema source and its path relative to the root, passed to codegen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveredSource {
    pub(crate) relative_path: crate::filesystem::PortableRelativePath,
    pub(crate) contents: String,
}

#[cfg(test)]
mod tests;
