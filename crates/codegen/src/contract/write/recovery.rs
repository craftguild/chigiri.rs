//! Synchronizes one complete recovery archive before deleting the old tree with a single unlink.

use crate::filesystem::{BoundedWriter, CapturedTree, Directory, FsError, OwnedFile};

pub(super) const NAME: &str = ".chigiri-recovery";
pub(super) const MAGIC: &[u8] = b"chigiri/recovery-v1\n";

pub(super) fn create(
    directory: &Directory,
    output: &str,
    tree: &CapturedTree,
) -> Result<OwnedFile, FsError> {
    directory.create_stream(NAME, 68 * 1024 * 1024, |writer| {
        writer.write(MAGIC)?;
        writer.write(&(output.len() as u32).to_be_bytes())?;
        writer.write(output.as_bytes())?;
        writer.write(&(tree.files.len() as u64).to_be_bytes())?;
        for (path, bytes) in &tree.files {
            record(writer, path, bytes)?;
        }
        Ok(())
    })
}

fn record(writer: &mut BoundedWriter<'_>, path: &str, bytes: &[u8]) -> Result<(), FsError> {
    crate::filesystem::relative_components(path)?;
    let length = u32::try_from(path.len()).map_err(|_| FsError::Limit)?;
    writer.write(&length.to_be_bytes())?;
    writer.write(&(bytes.len() as u64).to_be_bytes())?;
    writer.write(path.as_bytes())?;
    writer.write(bytes)
}
