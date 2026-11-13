//! Unix descriptor-relative filesystem backend.

mod directory;
mod file;
mod lock;
mod owned_file;
mod project;
mod remove;
mod snapshot;
mod temporary;
mod tree;
mod write_tree;

pub use directory::Directory;
pub use directory::mutation::RenameFailure;
pub use lock::DirectoryLock;
pub use owned_file::{BoundedWriter, OwnedFile};
pub use project::{DirectoryCreationFailure, FileObservation, ObservedFile};
pub use snapshot::{FileSnapshot, SnapshotMoveFailure};
pub use temporary::TemporaryFile;

use super::{CapturedTree, FsError, TreeLimits, relative_components};

fn os_error(error: rustix::io::Errno) -> FsError {
    use rustix::io::Errno;
    match error {
        Errno::NOSYS | Errno::OPNOTSUPP => FsError::Unsupported,
        Errno::NOENT => FsError::Missing,
        Errno::EXIST => FsError::Exists,
        Errno::LOOP | Errno::NOTDIR | Errno::ISDIR => FsError::Path,
        _ => FsError::Io,
    }
}
