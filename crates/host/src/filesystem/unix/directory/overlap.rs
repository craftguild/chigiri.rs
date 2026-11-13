//! Detects path overlap using existing ancestor inodes and conservative volume comparison for missing suffixes.

use super::Directory;
use crate::filesystem::{FsError, ResolvedPath};

impl Directory {
    /// Reports whether two retained capabilities identify the same directory.
    pub fn same_location(&self, other: &Self) -> Result<bool, FsError> {
        self.verify()?;
        other.verify()?;
        Ok(self.0.identity == other.0.identity)
    }

    /// Reports whether `other` is this directory or a verified descendant.
    pub fn contains_directory(&self, other: &Self) -> Result<bool, FsError> {
        self.verify()?;
        other.verify()?;
        Ok(self.contains(other))
    }

    /// Reports whether two native destinations overlap after no-follow resolution.
    pub fn overlaps_destination(
        &self,
        left: &ResolvedPath,
        right: &ResolvedPath,
    ) -> Result<bool, FsError> {
        let (left, left_ancestor) = self.resolve_with_ancestor(left.as_path())?;
        let (right, right_ancestor) = self.resolve_with_ancestor(right.as_path())?;
        let left_tail = left
            .strip_prefix(&left_ancestor.0.path)
            .map_err(|_| FsError::Path)?;
        let right_tail = right
            .strip_prefix(&right_ancestor.0.path)
            .map_err(|_| FsError::Path)?;
        left_ancestor.verify()?;
        right_ancestor.verify()?;
        if left_ancestor.0.identity == right_ancestor.0.identity {
            // Existing entries use OS-resolved identity; missing suffixes can
            // only be compared lexically for case and Unicode distinctions.
            // Treat ambiguous non-ASCII components as potentially identical.
            let common = left_tail
                .components()
                .zip(right_tail.components())
                .all(|(a, b)| {
                    let a = a.as_os_str().to_string_lossy();
                    let b = b.as_os_str().to_string_lossy();
                    a.eq_ignore_ascii_case(&b) || !a.is_ascii() || !b.is_ascii()
                });
            return Ok(common);
        }
        Ok(
            (left_tail.as_os_str().is_empty() && left_ancestor.contains(&right_ancestor))
                || (right_tail.as_os_str().is_empty() && right_ancestor.contains(&left_ancestor)),
        )
    }

    fn contains(&self, other: &Self) -> bool {
        let mut current = other;
        loop {
            if self.0.identity == current.0.identity {
                return true;
            }
            let Some((parent, _)) = &current.0.parent else {
                return false;
            };
            current = parent;
        }
    }
}
