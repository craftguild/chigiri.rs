//! Codegen policy over the shared host filesystem boundary.

#[cfg(test)]
mod mutation_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tree_tests;
#[cfg(test)]
mod write_tests;

pub(crate) use chigiri_host::PortableRelativePath;
pub(crate) use chigiri_host::filesystem::{
    BoundedWriter, CapturedTree, Directory, DirectoryLock, FsError, OwnedFile, RenameFailure,
    ResolvedPath, TreeLimits, relative_components,
};
