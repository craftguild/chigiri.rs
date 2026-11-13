//! Reject retired directive definitions and uses without reserving the same spelling for enum values or type names.

use chigiri::core::language::ast::{
    Directive, Document, FieldDefinition, InputValueDefinition, Name, TypeSystemDefinition as Def,
};

use super::SchemaError;

pub(super) fn reject(documents: &[Document]) -> Result<(), SchemaError> {
    for definition in documents.iter().flat_map(Document::type_system_definitions) {
        let (annotations, fields, inputs): (
            &[Directive],
            &[FieldDefinition],
            &[InputValueDefinition],
        ) = match definition {
            Def::Directive(value) => {
                name(&value.name)?;
                (&[], &[], &value.arguments)
            }
            Def::Schema(v) => (&v.directives, &[], &[]),
            Def::SchemaExtension(v) => (&v.directives, &[], &[]),
            Def::Scalar(v) => (&v.directives, &[], &[]),
            Def::ScalarExtension(v) => (&v.directives, &[], &[]),
            Def::Object(v) => (&v.directives, &v.fields, &[]),
            Def::ObjectExtension(v) => (&v.directives, &v.fields, &[]),
            Def::Interface(v) => (&v.directives, &v.fields, &[]),
            Def::InterfaceExtension(v) => (&v.directives, &v.fields, &[]),
            Def::Union(v) => (&v.directives, &[], &[]),
            Def::UnionExtension(v) => (&v.directives, &[], &[]),
            Def::InputObject(v) => (&v.directives, &[], &v.fields),
            Def::InputObjectExtension(v) => (&v.directives, &[], &v.fields),
            Def::Enum(v) => {
                for value in &v.values {
                    directives(&value.directives)?;
                }
                (&v.directives, &[], &[])
            }
            Def::EnumExtension(v) => {
                for value in &v.values {
                    directives(&value.directives)?;
                }
                (&v.directives, &[], &[])
            }
        };
        directives(annotations)?;
        for field in fields {
            directives(&field.directives)?;
            for input in &field.arguments {
                directives(&input.directives)?;
            }
        }
        for input in inputs {
            directives(&input.directives)?;
        }
    }
    Ok(())
}

fn directives(values: &[Directive]) -> Result<(), SchemaError> {
    for directive in values {
        name(&directive.name)?;
    }
    Ok(())
}

fn name(name: &Name) -> Result<(), SchemaError> {
    let code = match name.value.as_str() {
        "chigiri_mock_model" => "retired mock model directive: remove model annotation",
        "chigiri_mock_relation" | "chigiri_mock_inverse" => {
            "retired mock relation directive: use relation"
        }
        "chigiri_mock_crud" => "retired mock CRUD directive: implement a resolver module",
        "chigiri_mock_resolver" => {
            "retired mock resolver directive: use virtual for computed fields"
        }
        "chigiri_mock_value" => "retired mock value directive: use constraint",
        "chigiri_mock_scalar" => "retired mock scalar directive: use @representation(type: ...)",
        _ => return Ok(()),
    };
    Err(SchemaError::new(code, Some(name.span)))
}
