//! Detect generated public and member-name collisions with source locations.

use ::std::collections::{BTreeMap, BTreeSet};

use chigiri_core::language::ast::Name;
use chigiri_core::schema::{SemanticSchema, SemanticType, TypeKind};
use chigiri_core::{Diagnostic, SourceMap, Span};

use crate::error::CodegenError;
use crate::naming::{IdentifierCase, IdentifierScope, NAME_COLLISION, prefixed_field_method};

use super::{field_name, user_types, variant_name};

pub(super) fn validate_member_names(
    schema: &SemanticSchema,
    domain_objects: &BTreeSet<String>,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    for ty in user_types(schema) {
        let members: Vec<&Name> = match ty.kind() {
            TypeKind::Object | TypeKind::Interface => {
                ty.fields().iter().map(|field| &field.name).collect()
            }
            TypeKind::Enum => ty.enum_values().iter().map(|value| &value.name).collect(),
            TypeKind::InputObject => ty.input_fields().iter().map(|field| &field.name).collect(),
            TypeKind::Scalar | TypeKind::Union => Vec::new(),
        };
        let case = if ty.kind() == TypeKind::Enum {
            IdentifierCase::UpperCamel
        } else {
            IdentifierCase::Snake
        };
        let mut scope = IdentifierScope::default();
        for name in members {
            scope
                .reserve(name, case)
                .map_err(|diagnostic| diagnostic_error(diagnostic, sources))?;
        }
        validate_arguments(ty, sources)?;
        if schema
            .roots()
            .subscription()
            .is_some_and(|root| root.type_name == ty.name())
        {
            validate_subscription_methods(ty, domain_objects.contains(ty.name()), sources)?;
        }
    }
    Ok(())
}

fn validate_subscription_methods(
    ty: &SemanticType,
    nested: bool,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    let mut methods = BTreeMap::<String, String>::new();
    for field in ty.fields() {
        let coordinate = format!("{}.{}", ty.name(), field.name.value);
        let subscribe = prefixed_field_method("subscribe_", &field.name.value);
        let event = format!(
            "{}_event",
            prefixed_field_method("resolve_", &field.name.value)
        );
        for method in [subscribe, event] {
            reserve_subscription_method(
                &mut methods,
                method,
                coordinate.clone(),
                field.name.span,
                sources,
            )?;
        }
        if nested {
            reserve_subscription_method(
                &mut methods,
                field_name(&field.name.value),
                coordinate,
                field.name.span,
                sources,
            )?;
        }
    }
    Ok(())
}

fn reserve_subscription_method(
    methods: &mut BTreeMap<String, String>,
    method: String,
    coordinate: String,
    span: Span,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    let Some(previous) = methods.insert(method.clone(), coordinate.clone()) else {
        return Ok(());
    };
    Err(diagnostic_error(
        Diagnostic::new(
            NAME_COLLISION,
            format!(
                "subscription resolver method `{method}` collides for `{previous}` and `{coordinate}`"
            ),
            span,
        ),
        sources,
    ))
}

fn validate_arguments(ty: &SemanticType, sources: Option<&SourceMap>) -> Result<(), CodegenError> {
    for field in ty.fields() {
        let mut arguments = IdentifierScope::default();
        for argument in &field.arguments {
            arguments
                .reserve(&argument.name, IdentifierCase::Snake)
                .map_err(|diagnostic| diagnostic_error(diagnostic, sources))?;
        }
    }
    Ok(())
}

