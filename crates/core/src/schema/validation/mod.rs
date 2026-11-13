//! Applies GraphQL Section 3 static-structure rules sequentially to the composed schema.

mod directive_cycles;
mod directive_metadata;
mod enum_values;
mod input_cycles;
mod input_default_cycles;
mod input_values;
mod interfaces_covariance;
mod interfaces_fields;
mod interfaces_graph;
mod member_shape;
mod one_of;
mod reserved_names;
mod scc;
mod type_positions;
mod unions;

use super::composer::Composer;

pub(super) fn validate_structure(composer: &mut Composer) {
    reserved_names::validate(composer);
    if composer.can_continue() {
        member_shape::validate(composer);
    }
    if composer.can_continue() {
        let diagnostic_count = composer.diagnostics.len();
        type_positions::validate(composer);
        if composer.can_continue() && composer.diagnostics.len() == diagnostic_count {
            input_values::validate(composer);
        }
    }
    if composer.can_continue() {
        interfaces_graph::validate(composer);
    }
    if composer.can_continue() {
        interfaces_fields::validate(composer);
    }
    if composer.can_continue() {
        unions::validate(composer);
    }
    if composer.can_continue() {
        input_cycles::validate(composer);
    }
    if composer.can_continue() {
        input_default_cycles::validate(composer);
    }
    if composer.can_continue() {
        one_of::validate(composer);
    }
    if composer.can_continue() {
        directive_metadata::validate(composer);
    }
    if composer.can_continue() {
        directive_cycles::validate(composer);
    }
    if composer.can_continue() {
        enum_values::validate(composer);
    }
}
