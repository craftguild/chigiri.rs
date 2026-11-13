//! Requires every union member to be a unique, defined object type after composition.

use std::collections::BTreeSet;

use super::super::TypeKind;
use super::super::composer::Composer;

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        if composer.schema.types[index].kind != TypeKind::Union {
            continue;
        }
        let union_name = composer.schema.types[index].name.clone();
        let members = composer.schema.types[index].union_members.clone();
        let mut seen = BTreeSet::new();
        for member in members {
            if !seen.insert(member.value.clone()) {
                composer.report(
                    super::super::SCHEMA_DUPLICATE_MEMBER,
                    format!(
                        "Union `{union_name}` includes member `{}` more than once",
                        member.value
                    ),
                    member.span,
                );
                if !composer.can_continue() {
                    return;
                }
            }
            let valid_object = composer
                .schema
                .type_by_name(&member.value)
                .is_some_and(|named_type| named_type.kind() == TypeKind::Object);
            if !valid_object {
                composer.report(
                    super::super::SCHEMA_INVALID_UNION_MEMBER,
                    format!(
                        "Union `{union_name}` member `{}` must be a defined object type",
                        member.value
                    ),
                    member.span,
                );
            }
            if !composer.can_continue() {
                return;
            }
        }
    }
}
