//! Normalizes toolchain differences in the rustdoc search index.

use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn rustdoc_search_index(root: &Path) -> String {
    let mut indexes = fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("search-index")
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    let split_index = root.join("search.index");
    if split_index.is_dir() {
        collect_javascript(&split_index, &mut indexes);
    }
    indexes.sort();
    assert!(!indexes.is_empty(), "rustdoc search index is missing");
    indexes
        .into_iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect()
}

fn collect_javascript(directory: &Path, indexes: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap().filter_map(Result::ok) {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_javascript(&entry.path(), indexes);
        } else if entry.path().extension().is_some_and(|value| value == "js") {
            indexes.push(entry.path());
        }
    }
}
