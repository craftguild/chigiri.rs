//! Builds self-contained recorded inputs and generated trees for contract checks.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{RenderedContract, ResolvedManifest};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(in crate::contract) struct Fixture {
    pub(in crate::contract) root: PathBuf,
    pub(in crate::contract) manifest: ResolvedManifest,
}

impl Fixture {
    pub(in crate::contract) fn new() -> Self {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = repository.join("target/sibling-generator").join(format!(
            "check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("graphql")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        fs::write(
            root.join("graphql/schema.graphql"),
            "type Query { value: String }",
        )
        .unwrap();
        fs::write(
            root.join("chigiri.toml"),
            format!(
                "{}\n[runtime]\nsource=\"path\"\nroot=\"../../..\"\n",
                crate::MANIFEST_TEMPLATE
            ),
        )
        .unwrap();
        let manifest = ResolvedManifest::load(&root.join("chigiri.toml")).unwrap();
        Self { root, manifest }
    }

    pub(in crate::contract) fn output(&self) -> PathBuf {
        self.root.join(self.manifest.manifest().output())
    }

    pub(in crate::contract) fn write(&self, rendered: &RenderedContract) {
        let output = self.output();
        if output.exists() {
            fs::remove_dir_all(&output).unwrap();
        }
        for (name, bytes) in rendered.files() {
            let path = output.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    pub(super) fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![self.root.clone()];
        while let Some(path) = pending.pop() {
            if path.is_dir() {
                files.insert(path.clone(), Vec::new());
                for entry in fs::read_dir(path).unwrap() {
                    pending.push(entry.unwrap().path());
                }
            } else {
                files.insert(path.clone(), fs::read(path).unwrap());
            }
        }
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
