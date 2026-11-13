//! Convert SDL named types to public application-facing Rust types.

use chigiri_core::language::ast::Description;
use chigiri_core::schema::{SemanticType, TypeKind};

use crate::error::CodegenError;
use crate::writer::{GeneratedItem, RustWriter};

use super::model::{RenderModel, enum_conversion_name, field_name, variant_name};

pub(super) fn render_type_items(
    model: &RenderModel<'_>,
) -> Result<Vec<GeneratedItem>, CodegenError> {
    let mut items = render_schema_types(model);
    for ty in model.user_types() {
        let item = match ty.kind() {
            TypeKind::Enum => Some(render_enum(model, ty)),
            TypeKind::InputObject if is_one_of(ty) => Some(render_one_of(model, ty)?),
            TypeKind::InputObject => Some(render_input(model, ty)?),
            TypeKind::Interface | TypeKind::Union => Some(render_composite(model, ty)),
            TypeKind::Scalar | TypeKind::Object => None,
        };
        items.extend(
            item.map(|item| item.in_file(format!("types/{}.rs", super::layout::owner(ty.name())))),
        );
    }
    Ok(items)
}

fn render_schema_types(model: &RenderModel<'_>) -> Vec<GeneratedItem> {
    let fragments = model
        .user_types()
        .filter(|ty| {
            ty.kind() == TypeKind::Scalar
                || (ty.kind() == TypeKind::Object && model.object_requires_domain(ty.name()))
                || (ty.kind() == TypeKind::Enum && model.enum_is_bound(ty.name()))
        })
        .map(|ty| schema_type_fragment(model, ty))
        .collect::<Vec<_>>();
    let mut writer = RustWriter::new();
    writer
        .line("/// Connect GraphQL objects, custom scalars, and bound enums to application types.");
    writer.line("pub trait SchemaTypes: ::std::marker::Send + ::std::marker::Sync + 'static {");
    writer.indented(|writer| {
        for fragment in &fragments {
            for line in fragment.lines() {
                writer.line(line);
            }
        }
    });
    writer.line("}");
    vec![GeneratedItem::new("schema_types.rs", writer.finish()).exported()]
}

fn schema_type_fragment(model: &RenderModel<'_>, ty: &SemanticType) -> String {
    let mut writer = RustWriter::new();
    write_description(&mut writer, ty.description());
    let name = model.type_name(ty.name());
    match ty.kind() {
        TypeKind::Scalar => writer.line(&format!("type {name}: ::chigiri::Scalar;")),
        TypeKind::Object => writer.line(&format!(
            "type {name}: ::std::marker::Send + ::std::marker::Sync + 'static;"
        )),
        TypeKind::Enum => {
            writer.line(&format!(
                "type {name}: ::std::marker::Send + ::std::marker::Sync + 'static;"
            ));
            writer.line(&format!(
                "fn {}(value: super::{name}) -> ::std::result::Result<Self::{name}, ::chigiri::EnumBindingError>;",
                enum_conversion_name(ty.name(), "from")
            ));
            writer.line(&format!(
                "fn {}(value: &Self::{name}) -> ::std::result::Result<super::{name}, ::chigiri::EnumBindingError>;",
                enum_conversion_name(ty.name(), "to")
            ));
        }
        _ => unreachable!("Type type filtered."),
    }
    writer.finish()
}

fn render_enum(model: &RenderModel<'_>, ty: &SemanticType) -> GeneratedItem {
    let name = model.type_name(ty.name());
    let mut writer = RustWriter::new();
    write_description(&mut writer, ty.description());
    writer.line("#[derive(Clone, Copy, Debug, Eq, PartialEq)]");
    writer.line(&format!("pub enum {name} {{"));
    writer.indented(|writer| {
        for value in ty.enum_values() {
            write_description(writer, value.description.as_ref());
            writer.line(&format!("{},", variant_name(&value.name.value)));
        }
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish()).exported()
}

fn render_input(model: &RenderModel<'_>, ty: &SemanticType) -> Result<GeneratedItem, CodegenError> {
    let name = model.type_name(ty.name());
    let generic = model.input_is_generic(ty.name());
    let fields = ty
        .input_fields()
        .iter()
        .map(|field| Ok((field, model.input_field_type(ty.name(), field)?)))
        .collect::<Result<Vec<_>, CodegenError>>()?;
    let mut writer = RustWriter::new();
    write_description(&mut writer, ty.description());
    writer.line(&format!(
        "pub struct {name}{} {{",
        schema_types_parameter(generic)
    ));
    writer.indented(|writer| {
        for (field, rust_type) in &fields {
            write_description(writer, field.description.as_ref());
            writer.line(&format!(
                "pub {}: {rust_type},",
                field_name(&field.name.value)
            ));
        }
    });
    writer.line("}");
    Ok(GeneratedItem::new(name, writer.finish()).exported())
}

fn render_one_of(
    model: &RenderModel<'_>,
    ty: &SemanticType,
) -> Result<GeneratedItem, CodegenError> {
    let name = model.type_name(ty.name());
    let generic = model.input_is_generic(ty.name());
    let variants = ty
        .input_fields()
        .iter()
        .map(|field| Ok((field, model.one_of_input_type(ty.name(), field)?)))
        .collect::<Result<Vec<_>, CodegenError>>()?;
    let mut writer = RustWriter::new();
    write_description(&mut writer, ty.description());
    writer.line(&format!(
        "pub enum {name}{} {{",
        schema_types_parameter(generic)
    ));
    writer.indented(|writer| {
        for (field, rust_type) in &variants {
            write_description(writer, field.description.as_ref());
            writer.line(&format!(
                "{}({rust_type}),",
                variant_name(&field.name.value)
            ));
        }
    });
    writer.line("}");
    Ok(GeneratedItem::new(name, writer.finish()).exported())
}

fn render_composite(model: &RenderModel<'_>, ty: &SemanticType) -> GeneratedItem {
    let name = model.type_name(ty.name());
    let possible = model.possible_objects(ty);
    let mut writer = RustWriter::new();
    write_description(&mut writer, ty.description());
    writer.line(&format!(
        "pub enum {name}{} {{",
        schema_types_parameter(!possible.is_empty())
    ));
    writer.indented(|writer| {
        for object in possible {
            let object_name = model.type_name(object.name());
            writer.line(&format!("{object_name}(T::{object_name}),"));
        }
    });
    writer.line("}");
    GeneratedItem::new(name, writer.finish()).exported()
}

fn schema_types_parameter(generic: bool) -> &'static str {
    if generic {
        "<T: super::SchemaTypes>"
    } else {
        ""
    }
}

