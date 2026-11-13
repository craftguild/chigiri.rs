//! Copy the immutable pre-cut fixture into an isolated writable directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct LegacyFixture {
    pub base: PathBuf,
    pub project: PathBuf,
}

impl LegacyFixture {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/mock-legacy-rejection-tests");
        fs::create_dir_all(&parent).unwrap();
        let base = loop {
            let path = parent.join(format!(
                "run-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test setup: {error}"),
            }
        };
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/project/legacy_rejection_tests/fixtures/v8");
        copy_tree(&source, &base);
        Self {
            project: base.join("project"),
            base,
        }
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.project.join(path)).unwrap()
    }

    pub fn write(&self, path: &str, bytes: &str) {
        let path = self.project.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    pub fn tree(&self) -> BTreeMap<String, Vec<u8>> {
        tree(&self.base)
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn collect(root: &Path, path: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), result);
            } else {
                result.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result);
    result
}
