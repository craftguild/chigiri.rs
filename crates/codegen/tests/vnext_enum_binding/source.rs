//! Collects Rust sources for complete generated-file and ownership comparisons.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(super) fn generated_files(directory: &Path) -> BTreeMap<String, String> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(name, fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(directory, directory, &mut files);
    files
}

pub(super) fn joined(files: &BTreeMap<String, String>, prefix: &str) -> String {
    let source: String = files
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(_, source)| source.as_str())
        .collect();
    assert!(!source.is_empty(), "generated source must not be empty");
    source
}

pub(super) fn fingerprint(files: &BTreeMap<String, String>, marker: &str) -> String {
    let source = joined(files, "src/__chigiri/internal/bridge/");
    source
        .split_once(marker)
        .and_then(|(_, rest)| rest.lines().nth(1))
        .expect("expected a fingerprint body.")
        .trim()
        .to_owned()
}
