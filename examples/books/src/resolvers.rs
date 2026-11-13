//! Generated resolver traits are fully implemented in the application domain type.

use std::sync::Arc;

use chigiri::{FieldContext, FieldError};

use crate::domain::{Book, CatalogError};
use crate::graphql::generated;
use crate::server::AppContext;

pub(crate) struct Types;

impl generated::SchemaTypes for Types {
    type Book = Arc<Book>;
}

pub(crate) struct QueryFields;

impl generated::QueryResolver<AppContext, Types, CatalogError> for QueryFields {
    async fn books<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
    ) -> Result<Vec<Arc<Book>>, CatalogError> {
        context.context().catalog.books()
    }

    async fn book<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: generated::QueryBookArgs,
    ) -> Result<Option<Arc<Book>>, CatalogError> {
        context.context().catalog.book(&arguments.id)
    }
}

pub(crate) struct MutationFields;

impl generated::MutationResolver<AppContext, Types, CatalogError> for MutationFields {
    async fn add_book<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: generated::MutationAddBookArgs,
    ) -> Result<Arc<Book>, CatalogError> {
        let input = arguments.input;
        context.context().catalog.add(input.title, input.author)
    }

    async fn replace_book<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: generated::MutationReplaceBookArgs,
    ) -> Result<Arc<Book>, CatalogError> {
        let input = arguments.input;
        context
            .context()
            .catalog
            .replace(&arguments.id, input.title, input.author)
    }
}

/// Convert an explicit public allowlist while retaining all other domain sources.
pub(crate) fn map_catalog_error(error: CatalogError) -> FieldError {
    match error {
        CatalogError::Full => {
            FieldError::public("catalog capacity was reached").code("CATALOG_CAPACITY_REACHED")
        }
        CatalogError::InvalidBook => {
            FieldError::public("title and author must be non-empty and at most 256 UTF-8 bytes")
                .code("BAD_USER_INPUT")
        }
        CatalogError::NotFound => FieldError::public("book was not found").code("BOOK_NOT_FOUND"),
        other => FieldError::internal(other),
    }
}

pub(crate) struct BookFields;

impl generated::BookResolver<AppContext, Types, CatalogError> for BookFields {
    async fn id<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        book: &'a Arc<Book>,
    ) -> Result<chigiri::Id, CatalogError> {
        Ok(book.id.clone())
    }

    async fn title<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        book: &'a Arc<Book>,
    ) -> Result<String, CatalogError> {
        Ok(book.title.clone())
    }

    async fn author<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        book: &'a Arc<Book>,
    ) -> Result<String, CatalogError> {
        Ok(book.author.clone())
    }
}
