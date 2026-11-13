//! Apply field lookup, defaults, and one-of rules to input objects.

use std::collections::{BTreeMap, BTreeSet};

use crate::language::ast::{TypeRefKind, Value as AstValue};
use crate::schema::SemanticType;
use crate::{Object, Span, Value};

use super::engine::{Engine, Source};
use super::{
    CoercedInput, INPUT_DUPLICATE_FIELD, INPUT_OBJECT_VALUE, INPUT_ONE_OF_VALUE,
    INPUT_REQUIRED_VALUE_MISSING, INPUT_UNKNOWN_FIELD,
};

enum Provided<'value> {
    Raw(Value),
    Literal(&'value crate::language::ast::ValueNode),
}

impl<'value> Provided<'value> {
    fn into_source(self) -> Source<'value> {
        match self {
            Self::Raw(value) => Source::Raw(value),
            Self::Literal(value) => Source::Literal(value),
        }
    }
}

pub(super) fn coerce(
    engine: &mut Engine<'_>,
    named: &SemanticType,
    source: Source<'_>,
    depth: usize,
) -> Option<CoercedInput> {
    let (mut provided, object_span) = collect(engine, named, source)?;
    let mut positions = BTreeSet::new();
    for (name, value) in &provided {
        if named.input_field(name).is_some() {
            if let Some(position) = engine.input_field_position(named, name) {
                positions.insert(position);
            }
        } else {
            engine.report(
                INPUT_UNKNOWN_FIELD,
                format!("Input object `{}` has no field `{name}`", named.name()),
                provided_span(value).or(object_span),
            );
        }
        if !engine.can_continue() {
            return None;
        }
    }
    positions.extend(engine.active_input_positions(named));
    let one_of = named
        .directives()
        .iter()
        .any(|directive| directive.name.value == "oneOf");
    let mut one_of_values = Vec::new();
    let mut one_of_coercion_failed = false;
    let mut output = Object::with_capacity(positions.len());
    let mut valid = true;
    for position in positions {
        super::plan::record_occurrence_field();
        let field = &named.input_fields()[position];
        if !engine.reserve(0, field.name.value.len(), Some(field.span)) {
            return None;
        }
        engine.push_path(field.name.value.clone());
        let mut value = match provided.remove(&field.name.value) {
            Some(provided) => {
                engine.coerce(provided.into_source(), &field.ty, depth.saturating_add(1))
            }
            None => Some(CoercedInput::Undefined),
        };
        if matches!(value, Some(CoercedInput::Undefined)) {
            value = field.default_value.as_ref().and_then(|default| {
                engine.coerce(Source::Literal(default), &field.ty, depth.saturating_add(1))
            });
            if field.default_value.is_none() {
                value = Some(CoercedInput::Undefined);
            }
        }
        match value {
            Some(CoercedInput::Value(value)) => {
                if one_of {
                    one_of_values.push(true);
                }
                output.insert(field.name.value.clone(), value);
            }
            Some(CoercedInput::Null) => {
                if one_of {
                    one_of_values.push(false);
                }
                output.insert(field.name.value.clone(), Value::Null);
            }
            Some(CoercedInput::Undefined)
                if matches!(field.ty.kind, TypeRefKind::NonNull(_))
                    && field.default_value.is_none() =>
            {
                valid = false;
                engine.report(
                    INPUT_REQUIRED_VALUE_MISSING,
                    format!(
                        "Input object `{}` requires field `{}`",
                        named.name(),
                        field.name.value
                    ),
                    object_span,
                );
            }
            Some(CoercedInput::Undefined) => {}
            None => {
                valid = false;
                one_of_coercion_failed = true;
            }
        }
        engine.pop_path();
        if !engine.can_continue() {
            valid = false;
            break;
        }
    }
    if one_of && !one_of_coercion_failed && (one_of_values.len() != 1 || !one_of_values[0]) {
        valid = false;
        engine.report(
            INPUT_ONE_OF_VALUE,
            format!(
                "OneOf input object `{}` requires exactly one non-null field",
                named.name()
            ),
            object_span,
        );
    }
    valid.then_some(CoercedInput::Value(Value::Object(output)))
}

fn collect<'value>(
    engine: &mut Engine<'_>,
    named: &SemanticType,
    source: Source<'value>,
) -> Option<(BTreeMap<String, Provided<'value>>, Option<Span>)> {
    match source {
        Source::Raw(Value::Object(object)) => Some((
            object
                .into_iter()
                .map(|(name, value)| (name, Provided::Raw(value)))
                .collect(),
            None,
        )),
        Source::Literal(node) => {
            let AstValue::Object(fields) = &node.value else {
                engine.report(
                    INPUT_OBJECT_VALUE,
                    format!("Expected an input object value for `{}`", named.name()),
                    Some(node.span),
                );
                return None;
            };
            let mut provided = BTreeMap::new();
            for field in fields {
                if provided
                    .insert(field.name.value.clone(), Provided::Literal(&field.value))
                    .is_some()
                {
                    engine.report(
                        INPUT_DUPLICATE_FIELD,
                        format!(
                            "Input field `{}` is provided more than once",
                            field.name.value
                        ),
                        Some(field.name.span),
                    );
                }
                if !engine.can_continue() {
                    return None;
                }
            }
            Some((provided, Some(node.span)))
        }
        Source::Raw(_) => {
            engine.report(
                INPUT_OBJECT_VALUE,
                format!("Expected an input object value for `{}`", named.name()),
                None,
            );
            None
        }
    }
}

fn provided_span(value: &Provided<'_>) -> Option<Span> {
    match value {
        Provided::Literal(value) => Some(value.span),
        Provided::Raw(_) => None,
    }
}
