//! The book domain model and in-memory catalog that are not dependent on GraphQL-specific DTOs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chigiri::Id;

const MAX_BOOKS: usize = 1_000;
const MAX_TEXT_BYTES: usize = 256;

/// Application domain state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Book {
    pub(crate) id: Id,
    pub(crate) title: String,
    pub(crate) author: String,
}

/// Bounded in-memory catalog shared between example requests.
pub struct Catalog {
    books: Mutex<Vec<Arc<Book>>>,
    next_id: AtomicU64,
    request_count: AtomicU64,
    max_books: usize,
}

impl Catalog {
    /// Creates an empty catalog.
    pub fn new() -> Self {
        Self::with_limit(MAX_BOOKS)
    }

    fn with_limit(max_books: usize) -> Self {
        Self {
            books: Mutex::new(Vec::with_capacity(max_books.min(16))),
            next_id: AtomicU64::new(1),
            request_count: AtomicU64::new(0),
            max_books,
        }
    }

    /// Creates the starter catalog used by the quick-start example.
    pub fn seeded() -> Self {
        let catalog = Self::new();
        let _ = catalog.add(
            "The Left Hand of Darkness".to_owned(),
            "Ursula K. Le Guin".to_owned(),
        );
        catalog
    }

    pub(crate) fn books(&self) -> Result<Vec<Arc<Book>>, CatalogError> {
        self.books
            .lock()
            .map(|books| books.clone())
            .map_err(|_| CatalogError::Unavailable)
    }

    pub(crate) fn book(&self, id: &Id) -> Result<Option<Arc<Book>>, CatalogError> {
        self.books
            .lock()
            .map(|books| books.iter().find(|book| book.id == *id).cloned())
            .map_err(|_| CatalogError::Unavailable)
    }

    pub(crate) fn add(&self, title: String, author: String) -> Result<Arc<Book>, CatalogError> {
        if !valid_text(&title, &author) {
            return Err(CatalogError::InvalidBook);
        }
        let mut books = self.books.lock().map_err(|_| CatalogError::Unavailable)?;
        if books.len() >= self.max_books {
            return Err(CatalogError::Full);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let book = Arc::new(Book {
            id: Id::from(id),
            title,
            author,
        });
        books.push(Arc::clone(&book));
        Ok(book)
    }

    pub(crate) fn replace(
        &self,
        id: &Id,
        title: String,
        author: String,
    ) -> Result<Arc<Book>, CatalogError> {
        if !valid_text(&title, &author) {
            return Err(CatalogError::InvalidBook);
        }
        let mut books = self.books.lock().map_err(|_| CatalogError::Unavailable)?;
        let position = books
            .iter()
            .position(|book| book.id == *id)
            .ok_or(CatalogError::NotFound)?;
        let book = Arc::new(Book {
            id: id.clone(),
            title,
            author,
        });
        books[position] = Arc::clone(&book);
        Ok(book)
    }

    pub(crate) fn observe_request(&self) -> u64 {
        self.request_count.fetch_add(1, Ordering::Relaxed) + 1
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub(crate) enum CatalogError {
    Unavailable,
    Full,
    InvalidBook,
    NotFound,
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("catalog lock is unavailable"),
            Self::Full => formatter.write_str("catalog capacity was reached"),
            Self::InvalidBook => formatter.write_str("book text is outside catalog limits"),
            Self::NotFound => formatter.write_str("book was not found"),
        }
    }
}

impl std::error::Error for CatalogError {}

fn valid_text(title: &str, author: &str) -> bool {
    !title.trim().is_empty()
        && !author.trim().is_empty()
        && title.len() <= MAX_TEXT_BYTES
        && author.len() <= MAX_TEXT_BYTES
}

#[cfg(test)]
mod tests {
    use super::{Catalog, CatalogError};

    #[test]
    fn catalog_rejects_additions_that_would_exceed_the_configured_limit() {
        let catalog = Catalog::with_limit(1);
        catalog
            .add("first".to_owned(), "author".to_owned())
            .unwrap();
        assert!(matches!(
            catalog.add("second".to_owned(), "author".to_owned()),
            Err(CatalogError::Full)
        ));
        assert_eq!(catalog.books().unwrap().len(), 1);

        let invalid = Catalog::with_limit(1);
        assert!(matches!(
            invalid.add("x".repeat(257), "author".to_owned()),
            Err(CatalogError::InvalidBook)
        ));
    }

    #[test]
    fn replacing_an_item_updates_the_same_id_without_increasing_catalog_size() {
        let catalog = Catalog::seeded();
        let replaced = catalog
            .replace(
                &chigiri::Id::from(1_u64),
                "Soak".to_owned(),
                "Chigiri".to_owned(),
            )
            .unwrap();
        assert_eq!(replaced.id, chigiri::Id::from(1_u64));
        assert_eq!(replaced.title, "Soak");
        assert_eq!(catalog.books().unwrap().len(), 1);
    }
}
