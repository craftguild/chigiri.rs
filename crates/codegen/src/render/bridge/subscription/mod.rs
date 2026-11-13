//! Renders private subscription source and event adapters.

mod event;
mod router;
mod setup;

use chigiri_core::schema::SemanticType;

use crate::error::CodegenError;
use crate::writer::GeneratedItem;

use super::super::model::RenderModel;

pub(super) fn render_subscription_items(
    model: &RenderModel<'_>,
    object_index: usize,
    object: &SemanticType,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = Vec::new();
    for (field_index, field) in object.fields().iter().enumerate() {
        items.push(setup::render_setup(
            model,
            object_index,
            object,
            field_index,
            field,
        )?);
        items.push(event::render_event(
            model,
            object_index,
            object,
            field_index,
            field,
        )?);
    }
    items.push(router::render_subscribe_router(model, object_index, object));
    items.push(router::render_event_router(model, object_index, object));
    Ok(items)
}

pub(in crate::render) fn setup_name(object_index: usize, field_index: usize) -> String {
    format!("__chigiri_subscribe_{object_index:04}_{field_index:04}")
}

pub(in crate::render) fn event_name(object_index: usize, field_index: usize) -> String {
    format!("__chigiri_subscription_event_{object_index:04}_{field_index:04}")
}

pub(in crate::render) fn subscribe_router_name(object_index: usize) -> String {
    format!("__chigiri_subscription_subscribe_{object_index:04}")
}

pub(in crate::render) fn event_router_name(object_index: usize) -> String {
    format!("__chigiri_subscription_event_route_{object_index:04}")
}
