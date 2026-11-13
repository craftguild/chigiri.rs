//! Writes and deletes only files present in the captured inventory.

use std::path::Path;

use super::{CapturedTree, Directory, FsError, TreeLimits};

impl Directory {
    /// Exclusively writes a complete validated logical tree.
    pub fn write_tree(&self, tree: &CapturedTree) -> Result<(), FsError> {
        self.write_tree_with(tree, || Ok(()))
    }

    /// Writes a validated tree while checking cancellation before each entry.
    pub fn write_tree_with(
        &self,
        tree: &CapturedTree,
        mut before_write: impl FnMut() -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        // Apply the same fixed limits to an assembled output tree before writing it.
        tree.validate_write()?;
        for path in &tree.directories {
            before_write()?;
            let (parent, name) = self.parent_of(path)?;
            parent.create_directory(&name)?;
        }
        for (path, bytes) in &tree.files {
            before_write()?;
            let (parent, name) = self.parent_of(path)?;
            parent.create(&name, bytes, 64 * 1024 * 1024)?;
        }
        self.verify()?;
        self.sync()
    }

    /// Verify the complete tree, then preserve parent and file identity while removing it.
    pub fn remove_tree(
        &self,
        expected: &CapturedTree,
        before_remove: &mut impl FnMut() -> Result<(), FsError>,
    ) -> Result<(), FsError> {
        let actual = self.capture(TreeLimits::generated(), |_| true)?;
        if &actual != expected {
            return Err(FsError::Changed);
        }
        for (path, bytes) in &expected.files {
            before_remove()?;
            self.remove_matching_file(path, bytes)?;
        }
        for path in expected.directories.iter().rev() {
            before_remove()?;
            self.relative(path)?.remove_empty()?;
        }
        before_remove()?;
        self.remove_empty()
    }

    fn remove_matching_file(&self, path: &str, expected: &[u8]) -> Result<(), FsError> {
        let snapshot = self.read_snapshot(Path::new(path), expected.len())?;
        if snapshot.bytes() != expected {
            return Err(FsError::Changed);
        }
        snapshot.remove()
    }
}
