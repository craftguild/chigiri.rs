//! Generates public resolver traits and argument structs for objects.

mod arguments;
mod ordinary;
mod subscription;

use chigiri_core::schema::TypeKind;

use crate::error::CodegenError;
use crate::writer::GeneratedItem;

use super::model::RenderModel;

pub(super) use arguments::args_type;
pub(super) use ordinary::resolver_trait;
pub(super) use subscription::{
    subscription_event_type, subscription_resolve_method, subscription_subscribe_method,
};

pub(super) fn render_resolver_items(
    model: &RenderModel<'_>,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut definitions = Vec::new();
    for object in model
        .user_types()
        .filter(|ty| ty.kind() == TypeKind::Object)
    {
        let path = super::layout::resolver(model, object.name());
        for field in object
            .fields()
            .iter()
            .filter(|field| !field.arguments.is_empty())
        {
            definitions.push(arguments::render_args(model, object.name(), field)?.in_file(&path));
        }
        let resolver = if model.is_subscription_root(object.name()) {
            subscription::render_trait(model, object.name(), object.fields())?
        } else {
            ordinary::render_trait(model, object.name(), object.fields())?
        };
        definitions.push(resolver.in_file(path));
    }
    Ok(definitions)
}
