//! Bounded reuse of validated documents within a schema instance.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use chigiri_core::executable::ValidatedDocument;

use crate::DocumentCacheLimits;

pub(crate) struct DocumentCache {
    limits: DocumentCacheLimits,
    state: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    entries: BTreeMap<String, Arc<ValidatedDocument>>,
    insertion_order: VecDeque<String>,
    query_bytes: usize,
}

impl DocumentCache {
    pub(crate) fn new(limits: DocumentCacheLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(CacheState::default()),
        }
    }

    /// A cache hit clones only the `Arc` while locked and then immediately releases the guard.
    pub(crate) fn get(&self, query: &str) -> Option<Arc<ValidatedDocument>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .get(query)
            .cloned()
    }

    pub(crate) fn insert(
        &self,
        query: String,
        document: Arc<ValidatedDocument>,
    ) -> Arc<ValidatedDocument> {
        if self.limits.max_entries == 0
            || query.len() > self.limits.max_query_bytes
            || self.limits.max_query_bytes == 0
        {
            return document;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = state.entries.get(&query) {
            return Arc::clone(existing);
        }
        state.query_bytes = state.query_bytes.saturating_add(query.len());
        state.insertion_order.push_back(query.clone());
        state.entries.insert(query, Arc::clone(&document));
        while state.entries.len() > self.limits.max_entries
            || state.query_bytes > self.limits.max_query_bytes
        {
            let Some(oldest) = state.insertion_order.pop_front() else {
                break;
            };
            if state.entries.remove(&oldest).is_some() {
                state.query_bytes = state.query_bytes.saturating_sub(oldest.len());
            }
        }
        document
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .len()
    }
}

#[cfg(test)]
mod tests {
    use chigiri_core::executable::{ValidatedDocument, validate_executable_document};
    use chigiri_core::language::{parse_executable_document, parse_type_system_document};
    use chigiri_core::schema::compose_schema;
    use chigiri_core::{ExecutionLimits, SourceId};

    use super::DocumentCache;
    use crate::DocumentCacheLimits;

    #[test]
    fn entry_limit_evicts_the_oldest_validated_document() {
        let cache = DocumentCache::new(DocumentCacheLimits {
            max_entries: 2,
            max_query_bytes: 100,
        });
        for query in ["a", "bb", "ccc"] {
            cache.insert(query.to_owned(), document());
        }

        assert!(cache.get("a").is_none());
        assert!(cache.get("bb").is_some());
        assert!(cache.get("ccc").is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn oversized_keys_and_disabled_caches_store_no_entries() {
        let bounded = DocumentCache::new(DocumentCacheLimits {
            max_entries: 10,
            max_query_bytes: 2,
        });
        bounded.insert("long".to_owned(), document());
        let disabled = DocumentCache::new(DocumentCacheLimits::disabled());
        disabled.insert("a".to_owned(), document());

        assert_eq!(bounded.len(), 0);
        assert_eq!(disabled.len(), 0);
    }

    #[test]
    fn does_not_share_entries_between_schema_instances() {
        let first = DocumentCache::new(DocumentCacheLimits::default());
        let second = DocumentCache::new(DocumentCacheLimits::default());
        first.insert("{ value }".to_owned(), document());

        assert!(first.get("{ value }").is_some());
        assert!(second.get("{ value }").is_none());
    }

    fn document() -> std::sync::Arc<ValidatedDocument> {
        let limits = ExecutionLimits::unbounded();
        let schema_document =
            parse_type_system_document(SourceId::new(0), "type Query { value: String }", &limits)
                .document
                .unwrap();
        let schema = compose_schema(&[schema_document], &limits).schema.unwrap();
        let document = parse_executable_document(SourceId::new(1), "{ value }", &limits)
            .document
            .unwrap();
        std::sync::Arc::new(
            validate_executable_document(&schema, &document, &limits)
                .document
                .unwrap(),
        )
    }
}
