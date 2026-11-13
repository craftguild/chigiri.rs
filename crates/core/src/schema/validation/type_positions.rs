//! Resolves named types through wrappers, then validates their input or output position.

use crate::Span;
use crate::language::ast::{TypeRef, TypeRefKind};

use super::super::composer::Composer;
use super::super::{SemanticTypeRef, SemanticTypeRefKind, TypeKind};

#[derive(Clone, Copy)]
enum Position {
    Input,
    Output,
}

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = composer.schema.types[index].clone();
        for field in named_type.fields {
            validate_source_ref(composer, &field.ty, Position::Output);
            if !composer.can_continue() {
                return;
            }
            for argument in field.arguments {
                validate_source_ref(composer, &argument.ty, Position::Input);
                if !composer.can_continue() {
                    return;
                }
            }
        }
        for field in named_type.input_fields {
            validate_source_ref(composer, &field.ty, Position::Input);
            if !composer.can_continue() {
                return;
            }
        }
    }
    validate_directive_arguments(composer);
}

fn validate_directive_arguments(composer: &mut Composer) {
    for index in 0..composer.schema.directives.len() {
        if !composer.can_continue() {
            return;
        }
        let arguments = composer.schema.directives[index].arguments().to_vec();
        for argument in arguments {
            validate_semantic_ref(composer, &argument.ty, Position::Input);
            if !composer.can_continue() {
                return;
            }
        }
    }
}

fn validate_source_ref(composer: &mut Composer, reference: &TypeRef, position: Position) {
    reject_invalid_source_wrapping(composer, reference);
    if !composer.can_continue() {
        return;
    }
    let (name, span) = source_base(reference);
    validate_named_position(composer, name, Some(span), position);
}

fn source_base(reference: &TypeRef) -> (&str, Span) {
    match &reference.kind {
        TypeRefKind::Named(name) => (&name.value, name.span),
        TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => source_base(inner),
    }
}

fn validate_semantic_ref(composer: &mut Composer, reference: &SemanticTypeRef, position: Position) {
    reject_invalid_semantic_wrapping(composer, reference);
    if !composer.can_continue() {
        return;
    }
    let (name, span) = semantic_base(reference);
    validate_named_position(composer, name, span, position);
}

fn reject_invalid_source_wrapping(composer: &mut Composer, reference: &TypeRef) {
    let mut current = reference;
    loop {
        match &current.kind {
            TypeRefKind::NonNull(inner) => {
                if matches!(inner.kind, TypeRefKind::NonNull(_)) {
                    composer.report(
                        super::super::SCHEMA_INVALID_TYPE_WRAPPING,
                        "Non-null type must not wrap another non-null type",
                        current.span,
                    );
                    return;
                }
                current = inner;
            }
            TypeRefKind::List(inner) => current = inner,
            TypeRefKind::Named(_) => return,
        }
    }
}

fn reject_invalid_semantic_wrapping(composer: &mut Composer, reference: &SemanticTypeRef) {
    let mut current = reference;
    loop {
        match &current.kind {
            SemanticTypeRefKind::NonNull(inner) => {
                if matches!(inner.kind, SemanticTypeRefKind::NonNull(_)) {
                    report(
                        composer,
                        super::super::SCHEMA_INVALID_TYPE_WRAPPING,
                        "Non-null type must not wrap another non-null type".to_owned(),
                        current.span,
                    );
                    return;
                }
                current = inner;
            }
            SemanticTypeRefKind::List(inner) => current = inner,
            SemanticTypeRefKind::Named(_) => return,
        }
    }
}

fn semantic_base(reference: &SemanticTypeRef) -> (&str, Option<Span>) {
    match &reference.kind {
        SemanticTypeRefKind::Named(name) => (name, reference.span),
        SemanticTypeRefKind::List(inner) | SemanticTypeRefKind::NonNull(inner) => {
            semantic_base(inner)
        }
    }
}

fn validate_named_existence(composer: &mut Composer, name: &str, span: Option<Span>) {
    if composer.schema.type_index.contains_key(name) {
        return;
    }
    report(
        composer,
        super::super::SCHEMA_UNKNOWN_TYPE,
        format!("Referenced type `{name}` is not defined"),
        span,
    );
}

fn validate_named_position(
    composer: &mut Composer,
    name: &str,
    span: Option<Span>,
    position: Position,
) {
    let Some(named_type) = composer.schema.type_by_name(name) else {
        validate_named_existence(composer, name, span);
        return;
    };
    let allowed = match position {
        Position::Input => matches!(
            named_type.kind(),
            TypeKind::Scalar | TypeKind::Enum | TypeKind::InputObject
        ),
        Position::Output => !matches!(named_type.kind(), TypeKind::InputObject),
    };
    if allowed {
        return;
    }
    let (code, label) = match position {
        Position::Input => (super::super::SCHEMA_INVALID_INPUT_TYPE, "input"),
        Position::Output => (super::super::SCHEMA_INVALID_OUTPUT_TYPE, "output"),
    };
    report(
        composer,
        code,
        format!("Type `{name}` cannot be used in {label} position"),
        span,
    );
}

fn report(
    composer: &mut Composer,
    code: crate::DiagnosticCode,
    message: String,
    span: Option<Span>,
) {
    if let Some(span) = span {
        composer.report(code, message, span);
    } else {
        composer.report_without_span(code, message);
    }
}
