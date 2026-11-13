# GraphQL September 2025 v1 conformance matrix

Historical execution records and implementation prompts are retained privately.
Public ADRs, this document and explicit test fixtures define the current
contract; historical Passed statements do not establish a current gate result.
See the [documentation index](README.md) and run the current validation gates.

The [ADR-0059 compiler amendment](adr/0059-craftguild-standards-adoption.md#rust-195-unification-amendment-2026-11-07)
requires fixed Rust 1.95.0 and minimum Rust 1.95 for all current first-party
normal/generated/CLI/Mock paths. Earlier compiler matrices and Passed tables
below, including CH-10, preserve their historical scope; the current policy
and source-bound ci/full evidence supersede their compiler-lane requirements.

Status: Normative implementation checklist
Specification: [GraphQL September 2025](https://spec.graphql.org/September2025/)

Section 15 defines the generated-crate, CLI, and validation boundaries from
ADR-0026 and ADR-0027. Section 16 defines ADR-0028 semantic generated layout
and handwritten line limits. Historical ABI 5, module-output, legacy CLI, and
package traces remain baseline evidence without weakening GraphQL semantics.
Section 17 covers the independent Mock application experiment from ADR-0029;
it is distinct from GraphQL specification requirements. Section 18 covers the
ADR-0030 runtime-schema/Mock IR contract and its independent validation.

Section 41/ADR-0054 defines the locally accepted subscription implementation.
Current source validates and executes subscriptions at ABI 8;
the earlier Parse/Reject boundary is historical. The dedicated validation
record identifies actual executed scopes, final source identities and excluded qualification.

## How to read this matrix

- `Required`: required for v1 completion.
- `Parse/Reject`: the parser recognizes the syntax or definition, but v1
  returns an unsupported diagnostic.
- `Deferred`: v1 provides no parser- or executor-specific semantics.
- Each `Required` row tracks the test module/function or fixture path when implemented.
- Do not mark a broad chapter complete from one test. Each normative rule needs
  appropriate positive and negative cases.

## 1. Language

| Spec | Item | v1 status | Evidence |
| --- | --- | --- | --- |
| 2.1 | Source Text / Unicode | Required | `source::tests::rejects_invalid_utf8_before_adding_a_source`; `lexer::tests_string::quoted_strings_preserve_bom_characters`; `lexer::tests::ignores_bom_outside_strings_and_treats_it_as_a_comment_boundary` |
| 2.1.1–2.1.2 | White Space / Line Terminators | Required | `lexer::tests::{discards_ignored_tokens_and_crlf,treats_standalone_cr_and_lf_as_line_terminators,accepts_only_ascii_whitespace}` |
| 2.1.3 | Comments | Required | `lexer::tests_security::long_comments_are_scanned_to_eof_with_bounded_work` |
| 2.1.4 | Insignificant Commas | Required | `parser::tests_executable::fields_aliases_arguments_and_fragments_preserve_all_three_source_forms_in_order` |
| 2.1.5–2.1.7 | Lexical / Ignored Tokens / Punctuators | Required | `lexer::tests::{recognizes_punctuators_and_spread_tokens,spread_token_uses_the_longest_match,adjacent_names_report_an_error_without_joining_the_numeric_lexeme}` |
| 2.1.8 | Names | Required | `lexer::tests::{ascii_names_use_maximal_munch,rejects_unicode_outside_names_and_preserves_the_byte_span}`; `parser::tests_grammar_edges::accepts_keyword_names_and_explicit_anonymous_operations`; `schema::tests_ast_invariants::rejects_invalid_public_ast_names_before_semantic_conversion` |
| 2.2 | Descriptions | Required | `parser::type_system::tests_description::{preserves_descriptions_for_every_definition_kind,preserves_full_interface_and_member_description_syntax}`; `execution_contract::descriptions::operation_variable_fragment_description_does_not_change_the_execution_results` |
| 2.3 | Document | Required | `parser::tests_document::{generic_parser_preserves_both_document_domains_in_source_order,domain_specific_parsers_reject_the_opposite_domain_with_stable_codes}` |
| 2.4 | Operations | Query/Mutation/Subscription Required | `parser::tests_executable::shorthand_and_three_operations`; `tests_definitions::operation_selection_accepts_validated_subscriptions`; Section 41 |
| 2.5–2.9 | Selection Sets / Fields / Arguments / Alias / Fragments | Required | `parser::tests_executable::fields_aliases_arguments_and_fragments_preserve_all_three_source_forms_in_order`; `parser::tests_negative::{rejects_empty_documents_selections_and_argument_lists,rejects_incomplete_selections_fragments_and_type_references}` |
| 2.10 | Input Values | Required | `parser::tests_executable::preserves_all_input_value_lexemes_and_string_styles`; `lexer::tests::{integer_and_float_tokens_preserve_source_lexemes,rejects_leading_zeroes,requires_digits_in_fraction_and_exponent_parts}` |
| 2.11 | Variables | Required | `parser::tests_executable::parses_variable_types_defaults_and_directives`; `parser::tests_negative::rejects_variables_recursively_in_const_positions` |
| 2.12 | Type References | Required | `parser::tests_negative::rejects_incomplete_selections_fragments_and_type_references`; `parser::type_system::tests_rejection::rejects_double_non_null_and_descriptions_before_extensions_while_preserving_origins` |
| 2.13 | Directives | Required | `parser::tests_executable::parses_variable_types_defaults_and_directives`; `parser::type_system::tests_definition::directive_definitions_preserve_all_locations_in_source_order` |
| 2.14 | Schema Coordinates | Required for metadata | `schema::coordinate::{tests_parser,tests_resolver}`; `crates/core/tests/schema_contract.rs#public_api_parses_and_resolves_schema_coordinates` |

### Lexer/parser security cases

- Parser nesting limits produce diagnostics before stack exhaustion.
- Unterminated strings, block strings, and comments produce diagnostics without
  panicking.
- Invalid numbers, incomplete fractions or exponents, and invalid name
  adjacency are rejected.
- Quoted-string line terminators, invalid Unicode escapes, and invalid surrogate
  pairs are rejected. Other Unicode scalar values, including control
  characters where permitted by the grammar, are accepted.
- Processing stops at the configured diagnostic limit.

Checked-in evidence is in `lexer::tests_security`, `parser::tests_limits`,
`parser::tests_security::short_input_corpus_respects_input_and_diagnostic_limits_without_panicking`,
and `crates/core/tests/language_contract.rs`.

## 2. Type system

| Spec | Item | v1 status | Evidence |
| --- | --- | --- | --- |
| 3.1 | Type System Extensions | Required | `schema::tests_composition::{extensions_may_precede_definitions_across_documents,merges_minimal_extensions_into_the_incremental_index}` |
| 3.2 | Descriptions | Required | `parser::type_system::tests_description::preserves_descriptions_for_every_definition_kind`; `schema::tests_introspection_metadata::exposes_rich_sdl_metadata_without_loss` |
| 3.3 | Schema | Required | `schema::tests_composition::distinguishes_implicit_and_explicit_root_origins`; `schema::tests_errors::reports_all_root_composition_and_subscription_errors` |
| 3.3.2 | Schema Extension | Required | `schema::tests_errors::validates_schema_definitions_and_extension_targets`; `schema::tests_directive_applications::rejects_undefined_invalid_location_and_repeated_nonrepeatable_directives` |
| 3.4 | Types | Required | `schema::tests_structural::{rejects_reserved_prefixes_for_all_user_defined_schema_elements,validates_nested_wrapper_base_types_in_input_and_output_positions,classifies_scalar_enum_and_input_types_by_input_and_output_capability}` |
| 3.4.3 | Type Extensions | Required | `schema::tests_errors::{reports_the_missing_extension_member_kind,duplicate_definitions_members_and_directives_use_stable_codes,duplicate_members_across_all_type_kinds_use_the_same_stable_code}`; `schema::tests_structural::an_extension_can_supply_the_only_member_of_a_type` |
| 3.5 | Scalars | Required | `schema::tests_builtins::injects_specified_builtins_in_stable_order_and_supports_lookup`; `schema::tests_directive_metadata::specified_by_is_allowed_only_on_custom_scalars` |
| 3.5.1 | Int | Required | `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`; `executor::output::tests::builtin_scalar_output_is_coerced_to_the_required_type_and_range` |
| 3.5.2 | Float | Required | `tests_value_rules::float_integer_literals_require_exact_f64_representation`; `executor::output::tests::{integer_to_float_coercion_requires_exact_representability,builtin_scalars_reject_containers_null_and_nonfinite_values}` |
| 3.5.3 | String | Required | `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`; `executor::output::tests::builtin_scalar_output_is_coerced_to_the_required_type_and_range` |
| 3.5.4 | Boolean | Required | `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`; `executor::output::tests::builtin_scalar_output_is_coerced_to_the_required_type_and_range` |
| 3.5.5 | ID | Required | `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`; `executor::output::tests::builtin_scalar_output_is_coerced_to_the_required_type_and_range` |
| 3.6 | Objects | Required | `schema::tests_structural::rejects_every_memberless_named_composite_type`; `schema::tests_errors::{duplicate_definitions_members_and_directives_use_stable_codes,duplicate_members_across_all_type_kinds_use_the_same_stable_code}` |
| 3.6.2 | Field Deprecation | Required | `schema::tests_directive_metadata::{rejects_deprecation_on_required_inputs_without_defaults_across_argument_kinds,deprecated_input_values_may_be_nullable_or_have_defaults}` |
| 3.7 | Interfaces | Required | `schema::tests_interfaces_graph::{resolves_direct_and_transitive_interfaces_in_the_composed_graph,objects_and_interfaces_include_transitive_interfaces}`; `schema::tests_interfaces_fields::accepts_covariant_returns_optional_extra_arguments_and_matching_deprecation` |
| 3.8 | Unions | Required | `schema::tests_structural::{union_members_must_be_unique_defined_objects_after_composition,union_members_preserve_composed_sources}` |
| 3.9 | Enums | Required | `schema::tests_enum_values::{rejects_reserved_enum_values_in_definitions_extensions_and_public_ast,enum_values_are_not_reserved_literals}` |
| 3.10 | Input Objects | Required | `schema::tests_input_cycles::{accepts_nullable_and_list_cycle_breaks_and_acyclic_shared_children,direct_required_singular_cycles_are_reported_at_the_field_span}` |
| 3.10.1 | OneOf | Required | `schema::tests_one_of::{one_of_definitions_and_extensions_apply_the_same_field_constraints,valid_one_of_extensions_require_nullable_fields_without_defaults}`; `tests_input_coercion::diagnoses_unknown_required_and_one_of_input_fields`; `tests_value_rules::one_of_literal_requires_exactly_one_non_null` |
| 3.10 | Input Object default cycles | Required | `schema::tests_input_default_cycles::{accepts_explicit_null_empty_lists_and_shared_branches,rejects_direct_default_cycles_at_the_closing_default_span,indirect_cycles_diagnose_each_closing_default}` |
| 3.11 | List | Required | `tests_input_coercion::coerces_defaults_singleton_lists_and_enums`; `execution_contract::completion::{null_list_items_preserve_their_index_paths,non_null_item_bubbles_up_to_nullable_list}` |
| 3.12 | Non-Null | Required | `schema::tests_manual_ast::rejects_hand_built_ast_with_double_non_null_wrappers`; `execution_contract::completion::{non_null_list_failures_bubble_to_the_root,nested_non_null_stops_at_the_nearest_nullable_object}` |
| 3.13 | Directives | Required | `schema::tests_directive_applications::{accepts_all_type_system_locations_and_builtin_directives,repeatable_directives_preserve_source_order_across_definitions_and_extensions}`; `schema::tests_directive_cycles::{rejects_recursive_directive_definitions,rejects_direct_self_referential_directive_cycles}` |
| 3.13.1 | `@skip` | Required semantics | `execution_contract::collection::skip_include_and_fragment_type_conditions_preserve_source_locations` |
| 3.13.2 | `@include` | Required semantics | `execution_contract::collection::nullable_null_include_variable_excludes_the_field` |
| 3.13.3 | `@deprecated` | Required metadata | `schema::tests_introspection_metadata::exposes_rich_sdl_metadata_without_loss`; `execution_contract::introspection::september_2025_standard_introspection_query_executes_successfully` |
| 3.13.4 | `@specifiedBy` | Required metadata | `schema::tests_directive_metadata::specified_by_is_allowed_only_on_custom_scalars`; `execution_contract::introspection::september_2025_standard_introspection_query_executes_successfully` |
| 3.13.5 | `@oneOf` | Required semantics | `schema::tests_one_of::valid_one_of_extensions_require_nullable_fields_without_defaults`; `tests_value_rules::one_of_literal_requires_exactly_one_non_null` |

WP-2 checked-in evidence covers:

- stable document/source ordering of members, directive applications, and
  origins across definitions and extensions;
- composition, extensions, roots, duplicates, and subscription rejection in
  `schema::tests_composition`, `schema::tests_errors`, and
  `crates/core/tests/schema_contract.rs`;
- descriptions and lossless introspection metadata in
  `schema::tests_introspection_metadata` and
  `schema::tests_introspection_descriptors`;
- type/member shapes, reserved names, input/output positions, wrappers, and
  manual-AST preflight in `schema::tests_structural`,
  `schema::tests_manual_ast`, and `schema::tests_ast_invariants`;
- interface graph, covariance, argument, and deprecation compatibility in
  `schema::tests_interfaces_graph` and `schema::tests_interfaces_fields`,
  including indexed comparison of 5,000 arguments and membership indexes for
  4,000 union members and 4,000 interface fields;
- unions, enums, input objects, and OneOf in `schema::tests_structural`,
  `schema::tests_enum_values`, and `schema::tests_one_of`;
- required singular and default cycles in `schema::tests_input_cycles` and
  `schema::tests_input_default_cycles`, including iterative 20,000-node,
  branching-DAG, and 10,000-default-field cases;
- schema defaults and SDL directive values in
  `schema::tests_const_values_scalars`,
  `schema::tests_const_values_objects`, and
  `schema::tests_const_values_directives`; and
- directive definitions, applications, locations, repeatability, cycles, and
  built-in metadata in `schema::tests_directive_applications`,
  `schema::tests_directive_cycles`, `schema::tests_directive_metadata`, and
  `schema::tests_builtins`, including 10,000 directive arguments, 10,000
  optional arguments across 10,000 applications, and 10,000 duplicate
  locations across 10,000 applications.

Variable, argument, custom-scalar, result-coercion, `@skip`, and `@include`
execution semantics are traced to the execution tests in Sections 4 and 5.

## 3. Introspection

| Spec | Item | v1 status | Evidence |
| --- | --- | --- | --- |
| 4.1 | `__typename` | Required, always enabled | `execution_contract::introspection::typename_is_always_active_and_only_schema_and_type_are_rejected_by_policy`; `execution_contract::completion::interfaces_and_unions_complete_selections_with_concrete_typenames` |
| 4.2 | `__schema` / `__type` | Required, policy controlled | `execution_contract::introspection::{typename_is_always_active_and_only_schema_and_type_are_rejected_by_policy,type_lookup_resolves_variable_and_type_wrappers_without_an_application_resolver}` |
| 4.2.1 | `__Schema` | Required | `schema::tests_introspection_descriptors::schema_and_type_introspection_field_signatures_are_exact`; `execution_contract::introspection::september_2025_standard_introspection_query_executes_successfully` |
| 4.2.2 | `__Type` | Required | `schema::tests_introspection_descriptors::{schema_and_type_introspection_field_signatures_are_exact,preserves_september_2025_enum_value_order}` |
| 4.2.3 | `__Field` | Required | `schema::tests_introspection_descriptors::member_descriptors_preserve_exact_field_signatures`; `schema::tests_introspection_metadata::exposes_rich_sdl_metadata_without_loss` |
| 4.2.4 | `__InputValue` | Required | `schema::tests_introspection_descriptors::{member_descriptors_preserve_exact_field_signatures,include_deprecated_defaults_to_false_in_all_five_locations}` |
| 4.2.5 | `__EnumValue` | Required | `schema::tests_introspection_descriptors::preserves_september_2025_enum_value_order`; `schema::tests_introspection_metadata::exposes_rich_sdl_metadata_without_loss` |
| 4.2.6 | `__Directive` | Required | `schema::tests_introspection_descriptors::member_descriptors_preserve_exact_field_signatures`; `schema::tests_introspection_metadata::exposes_rich_sdl_metadata_without_loss` |
| September 2025 | Introspection additions | Required | `schema::tests_introspection_descriptors::include_deprecated_defaults_to_false_in_all_five_locations`; `schema::tests_one_of::valid_one_of_extensions_require_nullable_fields_without_defaults`; `execution_contract::introspection::september_2025_standard_introspection_query_executes_successfully` |

The golden response test includes the standard introspection query. When
production introspection policy is disabled, `__typename` remains available,
while `__schema` and `__type` produce field errors.

## 4. Executable validation

| Spec | Rule | v1 status | Evidence |
| --- | --- | --- | --- |
| 5.1.1 | Executable Definitions | Required | `tests_definitions::{executable_input_rejects_type_system_definitions,selects_operations_by_reusing_the_validated_document}` |
| 5.2.1.1 | Operation Type Existence | Query/Mutation/Subscription Required | `tests_fields::diagnoses_a_missing_root_for_the_operation_type`; `tests_subscriptions::reports_a_missing_subscription_root` |
| 5.2.2.1 | Operation Name Uniqueness | Required | `tests_definitions::rejects_duplicate_operation_and_fragment_names_and_non_lone_anonymous_operations` |
| 5.2.3.1 | Lone Anonymous Operation | Required | `tests_definitions::rejects_duplicate_operation_and_fragment_names_and_non_lone_anonymous_operations` |
| 5.2.4.1 | Single Root Field | Required | `tests_subscriptions::{rejects_multiple_response_name_groups,aliases_and_merged_fields_form_one_response_name_group,collects_named_and_inline_fragments_using_root_applicability,rejects_conditional_directives_across_root_traversal_forms,rejects_an_aliased_introspection_root_field}` |
| 5.3.1 | Field Selections | Required | `tests_fields::distinguishes_field_shape_from_argument_structure`; `tests_fragments::september_2025_standard_introspection_query_can_be_documented_and_verified` |
| 5.3.2 | Field Selection Merging | Required | `tests_fields::{detects_collisions_across_alias_arguments_and_nested_selections,mutually_exclusive_object_fragments_do_not_conflict,merges_equivalent_input_objects_and_arguments_in_different_orders,nested_merges_preserve_diagnostics_before_later_siblings}` |
| 5.3.3 | Leaf Field Selections | Required | `tests_fields::distinguishes_field_shape_from_argument_structure` |
| 5.4.1 | Argument Names | Required | `tests_fields::distinguishes_field_shape_from_argument_structure` |
| 5.4.2 | Argument Uniqueness | Required | `tests_fields::distinguishes_field_shape_from_argument_structure` |
| 5.4.3 | Required Arguments | Required | `tests_fields::required_argument_errors_preserve_schema_diagnostics` |
| 5.5.1.1 | Fragment Name Uniqueness | Required | `tests_definitions::rejects_duplicate_operation_and_fragment_names_and_non_lone_anonymous_operations` |
| 5.5.1.2 | Fragment Spread Type Existence | Required | `tests_fragments::type_condition_is_limited_to_defined_composite` |
| 5.5.1.3 | Fragments on Object, Interface or Union Types | Required | `tests_fragments::{type_condition_is_limited_to_defined_composite,introspection_objects_are_valid_conditions_for_named_and_inline_fragments}` |
| 5.5.1.4 | Fragments Must Be Used | Required | `tests_fragments::rejects_fragment_cycles_reachable_from_an_operation` |
| 5.5.2.1 | Fragment Spread Target Defined | Required | `tests_fragments::rejects_undefined_fragment_spread_targets` |
| 5.5.2.2 | Fragment Spreads Must Not Form Cycles | Required | `tests_fragments::{detects_fragment_cycles_with_iterative_depth_first_search,detects_a_ten_thousand_fragment_cycle_without_stack_recursion,rejects_fragment_cycles_reachable_from_an_operation}` |
| 5.5.2.3 | Fragment Spread Is Possible | Required | `tests_fragments::fragment_spread_validity_uses_intersecting_possible_object_sets` |
| 5.6.1 | Values of Correct Type | Required | `tests_value_rules::{validates_builtin_enum_input_object_and_list_item_values,directive_arguments_use_the_same_literal_validation_rules,passes_custom_scalar_constant_literals_to_the_callback}` |
| 5.6.2 | Input Object Field Names | Required | `tests_value_rules::distinguishes_input_object_shape_from_missing_required_fields` |
| 5.6.3 | Input Object Field Uniqueness | Required | `tests_value_rules::rejects_null_custom_scalar_results_and_duplicate_object_fields` |
| 5.6.4 | Input Object Required Fields | Required | `tests_value_rules::distinguishes_input_object_shape_from_missing_required_fields` |
| 5.7.1 | Directives Are Defined | Required | `tests_fields::validates_directive_locations_repetition_and_argument_structure` |
| 5.7.2 | Directives Are in Valid Locations | Required | `tests_fields::validates_directive_locations_repetition_and_argument_structure` |
| 5.7.3 | Directives Are Unique per Location | Required | `tests_fields::validates_directive_locations_repetition_and_argument_structure` |
| 5.8.1 | Variable Uniqueness | Required | `tests_variables::distinguishes_duplicate_non_input_undefined_and_unused_variables` |
| 5.8.2 | Variables Are Input Types | Required | `tests_variables::distinguishes_duplicate_non_input_undefined_and_unused_variables` |
| 5.8.3 | All Variable Uses Defined | Required | `tests_variables::{distinguishes_duplicate_non_input_undefined_and_unused_variables,attributes_fragment_variable_use_to_the_operation}` |
| 5.8.4 | All Variables Used | Required | `tests_variables::{distinguishes_duplicate_non_input_undefined_and_unused_variables,attributes_fragment_variable_use_to_the_operation}` |
| 5.8.5 | All Variable Usages Are Allowed | Required | `tests_value_rules::{validates_non_null_defaults_after_matching_wrappers,fragment_variable_usage_is_validated_against_the_operation_definition,direct_one_of_variables_must_be_declared_non_null,distinguishes_variable_uses_with_the_same_span_by_node_identity}` |

Field-selection merging has dedicated fixtures for aliases, argument
equivalence, mutually exclusive object types, and nested merges. OneOf's
exactly-one/non-null runtime rule is traced from §3.10.1 and §6.1.2, and direct
OneOf variables are subject to the non-null rules in §5.8.5.

## 5. Execution/coercion

| Spec | Item | v1 status | Evidence |
| --- | --- | --- | --- |
| 6.1 | Executing Requests | Required | `crates/core/tests/executable_contract.rs#public_api_supports_separate_validation_selection_and_variable_coercion`; `execution_contract::{distinguishes_request_errors_from_nullable_non_null_completion,aliases_preserve_nested_object_response_order}` |
| 6.1.1 | Validating Requests | Required | `crates/core/tests/executable_contract.rs#public_api_supports_separate_validation_selection_and_variable_coercion`; `tests_definitions::selects_operations_by_reusing_the_validated_document` |
| 6.1.2 | Coercing Variable Values | Required | `tests_input_coercion::{coerces_defaults_singleton_lists_and_enums,diagnoses_unknown_required_and_one_of_input_fields,distinguishes_missing_null_and_defaulted_variables_from_singleton_lists,custom_scalar_callbacks_receive_the_name_and_normalized_value,undefined_variables_use_argument_defaults}`; `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`; `execution_contract::limits::zero_diagnostic_limit_still_returns_one_variable_coercion_error` |
| 6.2.1 | Query | Required | `execution_contract::concurrency::{query_siblings_poll_concurrently_and_preserve_source_order_despite_reverse_completion,concurrent_fields_do_not_start_resolver_futures_beyond_the_limit}` |
| 6.2.2 | Mutation | Required | `execution_contract::concurrency::mutation_roots_start_each_resolver_after_the_previous_field_completes`; `execution_contract::mutation_nested::nested_mutation_fields_use_normal_concurrent_execution` |
| 6.2.3 | Subscription | Required | `executor::subscription::tests`; `crates/runtime/tests/subscription_contract.rs`; Section 41 and its source-bound validation map |
| 6.3.1 | Root Selection Set | Required | `execution_contract::aliases_preserve_nested_object_response_order`; `execution_contract::completion::interfaces_and_unions_complete_selections_with_concrete_typenames` |
| 6.3.2 | Field Collection | Required | `execution_contract::collection::{skip_include_and_fragment_type_conditions_preserve_source_locations,nullable_null_include_variable_excludes_the_field,fields_with_the_same_response_key_call_the_resolver_once,deep_fragment_chains_expand_without_call_stack_recursion}` |
| 6.3.3 | Field Execution | Required | `execution_contract::directives::exposes_all_executable_locations_as_directive_metadata_with_source_sites`; `execution_contract::middleware::middleware_enters_in_order_exits_in_reverse_and_releases_coerced_context` |
| 6.3.4 | Normal and Serial Execution | Required | `execution_contract::concurrency::{query_siblings_poll_concurrently_and_preserve_source_order_despite_reverse_completion,mutation_roots_start_each_resolver_after_the_previous_field_completes}`; `execution_contract::mutation_nested::nested_mutation_fields_use_normal_concurrent_execution`; `execution_contract::poll_set::{immediately_ready_bounded_query_completes_on_the_first_outer_poll,pending_child_wakes_the_outer_future_and_completes_on_the_follow_up_poll}`; `execution_contract::cancellation::non_null_bubbling_drops_unnecessary_pending_futures` |
| 6.4.1 | Coercing Field Arguments | Required | `tests_input_coercion::{distinguishes_missing_null_and_defaulted_variables_from_singleton_lists,undefined_variables_use_argument_defaults}`; `execution_contract::introspection::{null_variable_for_non_null_type_name_argument_produces_a_field_error,null_variable_for_non_null_include_deprecated_argument_nulls_the_list}` |
| 6.4.2 | Resolving Fields | Required | `execution_contract::dispatch_integrity::{startup_rejects_return_type_changes_for_the_same_coordinate,startup_rejects_argument_signature_changes_for_the_same_coordinate}`; `generation::generated_public_contracts_compile_and_missing_ones_are_compile_errors` |
| 6.4.3 | Value Completion | Required | `execution_contract::completion::{builtin_scalar_output_errors_become_masked_field_errors,interfaces_and_unions_complete_selections_with_concrete_typenames,null_list_items_preserve_their_index_paths,non_null_item_bubbles_up_to_nullable_list}` |
| 6.4.4 | Handling Execution Errors | Required | `execution_contract::distinguishes_request_errors_from_nullable_non_null_completion`; `execution_contract::completion::bubbling_preserves_sibling_errors_completed_before_the_bubble`; `execution_contract::cancellation::non_null_bubbling_drops_unnecessary_pending_futures` |
| 7.1 | Response Format | Required | `execution_contract::aliases_preserve_nested_object_response_order`; `execution_contract::distinguishes_request_errors_from_nullable_non_null_completion` |
| 7.1.6 | Errors | Required | `execution_contract::completion::{builtin_scalar_output_errors_become_masked_field_errors,null_list_items_preserve_their_index_paths}`; `execution_contract::middleware::only_a_field_plugin_can_observe_the_internal_source_and_convert_the_result` |

### Execution concurrency tests

- Both sibling query-field futures are polled before either completes.
- A future beyond `max_concurrent_fields` does not start.
- Query completion preserves response-field source order even when futures finish
  in reverse order.
- Mutation root field B does not start until root field A and its nested
  completion finish.
- Fields below one mutation root field may execute concurrently.
- Pending futures made unnecessary by non-null bubbling are dropped.

## 6. Middleware/cache/runtime policy

| Item | Required v1 verification |
| --- | --- |
| Request middleware | `execution_contract::middleware::{middleware_enters_in_order_exits_in_reverse_and_releases_coerced_context,each_phase_can_short_circuit_its_inner_chain}` |
| Operation middleware | `execution_contract::middleware::{middleware_enters_in_order_exits_in_reverse_and_releases_coerced_context,each_phase_can_short_circuit_its_inner_chain}` |
| Field middleware | `execution_contract::middleware::{middleware_enters_in_order_exits_in_reverse_and_releases_coerced_context,only_a_field_plugin_can_observe_the_internal_source_and_convert_the_result}` |
| Consuming `Next` | `executor::plugin::{RequestNext::run,OperationNext::run,FieldNext::run}` consume `self`; public compile checks enforce the boundary. |
| Concurrent plugin call | `execution_contract::concurrency::field_plugins_are_loaded_safely_during_concurrent_execution` |
| Document cache | `cache::tests::{does_not_share_entries_between_schema_instances,oversized_keys_and_disabled_caches_store_no_entries,entry_limit_evicts_the_oldest_validated_document}`; `execution_contract::cache::cached_documents_reselect_operations_and_apply_each_requests_variables_independently` |
| Error masking | `execution_contract::middleware::only_a_field_plugin_can_observe_the_internal_source_and_convert_the_result` |
| Introspection policy | `execution_contract::introspection::typename_is_always_active_and_only_schema_and_type_are_rejected_by_policy` |
| Limits | `execution_limits::tests::{production_defaults_are_bounded,unbounded_limits_are_an_explicit_opt_in}`; `tests_complexity::depth_field_alias_and_directive_limits_use_distinct_codes`; `execution_contract::limits::{field_unit_list_overrides_the_masked_error_to_null_the_field,request_output_node_limit_applies_to_nested_completion}`; `execution_contract::output_diagnostics::{direct_field_list_errors_are_correlated_with_masked_ids_and_sources,direct_output_node_errors_are_correlated_with_masked_ids_and_sources,introspection_output_node_limits_correlate_with_common_diagnostics}` |

## 7. HTTP conformance

HTTP conformance follows the
[GraphQL over HTTP Stage 2 draft at `eae19ff`](https://github.com/graphql/graphql-over-http/blob/eae19ff930de8b1cde8511b16843175b0e537e66/spec/GraphQLOverHTTP.md),
not the GraphQL language specification.

The 400/422 request-error classification follows the pinned draft's SHOULD.
For field errors and partial data, the draft's normative 294 status conflicts
with its non-normative 200 example; v1 uses 200 for interoperability and does
not adopt 294. ADR-0004 contains the normative rationale.

Chigiri conformance covers the neutral endpoint after routing and its opt-in
bridges. The application server owns the listener, route mismatches, header-read timeouts,
connection limits, signals, and shutdown/drain; Chigiri does not convert these
into protocol failures. Application examples such as books may provide
integration evidence, but those responsibilities are outside the bridge public
contract.

| Case | Expected |
| --- | --- |
| POST JSON Query | 200 with a GraphQL response: `endpoint_conformance::handles_query_mutation_get_and_field_error_responses`. |
| POST JSON Mutation | 200 with a GraphQL response: `endpoint_conformance::handles_query_mutation_get_and_field_error_responses`. |
| GET Query | 200 with a GraphQL response: `endpoint_conformance::handles_query_mutation_get_and_field_error_responses`. |
| GET Mutation | 405: `endpoint_conformance::maps_media_shape_and_get_mutation_failures_to_status_codes`. |
| Method token | Only case-sensitive `GET` and `POST` are accepted: `request_decode::maps_content_type_accept_and_method_failures_to_status_codes`. |
| Missing/wrong Content-Type | 415: `request_decode::maps_content_type_accept_and_method_failures_to_status_codes`; `endpoint_conformance::body_preflight_and_read_timeouts_bound_unfinished_streams`. |
| Unsupported Accept / parameter | 406, or a matching legacy media range: `media::tests::{exact_zero_quality_is_more_important_than_wildcard,media_parameter_match_and_specificity_precede_quality}`. |
| Malformed JSON syntax / URI encoding | 400: `request_decode::{json_syntax_failures_return_400,maps_content_type_accept_and_method_failures_to_status_codes}`. |
| Invalid request shape | 422: `request_decode::json_syntax_failures_return_400`. |
| JSON numeric input | `request_decode::decodes_json_numbers_with_empty_fractions_as_lossless_integers` verifies lossless decoding before Int, Float, and ID coercion; `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion` verifies the scalar boundary and canonical ID conversion. |
| Document parse error with `graphql-response+json` | 400; legacy JSON uses 200: `response_encode::request_errors_use_400_or_422_for_graphql_media_and_200_for_legacy_json`; `endpoint_conformance::maps_media_shape_and_get_mutation_failures_to_status_codes`. |
| Validation / operation selection / variable coercion with `graphql-response+json` | 422: `response_encode::request_errors_use_400_or_422_for_graphql_media_and_200_for_legacy_json`; `endpoint_conformance::maps_media_shape_and_get_mutation_failures_to_status_codes`; `input_coercion::tests_scalars::applies_builtin_scalar_boundaries_and_canonical_id_conversion`. |
| Resolver error/partial data | 200: `endpoint_conformance::handles_query_mutation_get_and_field_error_responses`; this is the documented v1 choice for the draft's 294/200 conflict. |
| Oversized body | 413 with buffering stopped at the configured limit: `endpoint_conformance::body_limit_timeout_and_context_failures_return_graphql_errors`. |
| Body read timeout | 408 with `REQUEST_BODY_TIMEOUT`: `endpoint_conformance::body_preflight_and_read_timeouts_bound_unfinished_streams`. |
| Header read timeout | Application-server responsibility; outside endpoint/bridge acceptance. |
| Context factory timeout | 504 with `REQUEST_TIMEOUT`; the context future is dropped: `endpoint_conformance::body_limit_timeout_and_context_failures_return_graphql_errors`. |
| Execution timeout | 504 with `REQUEST_TIMEOUT`; the execution future is dropped: `endpoint_conformance::body_limit_timeout_and_context_failures_return_graphql_errors`. |
| Connection limit | Application-server responsibility; outside endpoint/bridge acceptance. |
| Serialization failure | Masked 500 using the negotiated `application/graphql-response+json` media: `endpoint_conformance::returns_a_fallback_response_when_serialization_fails`. |
| Oversized response | The bounded writer stops at the byte limit and returns a masked 500: `endpoint_conformance::response_byte_limit_returns_a_masked_internal_error`. |
| Graceful shutdown | Application-server responsibility, covered only by consumer integration evidence. |

## 8. Explicitly deferred

- Subscription source/execution and WebSocket protocols
- Federation directives/entity resolution/composition
- `@defer`/`@stream` validation and incremental transport
- APQ/document allow-list
- Batch requests
- Multipart upload
- GraphOS reporting/inline trace
- Live queries

## 9. vNext adoption-profile conformance

Status: Required. ADR-0007 sections 5–10 define the exact behavior, and section
15 defines the acceptance IDs.

| ID | Status | Required target |
| --- | --- | --- |
| SDL-01 | Required | Built-in import tests verify the five built-ins and preserve descriptor fingerprints without input drift. |
| SDL-02 | Required | Built-in restatements that differ only in argument or location order are accepted. |
| SDL-03 | Required | Built-in equivalence ignores description and block-string notation differences. |
| SDL-04 | Required | A nullable `if` conflict is rejected with SF2004 at the name span while existing output remains unchanged. |
| SDL-05 | Required | A changed `@deprecated` default is rejected with SF2004 at the name span while existing output remains unchanged. |
| RES-01 | Required | The ABI 6 vNext consumer, resolver formula, and `scripts/check-vnext-adoption.sh` verify typestate structure and single evaluation. |
| RES-02 | Required | Generation and compile fixtures reject early build, duplicate registration, and wrong registration order with stable fragments. |
| RES-03 | Required | The wrong-trait fixture fails compilation with the stable typestate diagnostic. |
| ENUM-01 | Required | Without configuration, enum-binding tests generate an owned enum under ABI 6. |
| ENUM-02 | Required | The 65-enum consumer compiles; generic conversion masks mapping failures. |
| ENUM-03 | Required | Invalid config shape returns SF4011 and preserves existing output. |
| ENUM-04 | Required | Invalid binding names return SF4012 and preserve existing output. |
| ENUM-05 | Required | Input/output mapping failures preserve path and source masking in the enum-binding consumer. |
| ENUM-06 | Required | Comment-only changes preserve the descriptor while changing config digest and drift classification. |
| AXUM-01 | Required | Endpoint and lifecycle suites cover queries, mutations, GET, field errors, and the shared Axum endpoint. |
| AXUM-02 | Required | Endpoint media/status tests, the Axum application router, and the differential corpus verify mapping. |
| AXUM-03 | Required | The shared Axum lifecycle suite covers source, decode, and context rejection. |
| AXUM-04 | Required | The differential corpus and strict allowlist cover auth, context, and multiple cookies. |
| AXUM-05 | Required | Shared endpoint tests bound body preflight, unfinished-stream timeouts, body limits, and context failures. |
| AXUM-06 | Required | Endpoint tests drop response-hook and execution futures on overall timeout. |
| AXUM-07 | Required | Direct, Hyper, and Axum suites enforce common behavior and `ResponseHeaderAuthority`. |
| DIFF-01 | Required | The synthetic differential corpus has zero structural differences outside the allowlist and includes ADR-0014 ROOT-01 through ROOT-15. |

The Axum adapter follows the Section 7 pinned GraphQL over HTTP draft and uses
400/422 for request errors and 200 for field errors. Route rejection remains an
application-router concern rather than a Chigiri codec concern. ADR-0007 defines
body, hook, context, execution, and overall timeout precedence. Section 8.5 of
that ADR is an additional required contract.

ADR-0014 fixes the response-member root contract as part of DIFF-01:

| ID | Required target |
| --- | --- |
| ROOT-01 | Positive differential tests preserve one marker for `data: null` versus an empty object. |
| ROOT-02 | Positive tests preserve the direct transition from `data: null` to an empty object. |
| ROOT-03 | Positive tests preserve non-empty objects at the root and every descendant. |
| ROOT-04 | Positive tests detect omitted descendants of non-empty objects. |
| ROOT-05 | Positive tests distinguish objects and arrays at the root and every child. |
| ROOT-06 | Positive tests distinguish missing, empty, and non-empty `errors` arrays. |
| ROOT-07 | Positive tests distinguish missing, null, and object `extensions`. |
| ROOT-08 | Positive tests preserve exact markers for missing `data` versus scalar null. |
| ROOT-09 | Negative tests reject scalar and null-scalar forms at both entry points. |
| ROOT-10 | Negative tests reject whole-body markers at both entry points. |
| ROOT-11 | Negative tests reject custom response members at both entry points. |
| ROOT-12 | Negative tests reject raw object and array snapshots at both entry points. |
| ROOT-13 | Negative tests reject non-canonical marker and pointer forms. |
| ROOT-14 | Negative tests reject unused extra markers and leaf markers. |
| ROOT-15 | Negative regression tests reject duplicate, unknown, and case-variant markers. |

## 10. Production platform conformance

Status: Required for production readiness. ADR-0004 and ADR-0008 through
ADR-0013 define the exact semantics and execution order; ADR-0009 section 2.2
defines the handler boundary. The handler-only revision of 2026-08-27 made the
prior DX, LIFE, and TEST server evidence subject to revalidation.

| ID | Required target |
| --- | --- |
| DX-01 | The books end-to-end suite and `scripts/check-books-quick-start.sh` cover checked-in schema, query, mutation, errors, and media behavior. |
| DX-02 | Codegen public-contract fixtures compile complete generated implementations and reject missing implementations. |
| DX-03 | The ABI 6 vNext consumer verifies resolver typestate. |
| DX-04 | `chigiri_testkit::developer_contract::same_endpoint_runs_direct_hyper_axum_without_server_api` |
| DX-05 | Server code generation excludes client artifacts. |
| DX-06 | `chigiri_testkit::developer_contract::public_api_has_no_product_types` |
| LIFE-01 | Common direct, Hyper, and Axum lifecycle suites verify request start. |
| LIFE-02 | The same suites verify ordered phase entry and exit. |
| LIFE-03 | The same suites verify rejection and failure results. |
| LIFE-04 | Common suites and `chigiri_testkit::public_contract::bridges_expose_no_listener_route_or_shutdown` verify the bridge boundary, with eight compile-negative fixtures. |
| LIFE-05 | Direct execution starts with the request plugin rather than an HTTP phase. |
| LIFE-06 | The HTTP production transport contract remains object safe and independent of product body models. |
| LIFE-07 | Common direct, Hyper, and Axum suites verify cancellation. |
| LIFE-08 | Common direct, Hyper, and Axum suites verify completion and close behavior. |
| OP-01 | `chigiri_operation::identity::op_01_calculates_a_stable_digest_from_exact_document_bytes` |
| OP-02 | Operation identity, runtime limits, and Axum tests require a selection name for multi-operation documents and return a bounded validation error for oversized names. |
| OP-03 | The catalog rejects duplicate IDs, invalid documents, unknown schemas, and schema mismatches. |
| OP-04 | APQ hash mismatch is rejected before store writes. |
| OP-05 | Persisted-operation tests distinguish APQ unknown from safelist unknown and compare opaque aliases with raw-document digests. |
| OP-06 | ID-only requests reject text and resolve known IDs to the exact stored document. |
| OP-07 | Provider timeouts drop the future and preserve the configured failure policy. |
| OP-08 | Debug/public errors omit raw queries, IDs, and provider sources; audit records preserve digest, revision, hit/miss, and sink failures. |
| OP-09 | `chigiri_testkit::developer_contract::endpoint_runs_without_client_tooling` |
| BUDGET-01 | Cost tests independently enforce structural and operation budgets, coordinate overrides, bounded integer multipliers, and literal/variable list multipliers before resolver start. |
| BUDGET-02 | Cost tests reject checked overflow, including alias/fragment/variable expansion, schema defaults, and nested cumulative list ratios, before execution. |
| RATE-01 | Stage A returns 429 and aligned `Retry-After` before reading the body. |
| RATE-02 | Stage B rejection does not roll back Stage A when no resolver starts. |
| RATE-03 | An over-capacity cost is rejected at construction rather than producing an impossible retry, and concurrent consumption is atomic. |
| RATE-04 | `chigiri_testkit::rate_conformance::rate_04_timeout_drop_and_fail_open_closed` and `crates/testkit/tests/rate_conformance/stage_b.rs#rate_04_stage_b_timeout_drops_the_provider_and_applies_the_failure_policy` verify future cancellation and fail-open/fail-closed policy. |
| RATE-05 | Stage A response tests preserve 429 body ordering and `Retry-After`. |
| RATE-06 | Transport debug output omits tokens and quota values. |
| RATE-07 | Capacity and expiry are bounded; capacity exhaustion is an explicit rejection rather than a provider failure; and a same-key policy mismatch is rejected without mutating the bucket. |
| RATE-08 | Production structural limits remain active when no limiter is configured. |
| OBS-01 | Lifecycle, endpoint, and runtime tests close every phase in fixed order for success, rejection, timeout, execution destruction, and cancellation. |
| OBS-02 | Request/error ID factory failures notify observers without inventing IDs and preserve response semantics. |
| OBS-03 | Runtime and HTTP output-diagnostic tests correlate client `errorId` and observer records while masking sources for independent failures and limit errors. |
| OBS-04 | Observer failures and full queues do not change GraphQL response semantics. |
| OBS-05 | Default debug, HTTP encoding, lifecycle, and OpenTelemetry tests exclude queries, variables, headers, IP/PII, sources, secrets, and identity values while preserving span relationships and error recording. |
| OBS-06 | Metric labels use a closed key set with at most eight labels and a 64-byte value limit. |
| OBS-07 | Queue tests cover nonblocking overflow/drop counts, FIFO behavior, disconnects, and concurrent producers delivering up to capacity while preserving cause-specific counts and FIFO order. |
| OBS-08 | `scripts/check-observability-contract.sh` verifies the `observability-opt-in-consumer` dependency fixture. |
| OBS-09 | Field instrumentation emits success events only when explicitly enabled. |
| GOV-02 | Governance conformance revalidates every operation by exact ID and reports result-shape and cost deltas. |
| GOV-04 | `chigiri_testkit::governance_conformance::runtime_and_readiness_metadata_match` |
| TEST-01 | Common endpoint lifecycle suites and `scripts/check-neutral-endpoint-contract.sh` verify transport neutrality. |
| TEST-02 | Third-party operation/rate provider suites cover public contracts, timeout cancellation, clock reversal/expiry, malformed metadata rejection, and atomic concurrent behavior. |
| TEST-03 | The strict differential allowlist suite and Axum synthetic corpus require zero unapproved structural differences. |
| TEST-04 | `scripts/check-fuzz-regressions.sh` runs six committed corpora. |
| REL-01 | `scripts/check-cargo-consumer.sh#consumer-quality-five-commands` verifies repository consumers with MSRV/stable fmt, check, test, Clippy, and rustdoc. |
| REL-02 | `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_nested_legacy_and_record_mismatch_without_input_values` verifies current marker, ABI, and provenance consistency; ADR-0027 removed distribution-only archive and SBOM checks. |
| REL-03 | The three-review finding/re-review table in section 5 of the production review record. |

REL-01 and REL-02 are the ADR-0027 transfer points in the current contract; the
historical review's executable evidence remains historical. S4o in the
sibling generator validation record
documents the move and removal boundary. REL-03 remains the historical
three-review record. SG-VERIFY-04 independently governs reviews A/B for the
current program.

ADR-0048 retired GOV-01 and GOV-03 with the product-owned schema-wide
snapshot/diff facility. GOV-02 catalog-impact validation and GOV-04 runtime
identity remain active and independent of that removed facility.

## 11. Custom scalar output governance

Status: Required / Implemented. ADR-0015 is the normative source. The dedicated
ABI 5 fixture is generated and executed from
`crates/codegen/tests/compile/custom_scalar_output`; codegen public-contract
tests verify that the generated paths compile.

| ID | Required target |
| --- | --- |
| CSO-01 | Direct and HTTP tests preserve every generated custom-scalar JSON type and response order. |
| CSO-02 | The same direct and HTTP tests preserve custom object/list scalar output order. |
| CSO-03 | Direct and HTTP tests reject built-in scalar values with invalid container or null shapes. |
| CSO-04 | Governance tests mask nested numeric and invalid-value failures without reserving an output node. |
| CSO-05 | Depth zero and depth exactly at the configured limit succeed; limit plus one fails, with HTTP masking/bubbling intact. |
| CSO-06 | Output deeper than 10,000 levels is rejected and destroyed without stack overflow; HTTP behavior remains masked. |
| CSO-07 | Empty and nested node boundaries do not double-count the root. |
| CSO-08 | The same node accounting applies through HTTP with normal bubbling and masking. |
| CSO-09 | Root and nested list items share the configured item limit, including the HTTP path. |
| CSO-10 | `chigiri_custom_scalar_output::governance::cso_10_concurrent_sibling_does_not_oversubscribe_to_global_node_budget` and the corresponding HTTP test share one global budget. |
| CSO-11 | Nullable and non-null custom scalar failures preserve ordinary null bubbling through direct and HTTP execution. |
| CSO-12 | Direct and HTTP serialization diagnostics preserve ID, source, path, coordinate, and masking rules. |
| CSO-13 | Encoded length exactly at the limit succeeds; a limit one byte too small returns a masked 500. |
| CSO-14 | Custom scalar inputs parse object/list containers to the same shape through direct and HTTP execution. |
| CSO-15 | Serialization field errors follow the existing field-error path for direct and HTTP execution. |

## 12. Developer usability program conformance

Status: Required / Implemented / Gate U-A to U-E Passed. The 54-ID test oracle is connected to individual exact evidence.
A finding reported on 2026-08-30 was
closed through a focused regression, the full suite, reviews A/B, a fresh-clone
check, and re-evaluation of the affected gate.
The applicable ADRs and section 12 of `implementation-handoff.md` define the
exact semantics, implementation order, and gates.

| ID | Required evidence |
| --- | --- |
| API-01 | `chigiri_testkit::usability::endpoint_builder::api_01_single_builder_reaches_query_mutation_get` |
| API-02 | `chigiri_testkit::usability::endpoint_builder::api_02_applies_safe_baseline_without_additional_calls` |
| API-03 | `chigiri_testkit::usability::endpoint_builder::api_03_accepts_resolvers_and_schema_on_the_same_builder`, `chigiri_testkit::usability::built_schema_graph::built_schema_input_does_not_inherit_the_source_plugin`, `chigiri_testkit::usability::built_schema_graph::built_schema_input_removes_the_sampler_and_observer_from_source`, `chigiri_testkit::usability::built_schema_graph::built_schema_input_keeps_the_catalog_and_replaces_the_error_observer`, `chigiri_testkit::usability::built_schema_graph::generated_and_built_schema_inputs_produce_the_same_final_graph`, `chigiri_custom_scalar_output::direct::built_schema_input_and_custom_scalar_record` |
| API-04 | `chigiri_testkit::usability::surface::api_04_second_endpoint_build_does_not_expose_an_additional_entry_point` |
| API-05 | `chigiri_testkit::usability::context_and_bridges::api_05_context_typestate_compiles_and_runs` |
| API-06 | `chigiri_testkit::usability::endpoint_builder::api_06_validates_request_id_configuration_at_build_and_request_time`, `chigiri_testkit::usability::request_identity::request_id_factory_is_initialized_at_build_and_called_once_for_the_first_request`, `chigiri_testkit::usability::request_identity::request_ids_are_unique_during_initial_parallel_generation`, `chigiri_testkit::usability::request_identity::one_waiting_factory_call_does_not_serialize_another_request`, `chigiri::endpoint::request_identity::tests::automatic_factory_initializes_and_probes_once_at_build_time`, `chigiri::endpoint::request_identity::tests::automatic_factory_build_failure_returns_identity_initialization_error`, `chigiri::endpoint::request_identity::tests::primed_request_id_is_restricted_to_the_creator_process`, `scripts/check-usability-contract.sh` |
| API-07 | `chigiri_testkit::usability::endpoint_builder::api_07_rate_keeps_structural_limits_when_unconfigured` |
| API-08 | `chigiri_testkit::usability::context_and_bridges::api_08_all_inputs_use_the_same_endpoint_and_defaults` |
| API-09 | `chigiri_testkit::usability::surface::api_09_quick_start_builds_a_default_endpoint` |
| API-10 | `scripts/check-usability-contract.sh` |
| API-11 | `chigiri_testkit::usability::endpoint_builder::api_11_only_dangerous_api_can_weaken_the_baseline` |
| API-12 | `chigiri_books::end_to_end::api_12_short_construction_keeps_traditional_security_contract` |
| INJ-01 | `chigiri_testkit::usability::injection_equivalence::inj_01_matches_typed_injection` |
| INJ-02 | `chigiri_testkit::usability::injection_equivalence::inj_02_single_slot_uses_last_call` |
| INJ-03 | `chigiri_testkit::usability::injection_equivalence::inj_03_stages_coexist_and_only_replace_the_same_stage` |
| INJ-04 | `chigiri_testkit::usability::injection_equivalence::inj_04_multiple_injections_edit_the_same_graph` |
| INJ-05 | `chigiri_testkit::usability::injection_equivalence::inj_05_distinguishes_plugin_push_set_and_clear`, `chigiri_testkit::usability::built_schema_graph::built_schema_supports_component_set_and_clear_operations`, `chigiri_testkit::usability::built_schema_rate::built_schema_input_replaces_inherited_operation_plugin_with_stage_b`, `chigiri_testkit::usability::built_schema_rate::runs_stage_b_before_draft_plugins` |
| INJ-06 | `chigiri_testkit::usability::injection_equivalence::inj_06_cross_validates_only_the_final_graph`, `chigiri_testkit::usability::configuration_diagnostics::distinguishes_body_and_response_byte_configuration_slots`, `chigiri_testkit::usability::configuration_diagnostics::diagnoses_each_deadline_configuration_slot_individually`, `chigiri_testkit::usability::configuration_diagnostics::diagnoses_complex_configuration_slots_and_safety_override_reason` |
| INJ-07 | `chigiri_testkit::usability::surface::inj_07_endpoint_is_immutable_after_build` |
| INJ-08 | `chigiri_testkit::usability::surface::inj_08_does_not_expose_a_dynamic_registry` |
| INJ-09 | `chigiri_testkit::usability::injection_equivalence::inj_09_standard_setters_cannot_weaken_protection`, `chigiri_testkit::usability::configuration_bounds::inj_09_enforces_hard_bounds_for_both_byte_limits`, `chigiri_testkit::usability::configuration_bounds::inj_09_rejects_duration_max_for_all_phase_deadlines`, `chigiri_testkit::usability::configuration_bounds::inj_09_enforces_standard_hard_boundary_values`, `chigiri_axum::deadline::tests::unschedulable_deadline_completes_immediately`, `chigiri_hyper::deadline::tests::unschedulable_deadline_completes_immediately` |
| INJ-10 | `chigiri_testkit::usability::injection_equivalence::inj_10_effective_config_is_redacted_and_shows_origin`, `chigiri_testkit::usability::configuration_diagnostics::diagnostic_output_omits_configured_values_and_reasons` |
| INJ-11 | `chigiri_testkit::usability::surface::inj_11_rustdoc_and_ide_hide_advanced_input` |
| INJ-12 | `scripts/check-usability-contract.sh` |
| PROJ-01 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-02 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-03 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-04 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-05 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-06 | `scripts/check-usability-contract.sh` |
| PROJ-07 | `chigiri_projection_consumer::governance::trait_completion_and_policy_not_circumvented` |
| PROJ-08 | Retired by ADR-0053; the projection-specific requirement is historical. |
| PROJ-09 | `chigiri_cli::rust_traits::ri_01_08_private_adapters_and_assembly_stages_are_hidden_from_rustdoc` |
| PROJ-10 | `chigiri_cli::rust_traits::ri_07_detects_private_dispatch_drift_without_writes` |
| PROJ-11 | `chigiri_cli::rust_traits::ri_07_large_generated_semantic_units_keep_resource_bounds` (ABI 7 semantic-unit layout) |
| PROJ-12 | `scripts/check-large-consumer-conformance.sh` (ABI 7 MSRV/stable gate) |
| PROJ-13 | `scripts/check-usability-contract.sh` (ABI 7 inventory/surface and isolated mismatch fixture) |
| PROJ-14 | `chigiri_books::end_to_end::proj_14_project_quick_start_preserves_the_traditional_contract` |
| LIMIT-01 | `chigiri_policy::limiters::token_bucket::limit_01_checks_boundaries_retry_delays_and_excess_costs` |
| LIMIT-02 | `chigiri_policy::limiters::fixed_window::limit_02_checks_window_boundaries_and_concurrent_consumption` |
| LIMIT-03 | `chigiri_policy::limiters::sliding_counter::limit_03_checks_integer_rotation_overflow_and_retry_delays` |
| LIMIT-04 | `chigiri_policy::limiters::leaky_meter::limit_04_drains_without_allocating_queues_or_spawning_tasks` |
| LIMIT-05 | `chigiri_policy::limiters::store::limit_05_bounds_keys_and_expiry_for_all_four_algorithms`, `chigiri_policy::limiters::store::quota_rejection_keeps_active_keys_from_idle_expiry_for_all_four_algorithms` |
| LIMIT-06 | `chigiri_policy::limiters::store::limit_06_capacity_and_policy_mismatches_do_not_change_state` |
| LIMIT-07 | `chigiri_testkit::usability::rate::limit_07_failure_policy_default_and_override`, `chigiri_testkit::usability::rate_configuration::limit_07_reports_distinct_timeout_and_algorithm_duration_errors` |
| LIMIT-08 | `chigiri_testkit::usability::rate::limit_08_timeout_and_cancel_drop_provider` |
| LIMIT-09 | `chigiri_testkit::usability::rate::limit_09_preserves_stage_a_stage_b_and_rollback_contract` |
| LIMIT-10 | `chigiri_testkit::usability::rate::limit_10_convenience_injection_uses_the_same_precedence_as_explicit_stages` |
| LIMIT-11 | `chigiri_testkit::usability::rate::limit_11_shows_unconfigured_coverage_and_structural_limit` |
| LIMIT-12 | `chigiri_testkit::usability::rate::limit_12_redacts_quota_and_provider_information` |
| LIMIT-13 | `chigiri_policy::limiters::clock::limit_13_handles_clock_rollback_and_large_forward_jumps`, `chigiri_testkit::provider_conformance::time::third_party_rate_provider_protects_rollback_and_bounded_expiry_contracts` |
| LIMIT-14 | `chigiri_testkit::provider_conformance::limit_14_rejects_inconsistent_third_party_provider_decisions`, `chigiri_testkit::provider_conformance::concurrency::concurrent_suite_rejects_non_atomic_providers_including_single_poll`, `chigiri_testkit::provider_conformance::blocking::limit_14_watchdog_rejects_method_and_poll_blocking_providers` |
| LIMIT-15 | `chigiri_testkit::usability::rate::limit_15_separates_query_concurrency_from_time_quota` |
| LIMIT-16 | `scripts/check-usability-contract.sh` |

The historical review trace contains all 54 IDs. ADR-0053 retires PROJ-01
through PROJ-05 and PROJ-08; retained invariants now trace to ordinary traits.
When one test verifies multiple IDs, the trace still records each ID and its
individual assertion.

## 13. DX and rate boundary hardening conformance

Status: Required / Implemented / Gates D-A through D-E Passed. ADR-0020 through
ADR-0024 define the exact semantics; section 13 of
`implementation-handoff.md` defines the implementation order and gates.

The references below retain the post-ADR-0026 tests. DIAG-06 and DIAG-07 now
use an explicit manifest package and verify rejection of package/lockfile
overrides.

| ID | Exact test or script assertion |
| --- | --- |
| SCAF-01 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-02 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-03 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-04 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-05 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-06 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-07 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-08 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-09 | Retired by ADR-0053; scaffold acceptance remains historical. |
| SCAF-10 | Retired by ADR-0053; scaffold acceptance remains historical. |
| DIAG-01 | `crates/cli/tests/rust_diagnostics.rs#exposes_public_models_and_human_and_json_renderers` |
| DIAG-02 | `crates/cli/tests/rust_diagnostics.rs#exposes_public_models_and_human_and_json_renderers` |
| DIAG-03 | `crates/codegen/src/path_redaction.rs#redacts_empty_boundary_and_primary_absolute_path_forms` |
| DIAG-04 | `crates/cli/tests/rust_diagnostics.rs#diag_04_07_13_14_static_after_successfully_fixing_cargo_and_side_effects` |
| DIAG-05 | `crates/cli/tests/rust_diagnostics.rs#diag_05_drift_exits_one_without_starting_cargo` |
| DIAG-06 | `crates/codegen/tests/dx_diagnostics/workspace.rs#diag_06_rejects_unspecified_packages_in_virtual_workspaces` |
| DIAG-07 | `crates/codegen/tests/dx_diagnostics/tools.rs#diag_07_rejects_lockfile_updates_and_target_overrides` |
| DIAG-08 | `crates/codegen/src/doctor/correlation/tests.rs#diag_08_reports_missing_group_with_context` |
| DIAG-09 | `crates/codegen/src/doctor/correlation/tests.rs#diag_09_trait_output_shows_graphql_and_rust_types` |
| DIAG-10 | `crates/codegen/src/doctor/correlation/tests.rs#diag_10_requires_more_than_a_matching_compiler_message` |
| DIAG-11 | `scripts/check-large-consumer-conformance.sh#generated-drift/abi-mismatch` |
| DIAG-12 | `crates/codegen/tests/dx_diagnostics/tools.rs#diag_12_bounds_child_group_output_and_waits_for_completion` |
| DIAG-13 | `crates/cli/tests/rust_diagnostics.rs#diag_04_07_13_14_static_after_successfully_fixing_cargo_and_side_effects` |
| DIAG-14 | `crates/cli/tests/rust_diagnostics.rs#diag_04_07_13_14_static_after_successfully_fixing_cargo_and_side_effects` |
| DIAG-15 | `scripts/check-large-consumer-conformance.sh#MSRV/stable-positive-negative-doctor` |
| RSPI-01 | `scripts/check-rate-boundary-contract.sh#chigiri_rate-normal-dependency-zero-and-toolchains` |
| RSPI-02 | `crates/rate/tests/public_contract.rs#rspi_02_trait_is_object_safe_and_returns_a_borrowed_send_future` |
| RSPI-03 | `tests/fixtures/rate-provider-minimal/tests/conformance.rs#minimal_provider_satisfies_public_conformance_suites` |
| RSPI-04 | `scripts/check-rate-boundary-contract.sh#forbidden-SPI-surface-scan` |
| RSPI-05 | `crates/rate/tests/public_contract.rs#rspi_05_detects_cost_key_policy_and_decision_mismatches` |
| RSPI-06 | `crates/rate/tests/public_contract.rs#rspi_06_rejection_reasons_and_coordinate_snapshots_are_closed` |
| RSPI-07 | `crates/rate/tests/public_contract.rs#rspi_07_public_debug_and_provider_errors_redact_secret_and_quota_values` |
| RSPI-08 | `crates/testkit/tests/provider_conformance.rs#rspi_08_state_suite_does_not_consume_quota_when_the_provider_rejects` |
| RSPI-09 | `scripts/check-rate-boundary-contract.sh#policy-composition-owner-scan` |
| RSPI-10 | `scripts/check-rate-boundary-contract.sh#facade-rate-reexport-and-usability` |
| RSPI-11 | `scripts/check-rate-boundary-contract.sh#legacy-policy-SPI-zero` |
| RSPI-12 | `scripts/check-rate-boundary-contract.sh#generated-ABI-7` |
| RMEM-01 | `scripts/check-rate-boundary-contract.sh#chigiri_rate_memory-only-atomic-waker-rate-and-toolchains` |
| RMEM-02 | `crates/rate-memory/tests/configuration.rs#rmem_02_safe_defaults_work_with_every_constructor` |
| RMEM-03 | `crates/rate-memory/tests/configuration.rs#rmem_03_config_debug_redacts_values_and_rejects_zero_or_unbounded_durations` |
| RMEM-04 | `crates/rate-memory/tests/algorithms.rs#limit_01_checks_boundaries_retry_delays_and_excess_costs` |
| RMEM-05 | `crates/rate-memory/tests/algorithms.rs#limit_02_checks_window_boundaries_and_concurrent_consumption` |
| RMEM-06 | `crates/rate-memory/tests/algorithms.rs#limit_03_checks_integer_rotation_overflow_and_retry_delays` |
| RMEM-07 | `crates/rate-memory/tests/algorithms.rs#limit_04_drains_without_allocating_queues_or_spawning_tasks` |
| RMEM-08 | `crates/rate-memory/tests/algorithms/store_clock.rs#limit_05_bounds_keys_and_expiry_for_all_four_algorithms` |
| RMEM-09 | `crates/rate-memory/tests/algorithms/store_clock.rs#quota_rejection_keeps_active_keys_from_idle_expiry_for_all_four_algorithms` |
| RMEM-10 | `crates/rate-memory/tests/state_contract.rs#rmem_10_cancel_capacity_mismatch_and_overcost_do_not_change_state` |
| RMEM-11 | `crates/rate-memory/tests/state_contract.rs#rmem_11_all_four_providers_handle_clock_rollback_and_large_forward_jumps` |
| RMEM-12 | `scripts/check-rate-boundary-contract.sh#rmem-12-bounded-waiter-wake-drop-official` |
| RMEM-13 | `crates/testkit/tests/usability/rate_memory.rs#rmem_13_convenience_and_explicit_memory_injection_produce_the_same_stage` |
| RMEM-14 | `scripts/check-rate-boundary-contract.sh#legacy-memory-surface-zero` |
| RMEM-15 | `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately` |
| RMEM-16 | `docs/production-validation-runbook.md#formal-evidence-invalidation-and-human-rerun` |
| LCON-01 | `scripts/large-consumer-contract.py#shape-minimums` |
| LCON-02 | `scripts/large-consumer-contract.py#shape-markers-and-five-command-compile` |
| LCON-03 | `scripts/check-large-consumer-conformance.sh#codegen-and-drift` |
| LCON-04 | Retired by ADR-0053; scaffold determinism and placeholders are removed. |
| LCON-05 | `scripts/check-large-consumer-conformance.sh#MSRV/stable-positive-doctor` |
| LCON-06 | `scripts/large-consumer-contract.py#missing-group-field-root-diagnostics` |
| LCON-07 | `scripts/large-consumer-contract.py#wrong-output-diagnostic` |
| LCON-08 | `scripts/large-consumer-contract.py#generated-drift-and-abi-mismatch-diagnostics` |
| LCON-09 | `scripts/large-consumer-suite.sh#MSRV/stable-five-command-timeouts` |
| LCON-10 | `scripts/large-consumer-evidence.py#generated-size-file-line-metrics` |
| LCON-11 | `scripts/check-large-consumer-conformance.sh#public-rustdoc-hidden-scan` |
| LCON-12 | `scripts/check-large-consumer-conformance.sh#provider-third-party` |
| LCON-13 | `scripts/check-large-consumer-conformance.sh#provider-negative` |
| LCON-14 | `scripts/check-large-consumer-conformance.sh#provider-official` |
| LCON-15 | `scripts/test-large-consumer-conformance-gates.sh#closed-evidence-and-safe-publication-mutations` |
| LCON-16 | `scripts/check-large-consumer-conformance.sh#fixture-and-formal-performance-scope` |

The table preserves all 69 historical IDs. ADR-0053 retires SCAF-01 through
SCAF-10 and LCON-04; its RI requirements replace projection authoring while
retaining diagnostics, resources, execution policy, and the rate contracts. Gate D-E Passed, including reviews A/B and reproduction from a
fresh clone of the final commit.

## 14. Correlated operation audit identity conformance

The eight IDs in ADR-0025 are the closed acceptance set for the operation-audit
public contract. Section 14 of `implementation-handoff.md` is normative. Every
ID has a fully qualified test owner and may not be omitted, merged, or renamed.

Status: 8 ID implemented / Gate OA-A-OA-D Passed.

| ID | Exact test or script assertion | Expected |
| --- | --- | --- |
| AUDIT-ID-01 | `chigiri_operation::audit_identity::hit_identity` | A hit records the exact request, trace, and sampling identity with no operation identity. |
| AUDIT-ID-02 | `chigiri_operation::audit_identity::miss_identity` | A miss uses the same identity alongside the expected digest, revision, mode, and outcome. |
| AUDIT-ID-03 | `chigiri_operation::audit_identity::provider_failure_identity` | Provider unavailability records one audit and one observer error with the exact request/trace identity, Drop sampling, and no operation identity, then continues the request. |
| AUDIT-ID-04 | `chigiri_operation::audit_identity::sink_failure_identity` | Sink failure records an audit and observer error with the exact request/trace identity, Record sampling, and no operation identity, then continues the request. |
| AUDIT-ID-05 | `chigiri_operation::audit_identity::missing_telemetry` | Success, provider failure, and sink failure without telemetry use an empty/default identity, notify audit and observer exactly once, and continue the request. |
| AUDIT-ID-06 | `chigiri_operation::audit_identity::sampling_drop` | Sampling Drop still records the audit event exactly once. |
| AUDIT-ID-07 | `chigiri_operation::audit_identity::parallel_identity` | Parallel requests never cross-wire, omit, or overlap identity and digest values. |
| AUDIT-ID-08 | `chigiri_testkit::operation_http::audit_id_08_axum_response_and_standard_diagnostic_are_redacted` | The real Axum endpoint with a failing registry or sink returns normal data and does not leak identity, raw document, or provider/sink source through HTTP status, body, headers, or standard diagnostics. |

Existing OP-01 through OP-09, OBS-01 through OBS-09, the 53 production IDs,
54 usability IDs, and 69 DX/rate IDs remain historical or current contracts as
recorded. Implementing these eight audit IDs does not change generated ABI 5,
the schema digest, HTTP results, or APQ/Safelist/IdOnly semantics.

## 15. Sibling generator conformance

Status: Implemented; local macOS validation passed. MSRV/stable validation and
independent reviews A/B completed. The privately retained validation
record identifies unexecuted environments and
remaining external proof such as Linux CI.

[ADR-0026](adr/0026-generated-contract-crate-and-manifest-cli.md),
[ADR-0027](adr/0027-lightweight-validation-and-generator-build-identity.md),
and section 15 of `implementation-handoff.md` define the contract. ADR-0028
replaces generated-file line limits with verified ownership and semantic-unit
layout; section 16 adds Gate SG-F. Each ID points to a fully qualified test,
fixture, or script assertion. Broad workspace success is not a substitute for
an individual failure-condition owner.

### SG-A: Manifest/CLI

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-CLI-01 | Root/subcommand help and version work offline without manifest, Git, or network access; human and closed JSON output include bounded build-time information. | `crates/cli/tests/rust_terminal.rs#sg_cli_01_help_succeeds_at_all_levels_offline_without_a_manifest`, [All assertions](sibling-generator-test-trace.md#sg-cli-01) |
| SG-CLI-02 | Init creates one deterministic manifest exclusively, rejects existing files, directories, links, and FIFOs, and does not generate an SDK. | `crates/codegen/src/filesystem/tests.rs#sg_cli_02_exclusive_creation_never_overwrites_files_directories_links_or_fifos`, [All assertions](sibling-generator-test-trace.md#sg-cli-02) |
| SG-CLI-03 | Config-v2 is closed at every level and rejects duplicates, unknown fields, legacy schema/enum settings, misspellings, and conflicting features. | `crates/codegen/src/manifest/tests.rs#sg_cli_03_validates_consumer_features_against_additive_rules`, [All assertions](sibling-generator-test-trace.md#sg-cli-03) |
| SG-CLI-04 | All commands share one default manifest; legacy flags/commands and CLI or environment overrides are rejected, and report/output selection remains explicit. | `crates/cli/tests/rust_contract.rs#sg_cli_04_legacy_overrides_and_environment_values_defer_to_manifest_authority`, [All assertions](sibling-generator-test-trace.md#sg-cli-04) |
| SG-CLI-05 | Runtime identity rejects missing or mixed Git/path revisions, URLs, and sources; relative paths are rebased correctly and no fallback is allowed. | `crates/codegen/src/doctor/metadata/runtime/tests.rs#sg_cli_05_rejects_mixed_cargo_git_sources_and_revisions`, [All assertions](sibling-generator-test-trace.md#sg-cli-05) |
| SG-CLI-06 | Commands and exit codes are stable; JSON remains bounded; read-only checks preserve inputs and diagnostics use stderr; operational and diagnostic output do not drift. | `crates/codegen/src/cargo_diagnostics/tests.rs#diagnostic_policy_can_deny_warnings`, [All assertions](sibling-generator-test-trace.md#sg-cli-06) |
| SG-CLI-07 | Common redaction covers messages, labels, help, notes, malformed Cargo output, paths, URIs, and credentials. | `crates/codegen/src/cargo_diagnostics/tests.rs#keeps_distinct_lib_and_test_diagnostics_and_deduplicates_only_exact_matches`, [All assertions](sibling-generator-test-trace.md#sg-cli-07) |

### SG-B: Generated crate/consumer

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-CRATE-01 | The standalone generated crate is application-independent and passes compile, Clippy, rustdoc, formatting, and semantic-layout checks with external Cargo state. | `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`, [All assertions](sibling-generator-test-trace.md#sg-crate-01) |
| SG-CRATE-02 | The three-crate enum contract preserves successful input/output conversion and masks conversion failures. | `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`, [All assertions](sibling-generator-test-trace.md#sg-crate-02) |
| SG-CRATE-03 | Missing fields, groups, related types, enum conversions, or required trait methods fail consumer compilation while the generated crate remains valid. | `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`, [All assertions](sibling-generator-test-trace.md#sg-crate-03) |
| SG-CRATE-04 | Dependency renames, keywords, namespaces, GeneratorInfo, generator/std names, and cross-crate macros remain hygienic. | `crates/cli/tests/rust_trait_errors.rs#ri_01_02_traits_compile_with_external_domain_values_and_errors`, [All assertions](sibling-generator-test-trace.md#sg-crate-04) |
| SG-CRATE-05 | Only current ABI 8 executes; verified ABI 7 ownership permits protected update. ABI/type-binding disagreement and runtime mixing are rejected without exposing private adapters. | `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_nested_legacy_and_record_mismatch_without_input_values`, [All assertions](sibling-generator-test-trace.md#sg-crate-05) |
| SG-CRATE-06 | The same SDL preserves SchemaDigest, scalar and binding semantics, and manifest/build provenance. | `crates/codegen/src/manifest/io_tests.rs#sg_crate_06_preserves_the_original_schema_digest_during_handle_validation`, [All assertions](sibling-generator-test-trace.md#sg-crate-06) |

### SG-C: Lifecycle/process/filesystem

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-FS-01 | Generate requires absent output; update requires a valid marker; empty directories, unknown files, missing markers, and legacy modules are rejected. | `crates/codegen/src/contract/write/tests.rs#sg_fs_01_generate_is_absent_so_update_is_required`, [All assertions](sibling-generator-test-trace.md#sg-fs-01) |
| SG-FS-02 | Captured input rejects schema mismatch, symlinks, FIFOs, path swaps, growth, and limit violations before parent creation. | `crates/codegen/src/contract/write/input_tests.rs#sg_fs_02_rejects_output_nested_inside_another_schema_before_parent_creation`, [All assertions](sibling-generator-test-trace.md#sg-fs-02) |
| SG-FS-03 | Every precommit, generation, verification, install, and rename failure preserves or restores the old bytes and reports rollback failures. | `crates/codegen/src/contract/write/input_tests.rs#input_failure_keeps_the_old_tree_and_clears_only_candidate_properties`, [All assertions](sibling-generator-test-trace.md#sg-fs-03) |
| SG-FS-04 | Cleanup failure retains recoverable old bytes; success/failure reporting matches the live tree, and corrupt restore artifacts are rejected. | `crates/codegen/src/contract/write/cleanup_tests.rs#failed_cleanup_prevents_a_subsequent_run`, [All assertions](sibling-generator-test-trace.md#sg-fs-04) |
| SG-FS-05 | TERM/INT, concurrent output, residual recovery, and unrelated-file changes preserve a consistent tree without claiming crash-atomic behavior. | `crates/codegen/src/cancellation/tests.rs#sg_fs_05_converts_term_and_int_into_isolated_process_cancellation`, [All assertions](sibling-generator-test-trace.md#sg-fs-05) |
| SG-FS-06 | Check is read-only and reports every added, missing, or changed byte without modifying manifest, SDK, or source. | `crates/codegen/src/contract/check/difference_tests.rs#sg_fs_06_reports_changed_missing_and_added_files_by_name`, [All assertions](sibling-generator-test-trace.md#sg-fs-06) |
| SG-PROC-01 | Formatter and Cargo share bounded deadlines and output/diagnostic limits; timeout and cancellation reap complete process groups. | `crates/codegen/src/cancellation/tests.rs#sg_proc_01_panicking_scope_is_removed_and_other_threads_can_recover`, [All assertions](sibling-generator-test-trace.md#sg-proc-01) |
| SG-PROC-02 | Real-consumer Cargo reports missing resolvers and dependencies, preserves locked state, and rejects ambiguous SDK aliases. | `crates/codegen/src/doctor/metadata/tests.rs#sg_proc_02_rejects_duplicate_aliases_when_the_graph_contains_an_sdk_name`, [All assertions](sibling-generator-test-trace.md#sg-proc-02) |
| SG-PROC-03 | Private compilation preserves exact path/byte meaning, reuses safe caches, rejects SDK/cache overlap, and preflights tool failures. | `crates/codegen/src/contract/write/input_tests.rs#cache_inside_sdk_rejects_cargo_check_and_preserves_the_old_tree`, [All assertions](sibling-generator-test-trace.md#sg-proc-03) |

### SG-D: Generator information

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-INFO-01 | Generator information has closed clean/dirty/unknown states, bounded validated fields, and consistent CLI, JSON, and Rust forms. | `crates/codegen/src/generator_info/tests.rs#sg_info_01_round_trips_every_field_between_json_and_the_rust_model`, [All assertions](sibling-generator-test-trace.md#sg-info-01) |
| SG-INFO-04 | Different CLI, host, and profile values are retained and compared exactly, including post-commit equivalents; GeneratorInfo differences are notice-only. | `crates/codegen/src/contract/check/tests.rs#sg_info_04_preserves_and_exactly_compares_different_host_profile_and_unknown_versions`, [All assertions](sibling-generator-test-trace.md#sg-info-04) |
| SG-INFO-05 | Unknown, missing, or inconsistent marker/GeneratorInfo fields, Rust/JSON disagreement, and renderer/input/runtime drift are rejected. | `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_renderer_byte_differences_even_when_inventory_hash_matches`, [All assertions](sibling-generator-test-trace.md#sg-info-05) |

### SG-E: Repository/product

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-VERIFY-01 | Examples, large consumer, testkit, golden, and compile-fail fixtures reproduce normally, with no legacy module, CLI, or config path. | `scripts/check-generated-crates.py#def exact(`, [All assertions](sibling-generator-test-trace.md#sg-verify-01) |
| SG-VERIFY-02 | The repo-owned full-schema corpus preserves complete source inventories, large-input coverage and representative independent domain-enum bindings. | `scripts/full_schema_corpus.py#def verify(`, [All assertions](sibling-generator-test-trace.md#sg-verify-02) |
| SG-VERIFY-03 | Existing positive and negative GraphQL, endpoint, HTTP, rate, and audit behavior remains intact; core ordinary dependencies remain zero. | `crates/core/tests/executable_contract.rs#public_api_can_select_custom_scalar_documents_for_validation`, [All assertions](sibling-generator-test-trace.md#sg-verify-03) |
| SG-VERIFY-04 | Common validation covers MSRV/stable, every generated crate, handwritten line limits, diff checks, reviews A/B, and the complete ID trace. | `scripts/check-generated-crates.py#def quality(`, [All assertions](sibling-generator-test-trace.md#sg-verify-04) |
| SG-VERIFY-05 | Publisher removal and shared-test transfer are inventoried; required jobs reject failure, skip, cancellation, missing output, and secrets/fork exceptions. | `scripts/test-cargo-consumer-gates.py#test_cancellation_stops_before_the_next_command`, [All assertions](sibling-generator-test-trace.md#sg-verify-05) |
| SG-VERIFY-06 | Formal performance and long fuzzing remain separate gates; publish, package, commit, and SSH work do not block product completion. | `scripts/performance_evidence_regression.py#test_dirty_short_harness_evidence_cannot_satisfy_formal_load_or_soak`, [All assertions](sibling-generator-test-trace.md#sg-verify-06) |

A gate is Passed only when every corresponding row has evidence. Unexecuted
environments are not successes, and an earlier release or fresh-clone
certificate cannot prove that a new program ran.

## 16. Semantic generated layout conformance

Status: Implemented; local macOS validation passed.
[ADR-0028](adr/0028-semantic-generated-layout-and-handwritten-line-budget.md)
adds the following
eight IDs as Gate SG-F while retaining the existing 34 SG IDs. The exact trace
links each row to its test, fixture, or script assertion; aggregate and review
results are recorded in the validation record.

| ID | Required behavior | Trace |
| --- | --- | --- |
| SG-LAYOUT-01 | A genuine generated model over 400 lines is formatted, compiled, checked, linted, documented, and accepted without a handwritten-line warning. | `crates/codegen/src/contract/mod.rs#sg_layout_01_rendered_path_and_rust_over_400_lines_are_allowed`, [All assertions](sibling-generator-test-trace.md#sg-layout-01) |
| SG-LAYOUT-02 | A 401-line handwritten renderer, consumer, or test is rejected; headers, arbitrary markers, same-name directories, and unknown files cannot claim exemption. | `scripts/test-generated-ownership.py#test_arbitrary_marker_outside_registry_does_not_exempt`, [All assertions](sibling-generator-test-trace.md#sg-layout-02) |
| SG-LAYOUT-03 | Types, inputs, enums, operation roots, projections, and internal responsibilities are split into named semantic units. | `crates/codegen/src/render/tests_layout.rs#sg_layout_03_derives_roles_from_actual_operation_roots`, [All assertions](sibling-generator-test-trace.md#sg-layout-03) |
| SG-LAYOUT-04 | Identical inputs reproduce identical bytes; unrelated schema growth preserves existing model and public-trait paths/bytes except required aggregate metadata. | `crates/codegen/src/render/tests_layout.rs#sg_layout_04_unrelated_growth_does_not_change_existing_model_and_trait_paths_or_bytes`, [All assertions](sibling-generator-test-trace.md#sg-layout-04) |
| SG-LAYOUT-05 | Keyword, normalization, case, and file/directory collisions are rejected while private layout, macros, re-exports, and cross-crate hygiene remain valid. | `crates/codegen/src/writer/layout.rs#semantic_paths_reject_traversal_case_and_file_directory_collisions`, [All assertions](sibling-generator-test-trace.md#sg-layout-05) |
| SG-LAYOUT-06 | Large line counts succeed when byte, file, depth, process, and resource bounds hold, without unbounded concatenation or duplicate allocation; violations fail closed before excessive allocation and preserve existing output. | `crates/codegen/src/writer/layout.rs#sg_layout_06_rejects_total_byte_overflow_before_adding_a_file`, [All assertions](sibling-generator-test-trace.md#sg-layout-06) |
| SG-LAYOUT-07 | Public types, traits, projections, ABI 6, SchemaDigest, binding fingerprints, and GraphQL behavior are unchanged by layout-only changes. | `crates/codegen/tests/sibling_contract.rs#sg_layout_07_semantic_formatting_preserves_public_contract_surfaces`, [All assertions](sibling-generator-test-trace.md#sg-layout-07) |
| SG-LAYOUT-08 | Ordinary update migrates every golden, example, testkit, large, and full-schema corpus output; exact checks reject added, missing, or changed files. | `crates/codegen/src/contract/check/tests.rs#sg_fs_06_reports_extra_missing_and_changed_files_without_modifying_bytes`, [All assertions](sibling-generator-test-trace.md#sg-layout-08) |

SG-INFO-02 and SG-INFO-03 are retired build-matrix acceptance IDs under
ADR-0059. SDK clean/dirty/unknown observation behavior remains unchanged;
ordinary product acceptance no longer repeatedly rebuilds Git/cache variants.
Public GeneratorInfo schema, generated-record consistency, actual build identity
and copied-binary behavior remain covered by the retained product checks.

Historical generated-line assertions such as PROJ-11 and the large-consumer
check now assert semantic-unit layout and resource limits. Other resource,
completeness, and ADR-0059's implementation-only 400 Code checks remain in force. Gate SG-F does
not pass unless the exact trace covers all 39 active SG IDs and every required
execution has completed.

## 17. Experimental Luau blog mock conformance

Status: Implemented. Gates M-A through M-D and MOCK-01 through MOCK-16 Passed
locally on macOS. [ADR-0029 section
8](adr/0029-experimental-luau-mock-cli.md#8-implementation-order-and-achievement-gate)
and section 17 of `implementation-handoff.md` define the implementation order
and acceptance gates. This validates the fixed blog application; it does not
replace GraphQL conformance or establish general production readiness.

| Gate | Target | Current evidence |
| --- | --- | --- |
| M-A | MOCK-01–04: VM, types, host calls, static initialization, preboot checks, and real CLI/HTTP iteration | Passed on fixed Rust 1.95 and stable across all six wrappers and every CRUD path using the real binary on port 0. |
| M-B | MOCK-05–11 and MOCK-14: CRUD, bidirectional relationships, GraphQL, rollback, authorization, script editing, and diagnostics | Passed with independent real-HTTP assertions, store/order/counter checks, restoration, nested failures, and commit preservation. |
| M-C | MOCK-12, MOCK-13, and MOCK-15: stop/return, cancellation/recovery, snapshot, restart, CORS, and admission | Passed with server return, early termination, mutex contention, signal/join, queue boundaries 16/17, capability rejection, and debug/release reentry rollback. |
| M-D | MOCK-16 and the complete ID trace: ordinary generation, existing regressions, conventions, real tests, and handoff | Passed after exact/quality checks for all SDKs, the initial 64-command common validation, reviews A/B with zero open findings, and the ADR-0029 response-time regression in the final 65-command trace. |

The validation record maps every ID to
its test, independent assertion command, and result. Initial evidence comprised
six fixed-Rust-1.95 unit tests, 27 CLI/HTTP tests, five parent-VM tests, and the
stable full workspace. Existing Rust 1.85/full-product checks, dependency
isolation for Luau, and fail-closed mock aggregation also passed. Final source
evidence and unexecuted Linux/remote CI remain explicit; partial WP-M0
measurements are not promoted to aggregate acceptance.

A design-review correction changed the `pcall` reentry result and added three
MOCK-12 cases in `tests/acceptance/reentry.rs`. They verify success at phase
16, rejection at phase 17, a caught re-entry overflow, a caught `MemoryError`, and restoration
of store, order, and counters. Both debug and release real-HTTP paths passed.
The current debug totals are six unit, 30 CLI/HTTP, and five parent-VM tests.

Acceptance relies on independent assertions of the new model/VM boundary and
the real CLI/VM, not only pre-existing tests. Separate evidence records external
watchdog termination and a normal request on the same server after VM shutdown.
Fixed-fixture success is not generalized to arbitrary schemas, AI quality, or
hostile-script sandbox safety.

## 18. Runtime schema Mock IR conformance

Status: Passed locally on macOS. Gates G-A through G-E and RSM-01 through
RSM-22 completed the new contract. [ADR-0030 section
10](adr/0030-runtime-schema-mock-ir-and-directives.md#10-achievement-gate) and
section 18 of the handoff define the gates. This is an additional Mock
application contract, not a GraphQL CRUD specification.

| Gate | Required IDs | Evidence | Status |
| --- | --- | --- | --- |
| G-A | RSM-01–04 | SDL, directives, IR, relationship declarations, custom scalars, and initialization rejection | Passed |
| G-B | RSM-05–08 | Two domains, renaming, manual/abstract/payload types, same-type dual binaries, expected types, and authorization | Passed |
| G-C | RSM-09–14 | CRUD, seeds, every relation, transactions, snapshots, nulls, numbers, and containers | Passed |
| G-D | RSM-15–17 | VM/host behavior, debug/release reentry, cancellation, stop/return, CORS, and admission | Passed |
| G-E | RSM-18–22 | Portable TOML, existing product/SDK/MSRV checks, common validation, independent review, and handoff | Passed |

The validation record maps all 22 IDs to
fully qualified tests and records each assertion, command, and result. Aggregate
workspace success does not replace individual evidence. MOCK-01–16 security
results are retained but do not automatically establish the new gates. Coverage
continues to include `pcall` re-entry rejection, caught memory overflow, whole-model rollback, same-server
recovery, and worker joining.

Runtime schema and generated ABI 6 are tested as separate sources, including
consumer trait completeness, schema identity, and data quality. Existing
GraphQL, HTTP, scalar-output, observability, and limit suites remain active.
Real startup without the original SDL or Cargo, plus schema-renaming behavior,
is required. Linux/remote CI, external-watchdog rescue, open-ended AI,
subscriptions, and a production sandbox are excluded from Passed totals.

Focused evidence covers 25 owned-input/resource-return, vocabulary, type-graph,
semantic-IR, value-validation, store, seed, return-value, and Luau-conversion
cases, plus type-candidate, generic host, VM, and cancellation tests. The shared
worker/owned dispatch was connected to init/serve v1, and 30 existing CLI/HTTP
cases ran on the new path. Static root-type contracts support nested output.
Two-domain, rename, offline-HTTP, CLI-negative, and root/nested binary cases also
passed. Totals were 118 unit, 41 HTTP, seven parent-VM, three release-reentry,
and two generic parent-VM tests. Common validation completed 66 commands across
Rust 1.85, Mock Rust 1.95, stable, and all SDKs. Reviews A/B left zero open
findings. The dedicated owned-input byte-budget failure argument matches
ADR-0030 section 7.1.

## 19. Mock model API / generated reference conformance

ADR-0031 section 10 defines MR-01 through MR-27. MR-A covers 01, 02, and 16;
MR-B covers 03–09; MR-C covers 10–15; MR-D covers 17–23 and the three DX paths;
MR-E covers 24–27 and the complete trace. Existing RSM and MOCK results remain
active in v2. Acceptance requires executed assertions, not merely the presence
of tests or snapshots. The
validation record covers Rust 1.85, Mock
Rust 1.95, stable, ABI 6/core, zero ordinary core dependencies, exact/quality
checks for all SDKs, VMs, stop/recovery/join behavior, and read-only reviews A/B.
Unexecuted remote CI remains distinct from local success.

| Gate | Target | Local result |
| --- | --- | --- |
| MR-A | MR-01, MR-02, and MR-16: shared definitions, types, and examples | Passed |
| MR-B | MR-03–09: search, count, and snapshots | Passed |
| MR-C | MR-10–15: relationship editing, rollback, budgets, and cancellation | Passed |
| MR-D | MR-17–23: type-graph reference, CLI/pager behavior, and secrecy | Passed |
| MR-E | MR-24–27: v2, real HTTP, common validation, review, measurement, and trace | Passed |

The validation record maps all 27 IDs to individual assertions and the final
71-command certificate. Reviews A/B left zero open findings. Linux/macOS remote
CI and its summary job were unconfirmed and are not included in this result.

## 20. Mock project / portable export conformance

ADR-0032 section 11.2 defines MP-01 through MP-32. MP-A covers 01–08, MP-B
09–16, MP-C 17–23, MP-D 24–30, and MP-E 31–32 plus the complete trace. The
initial Required/Not Run inventory advanced to local Passed for all 32 IDs and
Gates MP-A through MP-E. Individual assertions cover CLI, HTTP, transactions,
export, debug/release parent VMs, and macOS external-network disconnection.

Completion includes Rust 1.85, Mock Rust 1.95, stable, the ABI 6 dependency
boundary, debug/release parent-VM tests, and independent read-only reviews A/B.
ADR-0030/0031 security and semantic tests remain active rather than being
replaced by the new format. Common validation completed 73 commands, and reviews
A/B left zero open findings. The
ADR-0032 validation record identifies
each gate and the final source. The designer review Passed on 2026-09-09 with
zero new findings at every severity. Designer-run and independent command
evidence remain separate. Linux/remote CI is a separate unexecuted gate.

## 21. Agent-assisted Mock inference conformance

[ADR-0033 section
9.2](adr/0033-agent-assisted-mock-inference.md#92-closed-acceptance-inventory)
defines AI-01 through AI-28. AI-A covers 01–05, AI-B 06–09, AI-C 10–14, AI-D
15–23, and AI-E 24–28. Final joint validation completed 75 commands, two-domain
and rename HTTP/PTY cases under the macOS network block, and independent A/B
review and re-review.

With explicit permission, AI-25 also passed across two domains, four total
calls, ten candidates accepted without correction, 28 fixed HTTP cases, and offline operation
after adoption. All 28 IDs and Gates AI-A through AI-E Passed locally. The
designer review also Passed with zero new findings at every severity; remote CI
remains unconfirmed. The privately retained ADR-0033 validation
record keeps
its assertions, commands, and results independent from existing MP Passed
evidence.

Common CI uses a fake backend/executable. AI-25's explicitly authorized real
Codex input is recorded separately. Two-domain and rename cases retain
independent HTTP assertions, real PTY coverage, full rollback/recovery, and
unchanged aggregate readiness. Local product validation, AI-25, remote CI, and
designer approval are reported separately; unexecuted work is never marked
Passed.

## 22. Generated Luau formatting conformance

[ADR-0034 section
9.2](adr/0034-generated-luau-formatting.md#92-closed-acceptance-inventory)
defines FMT-01 through FMT-24. The
formatting and CLI-DX validation record
maps every ID to its assertion, source, command, and result. Existing MP and AI
results are not reused as FMT evidence.

| Gate | Target | Result |
| --- | --- | --- |
| FMT-A | FMT-01–05: embedded/fixed style, semantic preservation, host/types, and static generation | Local Passed |
| FMT-B | FMT-06–09: B/U/N paths, edit protection, repeated updates, copy, and export | Local Passed |
| FMT-C | FMT-10–15: AI candidates, edits, proposal v2, read-only behavior, conformance, approval, and placement | Local Passed |
| FMT-D | FMT-16–20: resource limits, malformed input, debug/release, cancellation, recovery, and secrecy | Local Passed |
| FMT-E | FMT-21–24: two-domain semantics, common validation/license, measurement, independent review, and handoff | Local Passed |

Generated digest bytes, candidate display bytes, approved bytes, and placed
bytes match exactly; separate negative tests preserve existing user bytes. Common CI's fake
backend/executable is sufficient for FMT; absence of a real Codex call does not
make FMT incomplete. Unexecuted Linux/macOS product validation remains distinct
from local success. Successful rules and license checks alone do not constitute
full implementation Passed.

## 23. CLI TUI and embedded GraphiQL conformance

[ADR-0035 section
7.2](adr/0035-cli-tui-and-embedded-graphiql.md#72-closed-acceptance-inventory)
defines DX35-01 through DX35-30. The
formatting and CLI-DX validation record
maps every assertion, source, command, and result. Existing DX, SG, MR, MP, AI,
and FMT results are retained independently; partial evidence is not promoted to
complete success.

| Gate | Target | Result |
| --- | --- | --- |
| DX35-A | DX35-01–06: Clap/CLI surface, help/version, grammar, secrecy, and existing processing | Local Passed |
| DX35-B | DX35-07–12: ratatui/PTY, view/approve, input bounds, restore/recovery, reference, and pipes | Local Passed |
| DX35-C | DX35-13–18: assets, HTTP components, builder/introspection, both bridges, security, and one schema | Local Passed |
| DX35-D | DX35-19–24: Mock flag, origin, HTTP information storage, run/stop, and minimum manual UI procedure | Local Passed; procedural handoff complete, manual UI confirmation pending |
| DX35-E | DX35-25–30: common validation, ABI/MSRV, ADR-0034 regression, measurement, removal rules, review, and handoff | Local Passed |

Automated IDs map to individual Rust/HTTP assertions, commands, and results.
DX35-21 uses the minimum browser smoke and repeatable handoff procedure in
[ADR-0035 section
7.3](adr/0035-cli-tui-and-embedded-graphiql.md#73-minimal-browser-confirmation-and-manual-handoff).
A schema snapshot, query/mutation list, a UI result requiring no external requests or resources, or
successful HTML snapshot alone is insufficient. Automated tests and the manual
procedure jointly establish the UI result; actual PTY input, approval, and reset
remain required.

Embedded GraphiQL sends real introspection through the existing endpoint
pipeline rather than introducing a separate executor/authentication loop. UI
transitions preserve ADR-0034 formatting, saved/approved bytes, and copy/user
edit protection. Common validation covers Rust 1.85, Mock Rust 1.95, stable,
every SDK exact/quality check, features, dependencies, and asset licenses. Real
AI input, publishing, and SSH are outside the required gate. The 2026-09-09
designer reviews Passed with zero new ADR-0034/0035 integration findings. The
record covers 84 commands over the complete worktree, including untracked
files. Manual GraphiQL verification and unexecuted remote CI remain separately
identified.

## 24. Mock Luau hooks and JSON logging conformance

[ADR-0036 section
8.2](adr/0036-mock-luau-hooks-and-json-logging.md#82-closed-acceptance-inventory)
defines HK-01 through HK-36. All IDs and Gates HK-A through HK-E Passed locally
without reusing MR, MP, AI, FMT, or DX35 evidence. The
integrated validation record maps setup,
state, six-slot types, module B/U/N behavior, bundle v4,
synchronous VM/worker readiness, and every individual assertion. Added HTTP
plugin, numeric-status, and CORS cases completed the final 85-command validation
and independent A/B reviews.

Debug/release real HTTP covered all six slots across six resources, plus worker
cancellation, duration, and queue limits with every slot occupied. A second
domain/rename case verified TUI adoption through the three communication roles
and 115 HTTP cases under the macOS network block. DTO and abstract-field
snapshots including another request's commit inserted at the snapshot boundary,
Mutation contention and rollback, controlled parallel completion order,
reversed after-hook order, and access status were checked independently in
debug and release.

| Gate | Target | Result |
| --- | --- | --- |
| HK-A | HK-01–06: config/types, authorization, state patch, ownership, update/export, and new format | Local Passed |
| HK-B | HK-07–12 and HK-31–32: plugin order, request/all-field/numeric HTTP denial, snapshot, after, and commit | Local Passed |
| HK-C | HK-13–18: VM cancellation/recovery, hook/access JSONL, loss, secrets, and CORS | Local Passed |
| HK-D | HK-19–24 and HK-33–36: reference/AI target, candidate, proposal, approval, two-domain real HTTP, and message display | Local Passed |
| HK-E | HK-25–30: common validation, existing regressions, measurement, conventions, independent review, and handoff | Local Passed |

Authorization tests cover both static Luau checks and runtime denial through
`any`/`pcall`. Field hooks cover real SDL fields, including save, native, and
relation fields, while excluding `__typename` and introspection. State patches
are all-or-nothing: if a patch is invalid, no field is applied. Request
isolation, snapshots, and preservation of existing root commits have separate
assertions.

Real HTTP verifies that responses and readiness continue even when logs are
lost, and that after hooks do not change successful responses or alter
shutdown. Tests cover numeric-rejection 401/403/200, internal-code 500,
body-forbidden statuses, parallel rejection status, order independence, and
transport-failure precedence. Mock's intentional status overrides are separate
from GraphQL over HTTP conformance. Slow, closed, and truncated stdout,
deadlines/cancellation, and worker/writer recovery each have negative coverage.

The AI display path reuses the real PTY and fake backend; it requires neither
live reasoning nor a new browser harness. Prompt, schema, requested changes, and
public responses are checked across TTY, non-TTY, omission, and harmlessness
cases. Raw stderr, authentication data, and internal reasoning are not shown.
Communication loss neither weakens protocol checks nor replaces the complete
diff/approval view; new communication history is not persisted. Config,
registration, and state identity drift are checked while preserving candidate,
approval, placement, bundle/update/native, and `.dist/` recovery bytes.

Every Passed claim maps to a real assertion, command, and result. Common
validation covers Rust 1.85, Mock Rust 1.95, stable, ABI 6, and SDK quality.
Unexecuted Linux/macOS remote CI, designer approval, and design additions remain
separate from implementation acceptance.

## 25. Mock TUI usability and Unicode conformance

[ADR-0037 section
8.2](adr/0037-mock-tui-usability-and-unicode.md#82-closed-acceptance-inventory)
defines UX37-01 through UX37-24 independently from AI, FMT, DX35, and HK. The
integrated validation record maps common
layout, the Unicode editor, and real PTY assertions to final-source validation.
UX37-24 added a dedicated fake launcher, short manual procedure, and real-screen
documentation. Debug/release real PTY tests cover startup, cancellation, and
adoption. Final 85-command validation and independent A/B review produced local
Passed for all 24 IDs and Gates UX37-A through UX37-E. The 2026-09-10 designer
review also Passed with zero findings at every severity. Human visual/IME
verification and remote CI remain unconfirmed.

| Gate | Target | Result |
| --- | --- | --- |
| UX37-A | UX37-01–06: borders, checkboxes, editor, resize, keys, and reference | Local Passed |
| UX37-B | UX37-07–10: Japanese text, Unicode, safe display, and real PTY | Local Passed |
| UX37-C | UX37-11–16: unified diff, color, line endings, coverage, and resources | Local Passed |
| UX37-D | UX37-17–20: progress, approval, cancellation, worker/job recovery, and non-TTY | Local Passed |
| UX37-E | UX37-21–24: common quality, measurement, independent review, and manual handoff | Local Passed |

The expression buffer validates full lines rather than isolated characters,
including buffer characters, borders, regions, style, cursor position, UTF-8
paste, cancellation, and terminal restoration. Unicode edits preserve grapheme boundaries, display lines, and
exact submitted bytes. Tests distinguish dangerous controls from literal
escapes, assert that no Unicode normalization occurs, and cover Japanese and
other non-ASCII single-line display.

Diff tests cover additions, deletions, equality, old/new line numbers, tabs,
trailing blanks, and newline differences. PTY tests cover end/middle jumps, resize,
long-line clipping, and preservation of complete text. Search and coverage
limits fail safely; incomplete or over-budget information cannot open approval.
The same formatted candidate bytes are allocated, edited concurrently, rolled back,
restored, transmitted, verified, and finally approved in separate assertions.

Progress and ADR-0036 communication-history tests cover only implemented
integration. Existing bounded I/O, worker recovery, non-TTY text, and JSON
behavior remain active. Real AI and new browser infrastructure are not required.
Every result maps to a test, assertion, command, and common-validation record on
Rust 1.85, Mock Rust 1.95, stable, ABI 6, and all SDKs. Small fake fixtures retain
manual procedures and screen evidence for human readability and designer
approval. Unexecuted manual/remote-CI work is not marked Implemented or Passed.

## 26. Simplified Mock directives / model API conformance

ADR-0038 replaces the legacy Mock vocabulary, CRUD mapping, and seven-format
contract while preserving the ordinary GraphQL executor and ABI 6. The
simplified-directives validation record
maps SD-01 through SD-18 to individual assertions and actual commands, including
the 2026-09-12 final acceptance audit.

The tests cover models from unmarked objects, arbitrary IDs, embedded values,
explicit relation matching and connection order, manual/virtual models, seven
operations sharing one `QueryPlan`, 15 constraint arguments, and eight formats. Regex limits
are shared between startup and execution across real IR, host, and HTTP paths.
`init` emits empty JSONL seed files. Two-domain, rename, ID-less, portable, and
fake-inference cases in one binary also require PTY approval; unimplemented
operations are rejected.

Final local Passed evidence covers Rust 1.85, Mock Rust 1.95, stable,
debug/release VM and HTTP, every SDK exact/quality check, measurements,
corrected independent review, and exact final-source agreement.
ADR-0039 in section 27 completes the integrated current-source acceptance. The
observation-budget proposal
was not adopted and moved to the register. Remote CI and human UI/IME/designer
approval remain separate from local automated results.

## 27. Request-atomic Mock execution conformance

[ADR-0039](adr/0039-request-atomic-mock-execution-and-soft-hard-limits.md)
defines RA-01 through RA-18 and Gates RA-0 and RA-A through RA-D. All Passed locally
against the current source in a 102-command run with release-binary
measurements and independent A/B review. The earlier 101-command success remains
historical evidence for the prior candidate.

Assertions cover commit behavior, complete candidate elimination, read-only
after-hook candidates, soft-warning continuation, hard failures, rejected 200
responses versus internal failures, encoding and cancellation races, parallel
requests, worker collection, stderr diagnostics/secrecy, and seven-format edit
protection. Existing GraphQL executor behavior remains active where Mock does
not intentionally override it. The
validation record identifies every gate,
source, independent review, and unexecuted environment.

RA-12's lower-level post-commit evidence distinguishes transport return,
dropped delivery, and forbidden recommit from real injected TCP interruption.
The
contract does not guarantee complete diagnostic delivery, response receipt
after commit, or graceful recovery from an unrecoverable panic.

## 28. Mock scalar representation directive conformance

[ADR-0040](adr/0040-mock-scalar-representation-directive.md) defines REP-01
through REP-12 and Gates REP-A through REP-C. Parser, IR, and CLI tests cover
all seven representations, constraints and vocabulary synthesis, rejection of
legacy names and arguments, ordinary generation/version transitions, and
non-destructive behavior. Real HTTP values, rollback, same-server recovery, ABI
6, MSRV, and every SDK quality check remain active.

The 2026-09-15 validation executed every REP assertion and command against the
current source; it did not infer new syntax success from earlier ADR results.
Release-binary measurements and independent read-only reviews are recorded in
the validation record. Reference
topics, JSON shape, and TUI behavior were not part of the ADR-0040 gate and are
accepted separately in section 29. The final designer review Passed on
2026-09-15. Remote CI, human UI/IME verification, and real AI connections were
not executed.

## 29. Mock reference topics and navigation conformance

[ADR-0041](adr/0041-mock-reference-topics-and-navigation.md) defines the closed
REF-01 through REF-15 inventory. This is a Mock reference display/structure
contract generated from semantic schema, API, and IR; it is not a new GraphQL
execution requirement.

| Gate | IDs | Required behavior | Result |
| --- | --- | --- | --- |
| REF-A | REF-01–06 | Closed CLI, entries at most 30 lines, fixed title/man sections, standard/Mock/user directives, semantic arguments, and independent negatives | Local Passed |
| REF-B | REF-07–11 | API/IR, additional topics, domain/model coverage, complete v6, closed partial JSON, and the inference complete version | Local Passed |
| REF-C | REF-12–14 | man/less titles, ending and keys, list/detail TUI, real PTY, non-TTY/readiness/secrecy/nonexecution | Local Passed |
| REF-D | REF-15 | Complete quality, MSRV/stable, ABI 6/SDKs, 400-line rule, measurement, documentation, and trace | Local Passed |

Complete `directives` JSON contains the five standard, four Mock, and all
user-defined semantic directives with closed class/execution enums. Tests
compare syntax, argument type, defaults and requiredness, locations, seven
representations, eight formats, and implementation authority. Every Mock
directive has positive IR coverage and expected negative classifications.

All six topics and NAME selections have positive and negative cases. Tests
cover default text output, fixed title/section order, complete `all` output,
a no-schema case, and a separate two-domain model. Full JSON v6 and topic format v1 reject
unknown fields, versions, variants, inconsistent records, and resource
overruns. Directive documents resolve every `rule_id` to main text and a
failure class without dangling graph references. No-argument complete JSON,
`all` complete JSON, the inference-context complete reference, and proposal
identity v6 are compared independently.

The test backend covers topic lists, items, details, `all`, full version,
Enter/Backspace selection, detail scrolling, search/next/previous,
`CHIGIRI-MOCK-REFERENCE(1)`, `(END)`/`--More--`, status/footer, and resize.
Real PTY cases cover topic navigation, quit/signals, and terminal restoration.
Non-TTY, JSON, no-pager, BrokenPipe, and I/O failures have separate fixtures.
Incomplete resolvers, hooks, and seeds remain unexecuted; secret paths stay
hidden and `readiness_checked=false` is preserved.

Final acceptance is based on the validation record's source, tests, assertions,
commands, and results, not on remote CI or designer review alone. The
ADR-0041 validation record records local
results. The 2026-09-15 designer review requested three Medium and one Low
change. A 2026-09-16 review confirmed three fixes. The remaining U+061C
bidirectional-display correction covered all 12 bidi-control characters and awaited
final designer confirmation at this historical checkpoint.

## 30. Mock type-check batching and initialization performance

[ADR-0042](adr/0042-mock-typecheck-batch-and-init-performance.md) defines the
contract. It retains GraphQL execution, project-format, third-party-source, and
closed-test-set boundaries. Evidence covers module-local/export type isolation,
same-name symbols, single compilation, expected wrappers, logical coordinates
and line positions, existing resource limits, and separation between startup
and runtime VMs. Executed and unexecuted scope is recorded in the
performance validation record.

## 31. Agent-ready Mock authoring conformance

The current contract is defined by
[ADR-0044](adr/0044-composable-agent-ready-mock-authoring.md).

| Area | Required behavior |
| --- | --- |
| Generated guidance | Every new project contains deterministic English `AGENTS.md` guidance with editable and forbidden paths. |
| Scoped discovery | Exact GraphQL type, model, and resolver selections produce bounded text and closed topic JSON. |
| Machine diagnostics | The ADR-0044 baseline emitted `chigiri-mock-check/v1` without physical paths or source values; Section 32 supersedes its wire version. |
| Incremental validation | Exact active resolver, seed, and enabled-hook paths validate independently while unrelated incomplete files keep `readiness_checked=false`. |
| Readiness | Only a successful full check reports `readiness_checked=true` and permits serving or export. |
| Composition | Reference and check output use stdout and broken-pipe success; Chigiri owns no agent or terminal interaction session. |
| Removed surface | `infer`, `apply`, the Codex adapter, proposal/adoption protocol, review UI, and vendored terminal UI are absent. |

Focused unit and real-CLI tests cover deterministic guidance, exact selections,
unknown and duplicate targets, forbidden generated targets, partial success,
full failure, full success, JSON diagnostics, and direct reference output.

## 32. Stateful Mock authoring conformance

The current contract is
[ADR-0045](adr/0045-stateful-mock-authoring-and-runtime-contract-parity.md).
Its SA-01 through SA-18 evidence is implemented in unit, CLI, and HTTP
acceptance tests. The conformance boundaries are:

| Area | Required behavior |
| --- | --- |
| Capability parity | Published globals and library members type-check and execute from one registry; unavailable APIs remain absent; native operations have watchdog-backed limits and VM recovery. |
| Request and clock | Hooks-disabled root/nested resolvers receive a readonly bounded request snapshot; non-HTTP uses `ctx.null`; one fixed/system clock instant is shared within a request. |
| Record provenance | Genuine model records retain relation identity through DTO/list/Connection containers; forged, modified, foreign, and stale tables fail. |
| Private storage | Private fields/models support typed CRUD, constraints, keys, and relations, but remain absent from public validation, dispatch, introspection, and output. |
| Seeds | `$ref` supports forward and composite declared keys with bounded deterministic expansion; invalid, missing, ambiguous, cyclic, and ill-typed references fail; export is fully resolved. |
| Business errors | `ctx.fail` is sticky through `pcall`, exposes only the closed code/message payload plus host correlation, and rolls back the whole request without masking internal failures. |
| Diagnostics | Check v2 provides fixed cause/expected/actual/hint fields and logical positions; runtime diagnostics provide cause, logical module, coordinate, bounded stack, and request correlation without source values or physical paths. |
| Clean break | Only project v5, bundle v7, API v6, reference v7/topic v3, metadata v6, check v2, and log v2 are accepted; no legacy reader or automatic migration is active. |

The integrated stateful-domain acceptance begins with a fresh init, edits the
private schema, updates generated ownership, checks selected seed dependencies,
runs a full readiness check, exports a portable bundle, and exercises two-user
authentication, uploads, votes, idempotent transfers, atomic history/balance
updates, later-root rollback, after-hook rollback, and public-schema secrecy by
HTTP values and resulting state rather than status alone.

## 33. Standard time and timeleap conformance

The current time contract is
[ADR-0046](adr/0046-standard-os-time-and-explicit-timeleap-control.md). It
supersedes the `Request and clock` row and clean-break versions in Section 32;
the remaining ADR-0045 conformance rows stay active.

| Area | Required behavior |
| --- | --- |
| Standard `os` | Only `time`, `date`, `difftime`, and `clock` are exposed. Native explicit conversion, nil/error, UTC/local, and return behavior is preserved within resource limits. |
| Apparent time | Only no-argument `os.time` and `os.date` with omitted/nil timestamp observe timeleap. System policies progress; fixed policies do not. Explicit conversion, `os.clock`, host timing, logs, and IDs are unaffected. |
| Request ownership | A request captures the server policy before `request.before`; all of its hooks and resolvers share the view. Local travel/reset is limited to preparation and sealed before fields. Reset resumes the captured policy. |
| World ownership | `world = true` works only in the serialized host-owned controller and affects later contexts in one server. Candidate policy publishes once after final fencing; world reset restores immutable startup configuration. |
| Options and authority | Omitted/nil/empty options and `world = false` are request-local. Unknown fields, `scope`, `global`, non-boolean values, wrong phases, aliases, and missing-request defaults cannot promote authority. |
| Initialization | Explicit `os` conversions and `os.difftime` are allowed; current-time reads, `os.clock`, and timeleap controls are rejected. Runtime callbacks may be defined during initialization. |
| Timestamp admission | Travel accepts finite exactly represented integer Unix seconds only after native UTC decomposition and round-trip admission. Rejection changes no policy. |
| Discovery | Registry-derived declarations, checking, runtime installation, reference text/JSON, and examples agree. The runtime topic is `timeleap`; `clock` and `ctx.clock` have no alias. |
| Clean break | Only project v6, bundle v8, API v7, capabilities v2, reference v8/topic v4, and metadata v7 are accepted. Check v2, log v2, and generated ABI 6 remain unchanged. |

TL-01 through TL-17 require final-source unit, checker, generation, recovery,
debug/release, real HTTP, controller-interleaving, multi-server, export, and
quality-gate evidence. Exact commands, source correspondence, review findings,
and unexecuted platforms belong in `docs/timeleap-validation.md`; prior SA
evidence alone does not satisfy this follow-up.

## 34. Mock authoring boundary follow-up conformance

[ADR-0047](adr/0047-mock-authoring-type-and-output-boundaries.md) is Accepted /
Implemented. Its AB-01 through AB-16 matrix is satisfied by the final-source
evidence in
Mock authoring boundaries validation.
It does not replace the historical status of the SA/TL programs above.

The required coverage includes annotation-free inline query typing, unique
private record slots, trusted private embedded-value projection through HTTP,
internal nested-parent access without public disclosure, strict provenance,
snapshot versus relation behavior, rollback and recovery, precise sanitized
diagnostics, named hook discovery, executable guidance, and canonical update
and export. Exercise a second domain and assert selected nested values, not
only root success or HTTP status.

Public SDL remains authoritative for stored, relation, and virtual fields;
there is no mock-side override or inferred business behavior. The current
format inventory is log v3 and reference v9/topic v5, with check v2 and the
other ADR-0046 formats unchanged. Exact fixtures reject obsolete reference
documents and retain the clean-break contract.

## 35. Chigiri brand and single-binary CLI conformance

Status: Required / Implemented / CH-01 through CH-11 Passed locally on macOS.
[ADR-0048](adr/0048-chigiri-brand-and-single-binary-cli.md) is normative; the
validation record preserves focused,
relocated, complete-validator, and independent-review evidence.

| ID | Final evidence |
| --- | --- |
| CH-01 | Closed root/Rust/Mock help and version grammar, offline/no-write behavior, strict options, and retired-command rejection. |
| CH-02 | One copied `chigiri` outside the repository executes both workflows with retired binaries absent from `PATH`. |
| CH-03 | Relocated init/codegen/check/update/doctor and real consumer, ownership, drift, dependency, and recovery coverage. |
| CH-04 | Deterministic stdout-only scaffold, rejection/no-write checks, and a compiled adopted projection. |
| CH-05 | Fresh Mock init/update/check/reference/serve/restart/export/recovery plus HTTP value, rollback, and secrecy assertions. |
| CH-06 | Same-executable writer with closed grammar, bounded frames/pipes, cancellation, child reap, and shutdown deadlines. |
| CH-07 | Empty dedicated-governance inventory plus retained ABI/layout/descriptor/GraphQL behavior and Mock snapshot/update tests. |
| CH-08 | Checked 3,270-file brand inventory, 19 required identities, ten rejection owners, and four mutations. |
| CH-09 | Old/mixed pre-mutation rejection and exact fresh-destination regeneration/migration. |
| CH-10 | Rust 1.85 normal/generated, Rust 1.95 CLI/Mock, stable repeats, dependency direction, and std-only core gates. |
| CH-11 | Formatting, Clippy, rustdoc, doctests, retained tests, generated exactness, implementation rules, two reviews, and the 134-step final-source validator. |

Linux, Windows, remote CI, publishing, archives, signing, SSH, formal
performance/load/soak qualification, and long-running fuzzing remain
unexecuted. The non-UTF-8 real-filename case remains Linux-required because
macOS rejects its fixture with `EILSEQ`.

## 36. Mock authoring feedback conformance

Section 38 defines the implemented successor that removes completion tracking.
AF-11 and inventory-specific AF-14/AF-16 assertions are historical, not failed
tests; the retained feedback remains required.

Status: Required / Implemented. AF-08 through AF-11 and AF-14 through AF-16
passed locally on macOS through focused acceptance, retained workflow coverage,
two independent reviews, comparable measurements, and the complete product
validator. The shared-module feature and its public-release blocker were
withdrawn, not passed.
[ADR-0049](adr/0049-mock-authoring-modules-and-feedback.md) is normative. These
are Mock authoring and isolation requirements, not changes to GraphQL semantics.

| Area | Acceptance IDs | Required behavior |
| --- | --- | --- |
| Refinement | AF-08 | Genuine terminal guards narrow in resolvers, hooks, and typed same-file helpers; shadowing, captured aliases, protected calls, invalid input, genuine nil errors, runtime identity, and both span endpoints remain sound. |
| Safe feedback | AF-09 | Correct original sources and concrete guidance only from published kinds or proven host facts; honest fallback without message parsing, invented classifications, neighboring-expression blame, or leaked values. |
| Reference navigation | AF-10 | At most four distinct, deterministic, relevant selections resolve in the existing registry, including before readiness; text/JSON and input modes agree without repeated config evaluation or physical paths. |
| Unfinished inventory | AF-11 | Active unfinished descriptors plus explicit unclassified entries, deterministic bounded enumeration, correct completion/exit/readiness semantics, no seeds or body execution, aggregate controls, and verified input identity. |
| Public output | AF-14 | Exact check-v3/inventory-v1 shapes, nullable identities, reference bounds, usage failures, incomplete/empty output, direct stdout, and real BrokenPipe success. Existing target/full-check semantics remain unchanged. |
| Product admission | AF-15, AF-16 | Pinned unmodified registry dependencies, no added vendor/patch/fork/Git or unsafe workaround, no loader/sample, unchanged retained formats, current-project workflows/ownership/recovery, existing quality gates, final-source reviews/validator, and comparable measurements. |

Evidence must assert values, precise negative behavior, source spans, and
ownership outcomes rather than only successful compilation or HTTP status.
Product regression coverage does not impose comprehensive business scenario
tests on mock users. Historical AB/CH results do not satisfy AF acceptance;
unexecuted platform or release work must remain explicitly unexecuted.

AF-01 through AF-07 and AF-12/AF-13 are retired by the scope amendment, not
successful tests or remaining requirements. ADR Section 5 now authorizes public
delivery without a module release or another staged amendment. Project v6,
bundle v8, host API v7, capabilities v2, reference v9/topic v5, log v3, update
state/schema snapshot v7, vocabulary v3, and ABI 6 remain unchanged. Completion
was established by every active AF row, not revival of the retired module
acceptance.

## 37. Portable host boundary and tiered-build conformance

Status: Required / Implemented; HOST-01 through HOST-14 are complete within the
accepted evidence boundaries in the ADR-0050 validation record.
[ADR-0050](adr/0050-portable-host-boundary-and-tiered-builds.md) is normative.
These requirements change host isolation, build, and platform qualification,
not GraphQL semantics or generated ABI 6.

| Area | Acceptance IDs | Required behavior |
| --- | --- | --- |
| Boundary and paths | HOST-01, HOST-02 | One enforced host facade, no OS branches in product logic, and identical portable logical-path admission. |
| Filesystem and processes | HOST-03 through HOST-06 | Verified identities, bounded reads, containment, locks, transactions, cancellation, descendant cleanup, writer backpressure, and preserved output-failure contracts. |
| Build interface | HOST-07, HOST-08 | Native release/debug never change `dist`; named distribution targets use the explicit host/tool matrix, including the Linux compatibility environment. macOS-only four-target builds preflight before work and reject missing SDKs/images without acquisition. |
| Tier 1 | HOST-09, HOST-10 | Native complete validation on macOS arm64 and Linux x86_64, two required CI lanes, and the glibc 2.17 release check. |
| Tier 2 | HOST-11 | macOS x86_64 and Windows x86_64 release binaries build/link with correct Mach-O/PE identities and distribution slots. Test-executable linking and runtime checks are not required. Fail-closed design constraints remain unchanged, without claiming runtime qualification. |
| Product admission | HOST-12 through HOST-14 | Local structural/native Tier 1 smoke verification remains separate from complete qualification and marks foreign Tier 1 and all Tier 2 runtime checks unexecuted with reasons. Package publishability does not require registry uploads. MSRVs, dependency direction, unsafe prohibition, quality gates, reviews, and final-source evidence remain exact. |

Tier 2 results never satisfy a missing Tier 1 gate. A successful cross-build
does not replace native Tier 1 workflow and adversarial testing. Optional Tier 2
focused results must distinguish executed cases from unexecuted coverage;
availability of a native host does not create an additional completion gate.

## 38. Mock contract validation without completion state

Status: Required / Implemented; CV-01 through CV-09 have local macOS arm64
evidence in the ADR-0051 validation record.
[ADR-0051](adr/0051-mock-contract-validation-without-completion-state.md) is
normative. These requirements remove business-completion declarations, not
structural admission, typed contracts, or runtime safety.

| Area | Acceptance IDs | Required behavior |
| --- | --- | --- |
| Intentional-error scaffolds | CV-01, CV-02 | Fresh valid project and exported bundle pass admission without resolver edits; invocation returns the fixed `NOT_IMPLEMENTED` business error. Implemented operations and later requests work; ordinary internal errors remain sanitized. Entry bodies are not invoked by validation. |
| Request failure | CV-03 | Later stub failure rolls back earlier Mutation writes. Sticky failure, protected calls, independent-failure precedence, and recovery remain intact. |
| Module and hook safety | CV-04, CV-05 | One-field typed descriptors reject legacy state, extra fields, metatables, wrong signatures/returns, missing active files, and invalid initialization. Seeds, identities, limits, cancellation, disabled-hook selection, all six no-op hooks, registration, request state, patches, and phase permissions retain their contracts. |
| Clean break | CV-06 | Exact project v7, bundle v9, API v8, reference v10/topic v6, and management v8 identities reject old/mixed input before writes; fresh-destination migration regenerates owned data. |
| Removed and retained surfaces | CV-07 | No completion counts, inventory command/API/report, or reference completion types remain. Removed options fail as usage errors. Check v3 retains exact shape, scope/readiness distinction, safe navigation, direct output, and BrokenPipe behavior. |
| Update and ownership | CV-08 | New schema operations receive intentional-error bodies without blocking admission solely for that reason. Edited sources/seeds, `.dist`, recovery, current-input validation, export, and restart retain their contracts. |
| Product admission | CV-09 | Canonical examples, guidance, quick start, exact fixtures, retained safety/refinement/MSRV/ABI gates, quality checks, final-source reviews, and full product validator agree on the new contract. |

Historical AF/AB/CH evidence is preserved without being counted as new CV
acceptance. Check v3, capabilities v2, log v3, vocabulary v3, and Rust ABI 6
remain unchanged. No scenario-test framework or per-mock business-test suite
is required. Linux, Windows, and other non-local environments were unexecuted
for this implementation assignment.

## 39. Adopted schema and local-generation conformance

Status: Local Passed; both independent reviews closed after focused correction
re-review. The contract is
[ADR-0052](adr/0052-adopted-schema-snapshots-and-mock-codegen.md). These are Mock
authoring and lifecycle requirements, not changes to GraphQL execution rules.

| Area | Acceptance IDs | Required behavior |
| --- | --- | --- |
| Snapshot and normalization | AS-01, AS-02 | Reproducible adopted public SDL retains description values and order; ignored formatting does not cause freshness drift; public and internal identities remain separate. |
| Read boundaries | AS-03, AS-04 | Internal commands never open public source paths; local contract drift requires codegen, while reference does not require valid modules or seeds. |
| Generation and recovery | AS-05, AS-06 | Update adopts public SDL and regenerates once; codegen does not adopt it. Both retain edit protection, joint-input migration, no-op behavior, and complete generation/recovery checks. |
| Comparison interface | AS-07 | Real CLI read-only comparison has exact text/JSON status and exit behavior, with no false freshness success for unavailable inputs. |
| Descriptions | AS-08 | Reference, introspection, and exported descriptions agree with the adopted schema; private visibility, absent/empty values, escaping, and output bounds remain correct. |
| Input and ownership safety | AS-09 | Local storage confinement, bounded normalization, verified payload/summary/type identities, and truthful logical diagnostic locations reject invalid inputs. |
| Migration and portability | AS-10 | Fresh-init migration rejects old/mixed project identities without writes; relocated projects and portable bundles work without original public sources. |

Retain ADR-0051's completion-free contracts and existing Mock rollback, secrecy,
resource, and ownership coverage. Local macOS evidence is required for this
assignment; remote Linux, other platforms, and distribution qualification are
not implied by local success. Earlier acceptance records remain historical.
Executed AS evidence is recorded in the
ADR-0052 validation record.

## 40. Trait-only Rust integration conformance

Status: Required / Implemented; RI-01 through RI-08 Passed locally on macOS
arm64. Executed assertions and quality gates are mapped in the
validation record; both independent reviews
passed within their recorded scope. The contract
is [ADR-0053](adr/0053-trait-only-resolvers-and-application-boundaries.md),
with work packages in Handoff Section 40. Its acceptance replaces only the
projection/scaffold-specific requirements of earlier Rust usability programs.
Retain applicable compilation, doctor, ownership, resource, and execution
coverage when retiring old tests.

| ID | Required behavior |
| --- | --- |
| RI-01 | Complete traits are the only field implementation form; projection/scaffold APIs are absent, and removed CLI syntax fails without writes. |
| RI-02 | External domain values remain typed through normal/calculated/nested field resolution without DTOs, JSON round trips, or domain dependencies on Chigiri. |
| RI-03 | Compilation rejects missing implementations, parent/output/nullability mismatches, inconsistent error types, and missing/wrong mappers after SDL regeneration. |
| RI-04 | One external application error and mapper support ordinary `?` in all groups, with exactly-once conversion and preserved direct/HTTP GraphQL error semantics. |
| RI-05 | Framework and list-item failures bypass application mapping and preserve masking, index paths, observation, and error IDs; the identity mapper supports `FieldError`. |
| RI-06 | Existing named/closure factories compile and preserve request-local state, service sharing, preparation failure/rejection, timeout/drop, and response hooks. |
| RI-07 | ABI 7 regeneration/ownership checks protect old and modified output, preserve semantic layout/resource bounds, and reject ABI 6 execution. |
| RI-08 | Maintained examples and signatures compile with the new contract; actual IDE checks are versioned, or explicitly unexecuted. |

The validation record links each row to executed tests and supporting logs. An IDE smoke check supplements automated gates; compilation
alone must not be described as manual editor verification. Use local macOS
arm64 evidence and record unexecuted platforms. No full-product validator or
non-local qualification is required by this assignment; support tiers remain
unchanged.

## 41. Subscription and bounded delivery conformance

Status: Required contract / Implemented with local macOS acceptance.
The contract is
[ADR-0054](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md), with
work packages in Handoff Section 41 and target architecture in Section 49.
Current source implements typed subscriptions, bounded delivery and the three
profiles. Actual executed scopes and final acceptance evidence are mapped in the
validation record.
No former Parse/Reject assertion, prior ABI evidence or illustrative snippet
proves a current execution pass. ADR-0054 Section 5 finalizes the
initial safety defaults, configuration hard maxima, and fallible construction
policy. SU-WP0 preserves them while freezing concrete contracts, profiles,
remaining internal bounds, and the version/migration table before product code
changes. These defaults do not establish production capacity.

| ID | Required behavior | Executable evidence route |
| --- | --- | --- |
| SU-01 | Subscription validation and complete typed source/output contracts preserve selection, coercion, nesting, aliases, abstract types, and nullability; invalid consumers fail compilation. | Core subscription validation/execution, runtime contracts and CLI complete generated/shape positive consumers with diagnostic-specific compiler negatives. |
| SU-02 | Setup runs once, while each event gets fresh prepared context and request-local execution state; shared services are deliberate and credential checks are current. | Runtime setup/event contracts and facade `driver/tests/{event,refresh}.rs`, including revocation and original-argument policy changes. |
| SU-03 | Setup, event field, preparation, source, and terminal failures remain distinct, masked, correctly mapped once, and compatible with header-hook timing. | Core/runtime error/lifecycle fixtures, facade initial-completion/typed terminal/caller fixtures and real protocol consumers. |
| SU-04 | Sugar/inject equivalence, accepted defaults/hard maxima, replacement order, immutable effective settings/origins, and endpoint-clone quota ownership match the one-graph contract. Invalid numeric settings/combinations return construction errors without panic, clamping, or delayed startup failure. | Facade settings tests and compiler-negative rustdoc; delivery permits/resources and concurrent-admission fixtures. |
| SU-05 | Individual and combined limits, including shared encoded-buffer pressure, trigger OR-based decisions, complete post-eviction rechecks, atomic admission, and timer-driven enforcement without new input. | Delivery evaluator/controller/property matrix, Outbox timers and phase/worker deadline fixtures; validation per-bound map. |
| SU-06 | Default demand never silently drops; explicit DropOldest preserves order and current cumulative metadata, while control, terminal, and partial sends are not evicted. | Outbox queue/terminal/freeze/observation fixtures, HTTP metadata codec and positive genuine-Mock drop integration. |
| SU-07 | All termination paths release owned sources, tasks, workers, permits, queues, and timers; unrelated multiplexed operations survive an isolated failure. | Physical/quarantine/process-shutdown fixtures, HTTP/WS supervision and actual multiplexed consumers; validation termination-owner map. |
| SU-08 | Small real Rust consumers exercise all three pinned protocols through Hyper and Axum, including multiple events, errors, cancellation, and bounded slow consumers. | `crates/testkit/tests/subscription_streaming.rs` and its six-profile real loopback fixture with strict joins. |
| SU-09 | Real Mock consumers exercise all three protocols, typed post-commit publish, rollback, registration/order, fresh subscriber views, immutable captured roots with automatic declared relations through each event's current snapshot, private provenance, worker recovery, restart, and seed reset. | `crates/mock/tests/acceptance/subscriptions`, fresh preparation/dispatch fixtures and publication/request/worker tests. |
| SU-10 | Malformed/oversized input, leakage, privilege/provenance forgery, exhausted staging, and stalled peers fail safely within bounds. | Protocol codec/ingress/admission negatives, real consumers, Mock publication owner/provenance/permission/capacity and recovery fixtures. |
| SU-11 | Documented Rust/Mock and advanced Apollo recipes work; protected generation, adopted snapshots, reference/export, rejection, and migration match the approved identity table. | Subscription walkthrough, versioned Apollo recipe, generated consumers, protected migration/mixed-reader/reference/export and exact public-schema fixtures. |
| SU-12 | Focused quality gates and independent review evidence identify actual source and scope, without claiming unexecuted broad or non-local qualification. | Source-bound validation logs/inventories, final canonical/quality checks and two independent whole-goal Beads reviews. |

GraphQL September 2025 Sections 2.4, 3.3, 5.2.1.1, 5.2.4.1, and 6.2.3 require
positive and negative coverage at cutover. Distinguish GraphQL's execution
requirements from Chigiri's QoS policy and the separately pinned transport
profiles. Do not mark an entire protocol passed from a framing-only unit test.

Use bounded local macOS acceptance and record client/protocol versions. No
mandatory large performance, remote Linux, cross-build, or full-product campaign
is added. Keep existing Query/Mutation, Mock atomicity/security, generation,
dependency/MSRV, and ownership coverage. Product support tiers are unchanged.
New review findings and verification are tracked in Beads under the
[review workflow](review-workflow.md), not a second document status ledger.

## 42. Subscription consumer ergonomics conformance

Status: Required / Locally verified on macOS arm64. See the
validation record for SE-01 through
SE-08 source-bound evidence and scoped independent review records.
The contract is
[ADR-0055](adr/0055-subscription-configuration-and-lifecycle-ergonomics.md),
Architecture Section 50 and Handoff Section 42. The rows below are obligations,
not passing results. ADR-0054's typed execution and physical resource contracts
remain intact; manual lifecycle ownership and closed pre-stream failure
propagation are explicitly amended by ADR-0055 Section 5.

| ID | Required behavior | Evidence route |
| --- | --- | --- |
| SE-01 | Public bound constants cover all configurable Rust subscription/protocol settings and are used by validators; defaults and inclusive boundaries are unchanged. | External constants consumer and final range/metadata validation fixtures. |
| SE-02 | Numeric range/fit/overflow details retain classification, units, origins and sugar/inject parity without implicit budget adjustment. | Typed diagnostic and graph-validation matrix, including both fit conflicts. |
| SE-03 | Safe numeric evidence is consistent across accessor, Debug and Display; other slots and secret material remain redacted. | Explicit allowlist and negative leakage fixtures. |
| SE-04 | Official route services start shared SDK supervision fallibly and require no hidden APIs, manual lifecycle owners, server extensions or separate run. Adapters only bridge native interfaces and the neutral default stays Tokio-free. | External public-only native Router/service/hook consumers, missing-runtime/startup failures, both bridge compilation and dependency-boundary checks. |
| SE-05 | No-hook fallback, Immediate and default/overridden Graceful preserve finite/streaming execution, deadlines, physical ownership, clone sharing and endpoint isolation. SDK-first hooks do not claim server-wide admission stop, whole-server time bounds or peer receipt. | Automatic-start/stop and supervisor-failure races, notification idempotence, active-versus-waiting events, finite requests, native hook ordering, optional outcomes and process-isolated recovery fixtures. |
| SE-06 | The sizing guide distinguishes raw values, profile-encoded events, reservations and memory exclusions; examples' arithmetic and bounded encoding are correct. | Checked sizing examples and production-encoder exact-fit/overflow fixtures. |
| SE-07 | The native-router Axum example serves finite Query/POST SSE, rejects wrong routes/origins before setup, cancels on disconnect, and handles normal/expired shutdown through optional standard hooks without manual drivers or an extra await. | Local loopback tests for no-hook and native-hook paths, safe pre-stream errors, abnormal body failure, other-route availability during SDK drain, compatible Cargo layout and producer identity/ABI checks. |
| SE-08 | Focused quality gates and independent review identify actual source, checks and limits. | Local affected tests, MSRV/stable compilation, Clippy, rustdoc, formatting, implementation rules, diff checks and Beads review. |

No new ABI, Mock/CLI format, protocol or capacity qualification is implied.
Keep tests independent of Beads and the sibling report. Remote, full-product,
cross-build, browser, load, soak and fuzz campaigns are not acceptance
dependencies; preserve honest unexecuted scope and existing platform tiers.

## 43. Distributed tracing propagation conformance

Status: Implemented / Focused local macOS evidence. Contract:
[ADR-0056](adr/0056-distributed-tracing-context-propagation.md), Architecture
Section 51, and Handoff Section 43. These are obligations, not passing results.
Preserve OBS-01 through OBS-09 and the ADR-0054/0055 ownership and resource
contracts; event-only observer tests do not prove execution-context propagation.

| ID | Required behavior | Focused evidence route |
| --- | --- | --- |
| TR-01 | One public instrumentation registration supplies current context to context factories/resolvers without a context span field, plugin or generated edit. | External generated consumer and standard OTel context access. |
| TR-02 | Direct and HTTP paths adopt real upstream parents; valid current middleware context takes precedence over headers and legacy opaque IDs. | Known trace/span ID assertions and conflicting/missing parent matrix. |
| TR-03 | SDK task/poll/thread boundaries, parallel work and ID failure preserve parent/suppression ownership and ambient restoration, including nested owning suppression scopes. | Deterministic interleaving, both ambient suppression states and request/endpoint/field isolation fixtures. |
| TR-04 | Downstream carrier and ordinary contextual tracing spans retain correct SDK parentage. | Real local downstream request and compatible context-activating tracing layer, including an outer middleware span. |
| TR-05 | Each recorded event gets a new root/setup link and correct identity; all event scopes preserve captured setup suppression and fresh subscriber context across all profiles and both adapters. | Prompt setup close, suppressed events with/without a valid parent, and no publisher/previous-event inheritance. |
| TR-06 | Suppressed/unsuppressed, valid/no-parent, Chigiri Record/Drop and provider sampled/non-recording cases preserve propagation and independent observer delivery without forced instrumentation. | Standard suppression queries, zero suppressed SDK backend creations, observer calls, sampling flags and event-boundary links. |
| TR-07 | Every terminal path ends scopes once, preserves suppression during disposal, restores parent/suppression and releases references while retaining masking and physical ownership. | Success/rejection/timeout/disconnect/cancellation/unwind/shutdown fixtures. |
| TR-08 | Bounded carrier/retention/export failures degrade telemetry only; parent fallback and optional correlation loss never clear suppression, and baggage/secret data stay excluded. | Invalid/duplicate/oversized header, capacity pressure, suppression-control and redaction negatives. |
| TR-09 | Opt-out dependency and per-field cost boundaries remain; old/custom observers work without automatic duplicate spans. | Dependency consumer, field-disabled/enabled scope checks and registration migration. |
| TR-10 | Maintained examples and focused local quality/review evidence identify actual source, dependencies and unexecuted scope. | MSRV/stable consumer/test checks, Clippy, rustdoc, fmt, implementation rules and source-bound Beads review. |

The recorder must distinguish trace IDs, parent IDs, flags, links and end counts;
a constant-trace-ID fake cannot establish these requirements. Tests remain
independent of Beads and historical acceptance prose. External collectors,
remote Linux, cross/distribution builds, full-product validation and broad
performance/fuzz qualification are not prerequisites for this focused local
assignment. Keep support tiers, generated ABI 8 and existing formats unchanged.

## 44. Typed Mock event source and consumer conformance

Status: Implemented / EV-01 through EV-12 passed with focused local macOS
evidence and two accepting independent reviews. Contract:
[ADR-0057](adr/0057-typed-mock-event-sources-and-consumers.md), Architecture
Section 52 and Handoff Section 44. The
validation record maps these normative
obligations to actual tests. The ADR specifies exact API, timing, transaction, depth,
retention and clean-cutover rules.

| ID | Required behavior | Focused evidence route |
| --- | --- | --- |
| EV-01 | SDL-type selectors, destination-owned sources and phase privileges agree with runtime checks; old field selectors, forged sources and caught invalid calls fail safely. | Generated and real-VM scalar/list/null/object, provenance and role negatives. |
| EV-02 | Exact/all-topic matching broadcasts once to compatible same-type destinations and preserves named-type isolation, post-commit order, registration linearization, bounded topic retention and broadcast progress. | Two-consumer fan-out, dynamic topics, rollback and no-replay cases. |
| EV-03 | Independent sources actively generate after one interval without demand and ignore fixed/timeleap wall-time changes. | Controlled host time and a small real timer/loopback case. |
| EV-04 | Late ticks skip positions without a burst; finite final emission and looping are correct, and invalid definitions/exhaustion are bounded. | Tick arithmetic, empty/holey/oversized values and interval negatives. |
| EV-05 | Explicit consumer registrations, typed preserved scaffolds, startup activation and disabled behavior are consistent. | Codegen/scoped/full checks and partial-startup cleanup. |
| EV-06 | Consumer model changes and child publications commit atomically; failure/cancellation rolls back only that candidate and permits later processing without retry. | Actual store/queue observations across successful and failed callbacks. |
| EV-07 | Host depth permits 0-to-3 chains and rejects depth 4 atomically despite catches, forged metadata and cross-field/topic publication. | Branching/self-cycle and depth-3 model-only cases. |
| EV-08 | FIFO/single invocation, captured Record provenance, fresh store/clock/state and publisher exclusion hold without a retained VM/closure. | Independent consumers and GraphQL event paths, captured-root changes and lifecycle probes. |
| EV-09 | Source count/byte/age/shared limits compose by OR; overflow is isolated and physical ownership remains charged. | Equality/overflow, slow consumers, default stop and explicit owned eviction. |
| EV-10 | Actual source/encoded drops are counted once; ticks and shutdown/client loss are excluded, with bounded safe logging. | Consumer metadata and actual wire counters under both delivery policies. |
| EV-11 | Completion, disconnect, overflow, shutdown, restart and cancellation release actual timer/worker/queue ownership without chain draining. | Pending callback and producer cleanup with the existing SDK lifecycle. |
| EV-12 | Fresh project/bundle workflows and versions are coordinated, old/mixed inputs are rejected and adopted authority/sandbox/ABI boundaries remain. | Renamed-schema init/codegen/check/reference/export plus actual HTTP acceptance. |

Keep tests independent of Beads, review prose and previous acceptance manifests.
Focused local validation and independent reviews must identify executed source
and limits. No runtime evidence is established by publishing this table, and
no full-product or non-local qualification is required for this scope.

## 45. Tiered validation and weekly qualification

Contract: [ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md).
These are executable requirements, not review-status oracles.

| ID | Required evidence |
| --- | --- |
| VAL-01 | `scripts/test-validation-profiles.py` verifies default/explicit selection, unknown-profile rejection, fixed functional/optimized safety coverage and preserved broad full coverage. |
| VAL-02 | `scripts/test-validation.py` and `scripts/test-validation-profiles.py` reject incomplete, failed, cancelled, reordered, duplicate, source-mutated and profile-substituted completion. |
| VAL-03 | `scripts/test-required-jobs.py` rejects missing, skipped, failed, cancelled and wrong-profile native receipts. |
| VAL-04 | `scripts/test-validation-workflows.py` verifies both native PR profiles, the one weekly schedule, full native coverage, six finite long fuzz jobs and isolated formal load/soak with failure artifacts. |
| VAL-05 | `scripts/test-validation-workflows.py` executes the actual fuzz runner with a controlled producer to prove copied-corpus isolation, unique retained failure evidence, unknown-target rejection and source-mutation failure. |
| VAL-06 | The explicit full product profile and selected formal runners produce source-bound results; actual hosted/non-local execution and formal human acceptance are recorded separately from configuration inspection. |

## 46. Craftguild standards integration

Contract: [ADR-0059](adr/0059-craftguild-standards-adoption.md).

Required evidence includes offline copied-binary common presentation and exact
embedded SBOM bytes; unchanged machine and worker contracts; actual selected
build and runtime/embedded asset evidence; explicit implementation
roles and Code boundaries; bounded canonical regeneration; all retained ordinary
product commands; owned installation; in-memory release authority validation;
and source-only acquisition without siblings. Actual native/foreign platform,
remote CI, formal qualification and publication scopes are reported separately.
No inspection or fixture result establishes an unexecuted environment.

Build-tool regression suites are owned by standards using neutral fixtures.
Consumer SDK/preflight/entry/measurement test matrices are retired rather than
copied into standards. Actual builds and product-specific verification remain.
