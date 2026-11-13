//! Rejects reserved boolean and null enum names, including programmatically constructed ASTs.

use super::super::TypeKind;
use super::super::composer::Composer;

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        if composer.schema.types[index].kind != TypeKind::Enum {
            continue;
        }
        let type_name = composer.schema.types[index].name.clone();
        let values = composer.schema.types[index].enum_values.clone();
        for value in values {
            if !matches!(value.name.value.as_str(), "true" | "false" | "null") {
                continue;
            }
            composer.report(
                super::super::SCHEMA_RESERVED_ENUM_VALUE,
                format!(
                    "Enum `{type_name}` value `{}` is reserved and cannot be defined",
                    value.name.value
                ),
                value.name.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}
