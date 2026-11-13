//! Enforce source limits before parsing and type-member limits before composition.

use std::collections::BTreeSet;

use chigiri::core::Source;
use chigiri::core::language::ast::{Document, FieldDefinition, TypeSystemDefinition as Def};

use super::{SchemaError, VOCABULARY_SOURCE, vocabulary::identity};

pub(super) fn sources(sources: &[Source]) -> Result<(), SchemaError> {
    if sources.is_empty() || sources.len() > crate::limits::SDL_SOURCES {
        return Err(SchemaError::new("SDL source count exceeded", None));
    }
    let mut bytes = 0usize;
    let mut names = BTreeSet::new();
    for source in sources {
        if source.name() == VOCABULARY_SOURCE || !names.insert(source.name()) {
            return Err(SchemaError::new("duplicate or reserved source name", None));
        }
        bytes = bytes
            .checked_add(source.text().len())
            .filter(|bytes| *bytes <= super::MAX_SDL_BYTES)
            .ok_or_else(|| SchemaError::new("SDL byte budget exceeded", None))?;
    }
    Ok(())
}

pub(super) fn shapes(documents: &[Document], library: &Document) -> Result<(), SchemaError> {
    let reserved = library
        .type_system_definitions()
        .filter_map(|d| identity(d).map(|(name, _)| (matches!(d, Def::Directive(_)), name)))
        .collect::<BTreeSet<_>>();
    let mut names = BTreeSet::new();
    let mut members = 0usize;
    for definition in documents.iter().flat_map(Document::type_system_definitions) {
        let Some((name, span)) = identity(definition) else {
            continue;
        };
        if reserved.contains(&(matches!(definition, Def::Directive(_)), name)) {
            continue;
        }
        if !matches!(definition, Def::Directive(_)) {
            names.insert(name);
        }
        members += match definition {
            Def::Object(value) => fields(&value.fields),
            Def::Interface(value) => fields(&value.fields),
            Def::ObjectExtension(value) => fields(&value.fields),
            Def::InterfaceExtension(value) => fields(&value.fields),
            Def::InputObject(value) => value.fields.len(),
            Def::InputObjectExtension(value) => value.fields.len(),
            Def::Directive(value) => value.arguments.len(),
            _ => 0,
        };
        if names.len() > crate::limits::SDL_TYPES || members > crate::limits::SDL_MEMBERS {
            return Err(SchemaError::new("schema shape budget exceeded", Some(span)));
        }
    }
    Ok(())
}

fn fields(fields: &[FieldDefinition]) -> usize {
    fields.iter().map(|field| 1 + field.arguments.len()).sum()
}
