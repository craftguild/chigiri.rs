//! Parse the closed custom-scalar representation vocabulary.

use chigiri::core::language::ast::Value;
use chigiri::core::schema::{SemanticType, TypeKind};

use super::{Constraints, Representation, ScalarSpec, domains, parse};
use crate::schema::SchemaError;

impl ScalarSpec {
    pub(crate) fn read(ty: &SemanticType) -> Result<Option<Self>, SchemaError> {
        if ty.kind() != TypeKind::Scalar {
            return Ok(None);
        }
        let directive = ty
            .directives()
            .iter()
            .find(|d| d.name.value == "representation");
        if matches!(ty.name(), "Int" | "Float" | "String" | "Boolean" | "ID") {
            return if directive.is_some() {
                Err(SchemaError::new(
                    "builtin scalar override forbidden",
                    ty.name_span(),
                ))
            } else {
                Ok(None)
            };
        }
        let directive = directive.ok_or_else(|| {
            SchemaError::new("custom scalar representation required", ty.name_span())
        })?;
        let representation = match parse::argument(directive, "type").map(|v| &v.value) {
            Some(Value::Enum(name)) => match name.value.as_str() {
                "STRING" => Representation::String,
                "BOOLEAN" => Representation::Boolean,
                "INTEGER" => Representation::Integer,
                "NUMBER" => Representation::Number,
                "OBJECT" => Representation::Object,
                "ARRAY" => Representation::Array,
                "JSON" => Representation::Json,
                _ => return Err(parse::invalid(directive.span)),
            },
            _ => return Err(parse::invalid(directive.span)),
        };
        let constraints = Constraints::basic(directive)?;
        domains::basic(&constraints, representation, directive.span)?;
        Ok(Some(Self {
            representation,
            constraints,
        }))
    }
}
