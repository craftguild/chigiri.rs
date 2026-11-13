//! Binds Cargo workspace-relative spans to retained directories and caches bounded supplemental source.

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use crate::filesystem::Directory;

pub(super) struct Sources {
    pub(super) root: Directory,
    compiler_root: Directory,
    generated: Directory,
    pub(super) alias: String,
    cache: RefCell<Cache>,
}

#[derive(Default)]
struct Cache {
    files: BTreeMap<String, Entry>,
    bytes: usize,
}

#[derive(Clone)]
struct Entry {
    label: String,
    source: Option<Rc<str>>,
    generated: bool,
}

impl Sources {
    pub(super) fn new(
        root: Directory,
        compiler_root: Directory,
        generated: Directory,
        alias: &str,
    ) -> Self {
        Self {
            root,
            compiler_root,
            generated,
            alias: alias.to_owned(),
            cache: RefCell::default(),
        }
    }

    fn file(&self, value: &str) -> Option<(Directory, String)> {
        if value.len() > 4096 {
            return None;
        }
        let (parent, name) = self
            .compiler_root
            .resolve_file_parent(Path::new(value))
            .ok()?;
        if !self.root.contains_directory(&parent).ok()? {
            return None;
        }
        Some((parent, name))
    }

    fn entry(&self, value: &str) -> Option<Entry> {
        let mut cache = self.cache.borrow_mut();
        if let Some(entry) = cache.files.get(value) {
            return Some(entry.clone());
        }
        if cache.files.len() >= 1024 || value.len() > 4096 {
            return None;
        }
        let entry = self
            .file(value)
            .map(|(parent, name)| {
                let label = self
                    .root
                    .descendant_label(&parent, &name)
                    .map_or_else(|_| "<path>".to_owned(), |path| path.to_string());
                let available = (16 * 1024 * 1024usize)
                    .saturating_sub(cache.bytes)
                    .min(1024 * 1024);
                let source = parent
                    .read(&name, available)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .map(Rc::<str>::from);
                let generated =
                    source.is_some() && self.generated.contains_directory(&parent).unwrap_or(false);
                cache.bytes += source.as_ref().map_or(0, |source| source.len());
                Entry {
                    label,
                    source,
                    generated,
                }
            })
            .unwrap_or(Entry {
                label: "<path>".to_owned(),
                source: None,
                generated: false,
            });
        cache.files.insert(value.to_owned(), entry.clone());
        Some(entry)
    }

    pub(super) fn label(&self, value: &str) -> String {
        self.entry(value)
            .map_or_else(|| "<path>".to_owned(), |entry| entry.label)
    }

    pub(super) fn read(&self, value: &str) -> Option<Rc<str>> {
        self.entry(value)?.source
    }

    pub(super) fn generated(&self, value: &str) -> bool {
        self.entry(value).is_some_and(|entry| entry.generated)
    }
}
