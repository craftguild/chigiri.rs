//! Normalizes JSON responses into leaf differences and structural markers.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{ComparisonCase, Difference};
use crate::differential::allowlist::difference_marker;

pub(super) fn compare_case(
    case: &ComparisonCase<'_>,
    differences: &mut Vec<Difference>,
    consumed_volatile: &mut BTreeSet<(String, String)>,
) {
    compare_value(
        case.case_id,
        "/status",
        &Value::from(case.old.status),
        &Value::from(case.new.status),
        differences,
        case.volatile_extension_pointers,
        consumed_volatile,
    );
    compare_value(
        case.case_id,
        "/mediaType",
        &Value::from(case.old.media_type.clone()),
        &Value::from(case.new.media_type.clone()),
        differences,
        case.volatile_extension_pointers,
        consumed_volatile,
    );
    for name in case.header_names {
        compare_value(
            case.case_id,
            &format!("/headers/{}", pointer_segment(name)),
            &serde_json::to_value(case.old.header_values(name)).unwrap(),
            &serde_json::to_value(case.new.header_values(name)).unwrap(),
            differences,
            case.volatile_extension_pointers,
            consumed_volatile,
        );
    }
    compare_value(
        case.case_id,
        "/body",
        &case.old.body,
        &case.new.body,
        differences,
        case.volatile_extension_pointers,
        consumed_volatile,
    );
}

fn compare_value(
    case_id: &str,
    pointer: &str,
    old: &Value,
    new: &Value,
    differences: &mut Vec<Difference>,
    volatile_pointers: &[String],
    consumed_volatile: &mut BTreeSet<(String, String)>,
) {
    if old != new && consume_volatile(case_id, pointer, volatile_pointers, consumed_volatile) {
        return;
    }
    match (old, new) {
        (Value::Object(old), Value::Object(new)) => {
            let keys = old.keys().chain(new.keys()).collect::<BTreeSet<_>>();
            for key in keys {
                let child = format!("{pointer}/{}", pointer_segment(key));
                match (old.get(key), new.get(key)) {
                    (Some(old), Some(new)) => compare_value(
                        case_id,
                        &child,
                        old,
                        new,
                        differences,
                        volatile_pointers,
                        consumed_volatile,
                    ),
                    (old, new) => compare_optional(
                        case_id,
                        &child,
                        old,
                        new,
                        differences,
                        volatile_pointers,
                        consumed_volatile,
                    ),
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            for index in 0..old.len().max(new.len()) {
                compare_optional(
                    case_id,
                    &format!("{pointer}/{index}"),
                    old.get(index),
                    new.get(index),
                    differences,
                    volatile_pointers,
                    consumed_volatile,
                );
            }
        }
        _ if old == new => {}
        _ if is_container(old) || is_container(new) => {
            record_difference(
                case_id,
                pointer.to_owned(),
                normalized_value(old),
                normalized_value(new),
                differences,
            );
            compare_container_children(
                case_id,
                pointer,
                old,
                new,
                differences,
                volatile_pointers,
                consumed_volatile,
            );
        }
        _ => record_difference(
            case_id,
            pointer.to_owned(),
            old.clone(),
            new.clone(),
            differences,
        ),
    }
}

fn compare_container_children(
    case_id: &str,
    pointer: &str,
    old: &Value,
    new: &Value,
    differences: &mut Vec<Difference>,
    volatile_pointers: &[String],
    consumed_volatile: &mut BTreeSet<(String, String)>,
) {
    let old_children = container_children(old);
    let new_children = container_children(new);
    let segments = old_children
        .keys()
        .chain(new_children.keys())
        .collect::<BTreeSet<_>>();
    for segment in segments {
        compare_optional(
            case_id,
            &format!("{pointer}/{segment}"),
            old_children.get(segment).copied(),
            new_children.get(segment).copied(),
            differences,
            volatile_pointers,
            consumed_volatile,
        );
    }
}

fn compare_optional(
    case_id: &str,
    pointer: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    differences: &mut Vec<Difference>,
    volatile_pointers: &[String],
    consumed_volatile: &mut BTreeSet<(String, String)>,
) {
    if old != new && consume_volatile(case_id, pointer, volatile_pointers, consumed_volatile) {
        return;
    }
    if let (Some(old), Some(new)) = (old, new) {
        compare_value(
            case_id,
            pointer,
            old,
            new,
            differences,
            volatile_pointers,
            consumed_volatile,
        );
        return;
    }
    record_difference(
        case_id,
        pointer.to_owned(),
        old.map_or_else(missing_value, normalized_value),
        new.map_or_else(missing_value, normalized_value),
        differences,
    );
    if let Some(old) = old {
        expand_container(
            case_id,
            pointer,
            old,
            true,
            differences,
            volatile_pointers,
            consumed_volatile,
        );
    }
    if let Some(new) = new {
        expand_container(
            case_id,
            pointer,
            new,
            false,
            differences,
            volatile_pointers,
            consumed_volatile,
        );
    }
}

fn expand_container(
    case_id: &str,
    pointer: &str,
    value: &Value,
    old_side: bool,
    differences: &mut Vec<Difference>,
    volatile_pointers: &[String],
    consumed_volatile: &mut BTreeSet<(String, String)>,
) {
    let children: Vec<(String, &Value)> = match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| (pointer_segment(key), value))
            .collect(),
        Value::Array(array) => array
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => return,
    };
    for (segment, value) in children {
        let child = format!("{pointer}/{segment}");
        let (old, new) = if old_side {
            (Some(value), None)
        } else {
            (None, Some(value))
        };
        compare_optional(
            case_id,
            &child,
            old,
            new,
            differences,
            volatile_pointers,
            consumed_volatile,
        );
    }
}

fn container_children(value: &Value) -> BTreeMap<String, &Value> {
    match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| (pointer_segment(key), value))
            .collect(),
        Value::Array(array) => array
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn record_difference(
    case_id: &str,
    pointer: String,
    old: Value,
    new: Value,
    differences: &mut Vec<Difference>,
) {
    differences.push(Difference {
        case_id: case_id.to_owned(),
        pointer,
        old,
        new,
    });
}

fn consume_volatile(
    case_id: &str,
    pointer: &str,
    volatile_pointers: &[String],
    consumed: &mut BTreeSet<(String, String)>,
) -> bool {
    if volatile_pointers.iter().any(|value| value == pointer) {
        consumed.insert((case_id.to_owned(), pointer.to_owned()));
        true
    } else {
        false
    }
}

fn is_container(value: &Value) -> bool {
    value.is_object() || value.is_array()
}

fn normalized_value(value: &Value) -> Value {
    match value {
        Value::Object(_) => difference_marker("object"),
        Value::Array(_) => difference_marker("array"),
        _ => value.clone(),
    }
}

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn missing_value() -> Value {
    difference_marker("missing")
}
