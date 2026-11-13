//! Request-scoped loader and cookie-command state.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::{AccessLevel, Item, Viewer};

pub struct ItemLoader {
    source: BTreeMap<String, Item>,
    cache: Mutex<BTreeMap<String, Option<Item>>>,
    source_reads: AtomicUsize,
}

impl ItemLoader {
    pub fn synthetic() -> Self {
        let source = [
            Item {
                id: "item-1".into(),
                title: "Synthetic one".to_owned(),
            },
            Item {
                id: "item-2".into(),
                title: "Synthetic two".to_owned(),
            },
        ]
        .into_iter()
        .map(|item| (item.id.to_string(), item))
        .collect();
        Self {
            source,
            cache: Mutex::new(BTreeMap::new()),
            source_reads: AtomicUsize::new(0),
        }
    }

    pub fn load(&self, id: &chigiri::Id) -> Option<Item> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(item) = cache.get(id.as_str()) {
            return item.clone();
        }
        self.source_reads.fetch_add(1, Ordering::SeqCst);
        let item = self.source.get(id.as_str()).cloned();
        cache.insert(id.to_string(), item.clone());
        item
    }

    #[cfg(test)]
    pub fn source_reads(&self) -> usize {
        self.source_reads.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct CookieCommands {
    values: Mutex<Vec<String>>,
}

impl CookieCommands {
    pub fn record_touch(&self, id: &chigiri::Id) {
        self.values
            .lock()
            .unwrap()
            .push(format!("last_touch={id}; Path=/; HttpOnly"));
    }

    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.values.lock().unwrap())
    }
}

#[derive(Clone)]
pub struct AppContext {
    pub viewer: Option<Viewer>,
    pub loader: Arc<ItemLoader>,
    pub cookies: Arc<CookieCommands>,
}

pub type ResponseState = Arc<CookieCommands>;

pub fn authenticated_viewer() -> Viewer {
    Viewer {
        id: "viewer-1".into(),
        access: AccessLevel::Private,
    }
}

#[cfg(test)]
mod tests {
    use super::ItemLoader;

    #[test]
    fn request_loader_caches_repeated_reads_for_the_same_id() {
        let loader = ItemLoader::synthetic();

        assert!(loader.load(&"item-1".into()).is_some());
        assert!(loader.load(&"item-1".into()).is_some());
        assert_eq!(loader.source_reads(), 1);
    }
}
