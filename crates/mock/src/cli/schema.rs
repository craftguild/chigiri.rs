//! Load explicit schema input without evaluating project configuration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chigiri::core::Source;
use chigiri_mock::ir::MockIr;

use crate::diagnostic::Diagnostic;

pub(super) fn read(paths: &[PathBuf]) -> Result<Arc<MockIr>, Diagnostic> {
    let mut seen = BTreeSet::new();
    let mut sources = Vec::with_capacity(paths.len());
    let mut files = Vec::new();
    let mut remaining = chigiri_mock::schema::MAX_SDL_BYTES;
    for (index, path) in paths.iter().enumerate() {
        let file = crate::files::read(path, remaining)?;
        if !seen.insert(file.identity()) {
            return Err(Diagnostic::new(path, "schema", "duplicate SDL source"));
        }
        remaining -= file.source().len();
        sources.push(Source::new(
            format!("schema/{:03}.graphql", index + 1),
            file.source(),
        ));
        files.push(file);
    }
    let ir = MockIr::parse(sources).map(Arc::new).map_err(|error| {
        let path = error
            .span
            .and_then(|span| paths.get(span.source_id.index()))
            .map_or(Path::new("schema"), PathBuf::as_path);
        Diagnostic::new(path, "schema", error.code)
    })?;
    for file in files {
        file.verify()?;
    }
    Ok(ir)
}