pub(super) fn validate_public_names(
    schema: &SemanticSchema,
    type_names: &BTreeMap<String, String>,
    bound_enums: &::std::collections::BTreeSet<String>,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    let mut used = BTreeMap::<String, String>::new();
    let mut binding_methods = IdentifierScope::default();
    reserve_public(&mut used, "SchemaTypes", "framework item", None, sources)?;
    reserve_public(&mut used, "Resolvers", "framework item", None, sources)?;
    reserve_public(
        &mut used,
        "ResolverStage",
        "framework private item",
        None,
        sources,
    )?;
    reserve_public(
        &mut used,
        "ExecutableSchema",
        "framework item",
        None,
        sources,
    )?;
    reserve_public(
        &mut used,
        "ExecutableSchemaBuilder",
        "framework item",
        None,
        sources,
    )?;
    for ty in user_types(schema) {
        let rust = &type_names[ty.name()];
        let span = ty.name_span();
        if matches!(
            ty.kind(),
            TypeKind::Enum | TypeKind::InputObject | TypeKind::Interface | TypeKind::Union
        ) {
            reserve_public(&mut used, rust, ty.name(), span, sources)?;
        }
        if ty.kind() == TypeKind::Enum && bound_enums.contains(ty.name()) {
            binding_methods
                .reserve(&source_name(ty)?, IdentifierCase::Snake)
                .map_err(|diagnostic| diagnostic_error(diagnostic, sources))?;
        }
        if ty.kind() == TypeKind::Object {
            reserve_object_names(&mut used, ty, rust, span, sources)?;
        }
    }
    Ok(())
}

fn reserve_object_names(
    used: &mut BTreeMap<String, String>,
    ty: &SemanticType,
    rust: &str,
    span: Option<Span>,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    for suffix in ["Adapter", "Resolver"] {
        reserve_public(used, &format!("{rust}{suffix}"), ty.name(), span, sources)?;
    }
    for field in ty
        .fields()
        .iter()
        .filter(|field| !field.arguments.is_empty())
    {
        reserve_public(
            used,
            &format!("{rust}{}Args", variant_name(&field.name.value)),
            &format!("{}.{}", ty.name(), field.name.value),
            Some(field.name.span),
            sources,
        )?;
    }
    Ok(())
}

pub(super) fn resolver_method_names(
    schema: &SemanticSchema,
    sources: Option<&SourceMap>,
) -> Result<BTreeMap<String, String>, CodegenError> {
    let mut methods = BTreeMap::<String, String>::new();
    let mut names = BTreeMap::new();
    for ty in user_types(schema).filter(|ty| ty.kind() == TypeKind::Object) {
        let method = crate::naming::prefixed_field_method("with_", ty.name());
        if let Some(previous) = methods.insert(method.clone(), ty.name().to_owned()) {
            let span = ty.name_span().ok_or_else(|| {
                CodegenError::new("SF4007", format!("The `{}` has no source span.", ty.name()))
            })?;
            return Err(diagnostic_error(
                Diagnostic::new(
                    NAME_COLLISION,
                    format!(
                        "resolver method name `{method}` collides for `{previous}` and `{}`",
                        ty.name()
                    ),
                    span,
                ),
                sources,
            ));
        }
        names.insert(ty.name().to_owned(), method);
    }
    Ok(names)
}

fn reserve_public(
    used: &mut BTreeMap<String, String>,
    rust: &str,
    graphql: &str,
    span: Option<Span>,
    sources: Option<&SourceMap>,
) -> Result<(), CodegenError> {
    if let Some(previous) = used.insert(rust.to_owned(), graphql.to_owned()) {
        let message = format!(
            "The generated public name `{rust}` collides for `{previous}` and `{graphql}`."
        );
        return match span {
            Some(span) => Err(diagnostic_error(
                Diagnostic::new(NAME_COLLISION, message, span),
                sources,
            )),
            None => Err(CodegenError::new("SF4006", message)),
        };
    }
    Ok(())
}

pub(super) fn source_name(ty: &SemanticType) -> Result<Name, CodegenError> {
    let span = ty.name_span().ok_or_else(|| {
        CodegenError::new("SF4007", format!("The `{}` has no source span.", ty.name()))
    })?;
    Ok(Name::new(ty.name().to_owned(), span))
}

pub(super) fn diagnostic_error(
    diagnostic: Diagnostic,
    sources: Option<&SourceMap>,
) -> CodegenError {
    let rendered = sources.map_or_else(
        || format!("{}: {}", diagnostic.code, diagnostic.message),
        |sources| diagnostic.render(sources),
    );
    CodegenError::from_message(rendered)
}
