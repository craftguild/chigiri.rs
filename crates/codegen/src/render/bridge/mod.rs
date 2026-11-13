//! Connects generated resolver groups to the executor's dynamic ABI.

mod arguments;
mod dispatch;
mod field;
mod router;
mod scalars;
mod schema_router;
pub(in crate::render) mod subscription;

use chigiri_core::schema::TypeKind;

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::model::RenderModel;

pub(super) fn render_bridge_items(
    model: &RenderModel<'_>,
    resolver_abi_fingerprint: chigiri_core::executor::ResolverSchemaFingerprint,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = Vec::new();
    let scalars = model
        .user_types()
        .filter(|ty| ty.kind() == TypeKind::Scalar)
        .collect::<Vec<_>>();
    for (index, scalar) in scalars.iter().enumerate() {
        items.push(
            render_scalar_validator(model, index, scalar.name()).in_file(format!(
                "internal/scalars/types/{}.rs",
                super::layout::owner(scalar.name())
            )),
        );
    }
    let scalar_registration = scalars::render_registration(&scalars);
    items.extend(
        scalar_registration
            .items
            .into_iter()
            .map(|item| item.in_file("internal/scalars/registry.rs")),
    );
    let objects = model
        .user_types()
        .filter(|ty| ty.kind() == TypeKind::Object)
        .collect::<Vec<_>>();
    for (object_index, object) in objects.iter().enumerate() {
        if model.is_subscription_root(object.name()) && !model.object_requires_domain(object.name())
        {
            continue;
        }
        let path = format!(
            "internal/bridge/objects/{}.rs",
            super::layout::owner(object.name())
        );
        for (field_index, definition) in object.fields().iter().enumerate() {
            items.push(
                field::render_field(model, object_index, object, field_index, definition)?
                    .in_file(&path),
            );
        }
        items.extend(
            router::render_object(model, object_index, object)
                .into_iter()
                .map(|item| item.in_file(&path)),
        );
    }
    if let Some((object_index, object)) = objects
        .iter()
        .enumerate()
        .find(|(_, object)| model.is_subscription_root(object.name()))
    {
        let path = format!(
            "internal/bridge/subscription/{}.rs",
            super::layout::owner(object.name())
        );
        items.extend(
            subscription::render_subscription_items(model, object_index, object)?
                .into_iter()
                .map(|item| item.in_file(&path)),
        );
    }
    let schema_routes = schema_router::render_schema_routes(&objects);
    let fingerprint = chigiri_core::executor::generated_resolver_abi_fingerprint(model.schema);
    items.extend(
        schema_routes
            .items
            .into_iter()
            .map(|item| item.in_file("internal/bridge/schema.rs")),
    );
    items.push(
        dispatch::render_dispatch_impl(
            scalar_registration.root.as_deref(),
            &schema_routes.root,
            model.subscription_root(),
            fingerprint,
            resolver_abi_fingerprint,
        )
        .in_file("internal/bridge/dispatch.rs"),
    );
    Ok(items)
}

fn render_scalar_validator(model: &RenderModel<'_>, index: usize, scalar: &str) -> GeneratedItem {
    let name = scalar_validator_name(index);
    let associated = model.type_name(scalar);
    let mut writer = RustWriter::new();
    writer.line(&format!(
        "pub(super) fn {name}<T: super::SchemaTypes>(_: &str, value: ::chigiri::Value) -> ::std::result::Result<::chigiri::Value, ::chigiri::ScalarCoercionError> {{"
    ));
    writer.indented(|writer| {
        writer.line("let original = value.clone();");
        writer.line(&format!(
            "<T::{associated} as ::chigiri::Scalar>::parse(value).map(|_| original)"
        ));
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish())
}

pub(super) fn field_function_name(object_index: usize, field_index: usize) -> String {
    format!("__chigiri_resolve_{object_index:04}_{field_index:04}")
}

fn scalar_validator_name(index: usize) -> String {
    format!("__chigiri_validate_scalar_{index:04}")
}

pub(super) fn object_router_name(object_index: usize) -> String {
    format!("__chigiri_route_{object_index:04}")
}
