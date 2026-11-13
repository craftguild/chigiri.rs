//! Normalize AST, semantic, and introspection input types into one shape.

use crate::language::ast::{TypeRef, TypeRefKind};
use crate::schema::introspection::model::{TypeRefDescriptor, TypeWrapper};
use crate::schema::{SemanticTypeRef, SemanticTypeRefKind};

const NON_NULL: u8 = 1;
const LIST: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct InputType<'a> {
    wrappers: Vec<u8>,
    named: &'a str,
}

impl<'a> InputType<'a> {
    pub(super) fn from_ast(reference: &'a TypeRef) -> Self {
        let mut wrappers = Vec::new();
        let mut current = reference;
        let named = loop {
            match &current.kind {
                TypeRefKind::Named(name) => break name.value.as_str(),
                TypeRefKind::List(inner) => {
                    wrappers.push(LIST);
                    current = inner;
                }
                TypeRefKind::NonNull(inner) => {
                    wrappers.push(NON_NULL);
                    current = inner;
                }
            }
        };
        Self { wrappers, named }
    }

    pub(super) fn from_semantic(reference: &'a SemanticTypeRef) -> Self {
        let mut wrappers = Vec::new();
        let mut current = reference;
        let named = loop {
            match &current.kind {
                SemanticTypeRefKind::Named(name) => break name.as_str(),
                SemanticTypeRefKind::List(inner) => {
                    wrappers.push(LIST);
                    current = inner;
                }
                SemanticTypeRefKind::NonNull(inner) => {
                    wrappers.push(NON_NULL);
                    current = inner;
                }
            }
        };
        Self { wrappers, named }
    }

    pub(super) fn from_descriptor(reference: TypeRefDescriptor) -> Self {
        Self {
            wrappers: reference
                .wrappers
                .iter()
                .map(|wrapper| match wrapper {
                    TypeWrapper::NonNull => NON_NULL,
                    TypeWrapper::List => LIST,
                })
                .collect(),
            named: reference.named_type,
        }
    }

    pub(super) fn non_null_named(name: &'a str) -> Self {
        Self {
            wrappers: vec![NON_NULL],
            named: name,
        }
    }

    pub(super) fn named_type(&self) -> &str {
        self.named
    }

    pub(super) fn is_non_null(&self) -> bool {
        self.wrappers.first() == Some(&NON_NULL)
    }

    pub(super) fn is_list(&self) -> bool {
        self.wrappers.first() == Some(&LIST)
    }

    pub(super) fn peel(&self) -> Self {
        Self {
            wrappers: self.wrappers.iter().skip(1).copied().collect(),
            named: self.named,
        }
    }
}

pub(super) fn compatible(
    variable: &InputType<'_>,
    location: &InputType<'_>,
    default_exception: bool,
) -> bool {
    let mut variable_index = 0;
    let mut location_index = 0;
    if location.wrappers.first() == Some(&NON_NULL)
        && variable.wrappers.first() != Some(&NON_NULL)
        && default_exception
    {
        location_index += 1;
    }
    loop {
        let variable_wrapper = variable.wrappers.get(variable_index).copied();
        let location_wrapper = location.wrappers.get(location_index).copied();
        match (variable_wrapper, location_wrapper) {
            (Some(NON_NULL), Some(NON_NULL)) | (Some(LIST), Some(LIST)) => {
                variable_index += 1;
                location_index += 1;
            }
            (Some(NON_NULL), wrapper) if wrapper != Some(NON_NULL) => variable_index += 1,
            (None, None) => return variable.named == location.named,
            _ => return false,
        }
    }
}

#[cfg(test)]
#[test]
fn rsm_16_input_type_peeling_and_cloning_borrow_the_source_name() {
    use crate::language::ast::Name;
    use crate::{SourceId, Span};
    let name = "Long".repeat(8192);
    let span = Span::new(SourceId::new(74), 0, name.len());
    let reference = TypeRef::named(Name::new(name, span), span);
    let expected = InputType::from_ast(&reference);
    let TypeRefKind::Named(source) = &reference.kind else {
        panic!("expected a named type reference.");
    };
    assert!(std::ptr::eq(expected.named_type(), source.value.as_str()));
    for _ in 0..10_000 {
        assert!(std::ptr::eq(
            expected.peel().named_type(),
            expected.clone().named_type()
        ));
    }
}