fn is_one_of(ty: &SemanticType) -> bool {
    ty.directives()
        .iter()
        .any(|directive| directive.name.value == "oneOf")
}

fn write_description(writer: &mut RustWriter, description: Option<&Description>) {
    if let Some(description) = description {
        writer.doc_comment(&description.value.value);
    }
}

#[cfg(test)]
mod tests {
    use chigiri_core::language::parse_type_system_document;
    use chigiri_core::schema::compose_schema;
    use chigiri_core::{ExecutionLimits, SourceId};

    use crate::writer::assemble_items;

    use super::{RenderModel, render_type_items};

    fn generated_source(source: &str) -> String {
        let limits = ExecutionLimits::unbounded();
        let parsed = parse_type_system_document(SourceId::new(0), source, &limits);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let composed = compose_schema(&[parsed.document.unwrap()], &limits);
        assert!(
            composed.diagnostics.is_empty(),
            "{:?}",
            composed.diagnostics
        );
        let schema = composed.schema.unwrap();
        let model = RenderModel::build(&schema).unwrap();
        assemble_items(render_type_items(&model).unwrap())
            .unwrap()
            .into_iter()
            .map(|file| file.contents)
            .collect()
    }

    #[test]
    fn generates_schema_types_and_input_presence_markers() {
        let generated = generated_source(
            r#"
            scalar DateTime
            type Query { node(filter: Filter): Node }
            interface Node { id: ID! }
            type User implements Node { id: ID!, createdAt: DateTime! }
            enum Role { ADMIN READ_ONLY }
            input Nested { at: DateTime }
            input Filter {
                id: ID!
                name: String
                role: Role = ADMIN
                nested: Nested
            }
            "#,
        );
        assert!(generated.contains("type DateTime: ::chigiri::Scalar;"));
        assert!(
            generated.contains("type User: ::std::marker::Send + ::std::marker::Sync + 'static;")
        );
        assert!(
            !generated.contains("type Query: ::std::marker::Send + ::std::marker::Sync + 'static;")
        );
        assert!(generated.contains("pub id: ::chigiri::Id,"));
        assert!(generated.contains("pub name: ::chigiri::Input<::std::string::String>,"));
        assert!(generated.contains("pub role: ::std::option::Option<super::Role>,"));
        assert!(generated.contains("pub nested: ::chigiri::Input<super::Nested<T>>,"));
        assert!(generated.contains("pub enum Node<T: super::SchemaTypes>"));
        assert!(generated.contains("User(T::User),"));
    }

    #[test]
    fn one_of_and_union_generated_by_the_related_payload() {
        let generated = generated_source(
            r#"
            scalar Token
            type Query { search: Search }
            type User { id: ID! }
            type Team { id: ID! }
            union Search = User | Team
            input Selector @oneOf { id: ID, token: Token }
            "#,
        );
        assert!(generated.contains("pub enum Selector<T: super::SchemaTypes>"));
        assert!(generated.contains("Id(::chigiri::Id),"));
        assert!(generated.contains("Token(T::Token),"));
        assert!(generated.contains("pub enum Search<T: super::SchemaTypes>"));
        assert!(generated.contains("Team(T::Team),"));
    }

    #[test]
    fn output_only_root_objects_require_associated_types() {
        let generated = generated_source("type Query { root: Query }");

        assert!(
            generated.contains("type Query: ::std::marker::Send + ::std::marker::Sync + 'static;")
        );
    }

    #[test]
    fn interface_without_implementors_does_not_generate_an_unused_generic() {
        let generated = generated_source("interface Node { id: ID! } type Query { node: Node }");

        assert!(generated.contains("pub enum Node {"));
    }
}
