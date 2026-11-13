//! Compares user definitions with the checked-in SDL vocabulary and appends
//! only definitions the user did not supply.

use std::collections::BTreeSet;

use chigiri::core::Span;
use chigiri::core::language::ast::{Document, TypeSystemDefinition as Def};

use super::{SchemaError, VOCABULARY, equivalence};

pub(super) fn missing(documents: &[Document], library: &Document) -> Result<String, SchemaError> {
    let mut supplied = BTreeSet::new();
    for definition in documents.iter().flat_map(Document::type_system_definitions) {
        let Some((name, span)) = identity(definition) else {
            continue;
        };
        let expected = library.type_system_definitions().find(|d| {
            identity(d).is_some_and(|(n, _)| {
                n == name
                    && matches!(d, Def::Directive(_)) == matches!(definition, Def::Directive(_))
            })
        });
        if let Some(expected) = expected {
            if !equivalence::same(definition, expected)
                || !supplied.insert((matches!(definition, Def::Directive(_)), name))
            {
                return Err(SchemaError::new("conflicting mock vocabulary", Some(span)));
            }
        } else if matches!(definition, Def::Directive(_)) && name.starts_with("chigiri_mock_") {
            return Err(SchemaError::new("unknown mock vocabulary", Some(span)));
        }
    }
    let mut output = String::from("# Mock v3 vocabulary definitions missing from the user SDL.\n");
    for definition in library.type_system_definitions() {
        let (name, span) = identity(definition)
            .ok_or_else(|| SchemaError::new("invalid bundled vocabulary", None))?;
        if !supplied.contains(&(matches!(definition, Def::Directive(_)), name)) {
            output.push_str(&VOCABULARY[span.start..span.end]);
            output.push('\n');
        }
    }
    Ok(output)
}

pub(super) fn identity(definition: &Def) -> Option<(&str, Span)> {
    Some(match definition {
        Def::Directive(value) => (&value.name.value, value.span),
        Def::Scalar(value) => (&value.name.value, value.span),
        Def::Object(value) => (&value.name.value, value.span),
        Def::Interface(value) => (&value.name.value, value.span),
        Def::Union(value) => (&value.name.value, value.span),
        Def::Enum(value) => (&value.name.value, value.span),
        Def::InputObject(value) => (&value.name.value, value.span),
        Def::ScalarExtension(value) => (&value.name.value, value.span),
        Def::ObjectExtension(value) => (&value.name.value, value.span),
        Def::InterfaceExtension(value) => (&value.name.value, value.span),
        Def::UnionExtension(value) => (&value.name.value, value.span),
        Def::EnumExtension(value) => (&value.name.value, value.span),
        Def::InputObjectExtension(value) => (&value.name.value, value.span),
        Def::Schema(_) | Def::SchemaExtension(_) => return None,
    })
}
