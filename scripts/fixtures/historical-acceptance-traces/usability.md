# Frozen developer usability acceptance trace

This table is the 54-row acceptance trace from the historical developer usability
review, copied byte-for-byte when the Markdown review ledger was retired.
Trace checkers compare the current trace in `docs/spec-conformance.md`
with it. Do not edit it; the complete review record is Beads issue
`lab01-c43.6`.

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
| PROJ-01 | `chigiri_cli::rust_projection::proj_01_generates_correct_method_for_each_shape` |
| PROJ-02 | `chigiri_cli::rust_projection::proj_02_does_not_generate_a_root_projection` |
| PROJ-03 | `chigiri_projection_consumer::direct::proj_03_projection_and_full_trait_results_match` |
| PROJ-04 | `chigiri_projection_consumer::direct::proj_04_projection_and_async_error_fields_can_mix` |
| PROJ-05 | `chigiri_cli::rust_projection::proj_05_typestate_violations_fail_to_compile` |
| PROJ-06 | `scripts/check-usability-contract.sh` |
| PROJ-07 | `chigiri_projection_consumer::governance::proj_07_completion_and_policy_not_circumvented` |
| PROJ-08 | `chigiri_projection_consumer::direct::proj_08_complete_projection_traits_can_be_passed_to_the_same_assembly` |
| PROJ-09 | `chigiri_cli::rust_projection::proj_09_private_stage_is_hidden_from_rustdoc_and_the_ide` |
| PROJ-10 | `chigiri_cli::rust_projection::proj_10_detects_drift_in_private_shards` |
| PROJ-11 | `chigiri_cli::rust_projection::proj_11_generated_size_and_semantic_units_are_protected` (ABI 6 semantic-unit layout) |
| PROJ-12 | `scripts/check-large-consumer-conformance.sh` (ABI 6 MSRV/stable gate) |
| PROJ-13 | `scripts/check-usability-contract.sh` (ABI 6 inventory/surface and ABI 4 mismatch fixture) |
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
