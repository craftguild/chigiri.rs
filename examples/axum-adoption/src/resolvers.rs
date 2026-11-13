//! Complete implementation of the generated resolver contract using synthetic domain types.

use chigiri::{FieldContext, FieldError, FieldResult};

use crate::application::AppContext;
use crate::domain::{Item, Types, Viewer};
use crate::graphql::generated;

pub struct QueryFields;

impl generated::QueryResolver<AppContext, Types, chigiri::FieldError> for QueryFields {
    async fn viewer<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
    ) -> FieldResult<Option<Viewer>> {
        Ok(context.context().viewer.clone())
    }

    async fn item<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: generated::QueryItemArgs,
    ) -> FieldResult<Option<Item>> {
        Ok(context.context().loader.load(&arguments.id))
    }

    async fn expensive<'a>(&'a self, _: FieldContext<'a, AppContext>) -> FieldResult<bool> {
        Ok(true)
    }
}

pub struct MutationFields;

impl generated::MutationResolver<AppContext, Types, chigiri::FieldError> for MutationFields {
    async fn touch<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: generated::MutationTouchArgs,
    ) -> FieldResult<Item> {
        let item = context
            .context()
            .loader
            .load(&arguments.id)
            .ok_or_else(|| FieldError::public("Item was not found").code("NOT_FOUND"))?;
        context.context().cookies.record_touch(&arguments.id);
        Ok(item)
    }
}

pub struct ViewerFields;

impl generated::ViewerResolver<AppContext, Types, FieldError> for ViewerFields {
    async fn id<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        viewer: &'a Viewer,
    ) -> FieldResult<chigiri::Id> {
        Ok(viewer.id.clone())
    }
    async fn access<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        viewer: &'a Viewer,
    ) -> FieldResult<crate::domain::AccessLevel> {
        Ok(viewer.access)
    }
}

pub struct ItemFields;

impl generated::ItemResolver<AppContext, Types, FieldError> for ItemFields {
    async fn id<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        item: &'a Item,
    ) -> FieldResult<chigiri::Id> {
        Ok(item.id.clone())
    }
    async fn title<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        item: &'a Item,
    ) -> FieldResult<String> {
        Ok(item.title.clone())
    }
}
