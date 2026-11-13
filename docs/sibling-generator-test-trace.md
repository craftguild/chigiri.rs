# Sibling generator exact test trace

Status: This table maps every implemented acceptance ID to its exact evidence.
The validation record controls the final aggregate and review result; a row in
this table is not, by itself, a Passed determination.

ADR-0053/0054 retire scaffold/projection owners and establish current ABI 8
traits. Active references below use their retained trait, diagnostic, hygiene
and layout assertions; historical validation records remain unchanged.

Each entry is a repository-relative exact `path#test-or-script-assertion`
reference. Rust integration tests name the binary test file; module tests name
their owner by source path and function. When one test covers several failure
conditions, the table lists the distinct supporting assertions instead of
relying on the test name alone. Fixed Rust 1.95.0 ci/full workspace execution and the
script plan are connected through [`scripts/validate.sh`](../scripts/validate.sh).

ADR-0059 retires SG-INFO-02 and SG-INFO-03's repetitive Git/cache build
matrices. Their IDs remain historical; the SDK observation behavior is unchanged.
The active trace contains 39 IDs. Public metadata/schema consistency and actual
built-binary behavior remain product checks; shared build machinery is tested
in standards with neutral fixtures. No product-specific matrix is moved there.
SG-VERIFY-04 reviews A/B run after the complete automated gate and are recorded
in the validation record. SG-LAYOUT-08's real migration of the six existing SDKs from the legacy
layout is recorded in validation checkpoint L3 and its saved migration
inventory. Generation of the original captured corpus was historical evidence. The current
full-schema fixture requires fresh generation and verification.
Each CI run verifies exactness, quality, and update behavior for the current
layout.

## SG-CLI-01

- `crates/cli/tests/rust_terminal.rs#sg_cli_01_help_succeeds_at_all_levels_offline_without_a_manifest`
- `crates/cli/tests/rust_version.rs#human_and_json_versions_expose_the_same_fields`
- `crates/cli/tests/rust_version.rs#root_and_explicit_version_commands_return_identical_build_information`

## SG-CLI-02

- `crates/codegen/src/filesystem/tests.rs#sg_cli_02_exclusive_creation_never_overwrites_files_directories_links_or_fifos`
- `crates/codegen/src/manifest/io_tests.rs#sg_cli_02_init_preserves_existing_entries_and_rejects_parent_symlinks`
- `crates/codegen/src/manifest/io_tests.rs#sg_cli_02_init_creates_only_the_canonical_template_and_requires_runtime_selection`
- `crates/cli/tests/rust_terminal.rs#sg_cli_02_init_creates_manifest_once_and_ignores_environment_overrides`

## SG-CLI-03

- `crates/codegen/src/manifest/tests.rs#sg_cli_03_validates_consumer_features_against_additive_rules`
- `crates/codegen/src/manifest/tests.rs#sg_cli_03_enum_bindings_validate_existence_and_kind_against_the_semantic_schema`
- `crates/codegen/src/manifest/tests.rs#sg_cli_03_parses_defaults_and_consumer_values`
- `crates/codegen/src/manifest/tests.rs#sg_cli_03_rejects_unknown_duplicate_and_legacy_keys_at_all_levels`
- `crates/codegen/src/manifest/tests.rs#sg_cli_03_rejects_missing_required_fields_and_invalid_cargo_values`

## SG-CLI-04

- `crates/cli/tests/rust_contract.rs#sg_cli_04_legacy_overrides_and_environment_values_defer_to_manifest_authority`
- `crates/cli/tests/rust_terminal.rs#sg_cli_04_help_and_init_reject_unknown_duplicate_and_legacy_options_before_side_effects`
- `crates/cli/tests/rust_version.rs#version_rejects_legacy_overrides_and_unrecognized_arguments`

## SG-CLI-05

- `crates/codegen/src/doctor/metadata/runtime/tests.rs#sg_cli_05_rejects_mixed_cargo_git_sources_and_revisions`
- `crates/codegen/src/doctor/metadata/runtime/tests.rs#sg_cli_05_accepts_matching_cargo_ssh_and_https_source_identities`
- `crates/codegen/src/doctor/metadata/runtime/tests.rs#sg_cli_05_rejects_duplicate_runtime_identities`
- `crates/codegen/src/doctor/metadata/runtime/tests.rs#sg_cli_05_requires_complete_matching_git_revisions`
- `crates/codegen/src/manifest/io_tests.rs#sg_cli_05_resolves_the_consumer_from_the_manifest_root_and_rejects_output_overlap`
- `crates/codegen/src/manifest/tests.rs#sg_cli_05_runtime_source_and_revision_are_explicit_and_cannot_be_mixed`
- `crates/codegen/src/manifest/tests.rs#sg_cli_05_runtime_only_allows_parent_components_and_rejects_input_output_overlap`
- `crates/codegen/src/manifest/tests.rs#accepts_unambiguous_repository_urls_without_credentials`

## SG-CLI-06

- `crates/codegen/src/cargo_diagnostics/tests.rs#diagnostic_policy_can_deny_warnings`
- `crates/codegen/src/diagnostic_render/tests.rs#record_and_byte_limits_append_one_terminal_error`
- `crates/codegen/src/diagnostic_render/tests.rs#truncates_each_report_field_at_a_utf8_boundary`
- `crates/codegen/src/diagnostic_render/tests.rs#structured_report_preserves_origins_coordinates_and_complete_labels`
- `crates/cli/tests/rust_diagnostics.rs#diag_04_07_13_14_static_after_successfully_fixing_cargo_and_side_effects`
- `crates/cli/tests/rust_diagnostics.rs#diag_03_commands_and_retired_names_report_one_json_diagnostic_on_stderr`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#sg_cli_06_doctor_deny_warning_policy_returns_one_report`
- `crates/codegen/tests/sibling_cli/doctor_inputs.rs#sg_cli_06_unknown_dependency_feature_is_reported_as_input_failure`
- `crates/cli/tests/rust_contract.rs#sg_cli_06_deny_warnings_fails_before_commit_while_default_policy_keeps_warnings`
- `crates/cli/tests/rust_contract.rs#sg_cli_06_runs_generate_update_and_check_with_the_selected_manifest`

## SG-CLI-07

- `crates/codegen/src/cargo_diagnostics/tests.rs#keeps_distinct_lib_and_test_diagnostics_and_deduplicates_only_exact_matches`
- `crates/codegen/src/cargo_diagnostics/tests.rs#sg_cli_07_preserves_rustc_code_severity_labels_help_notes_and_raw_source`
- `crates/codegen/src/cargo_diagnostics/tests.rs#sg_cli_07_redacts_rustc_text_and_unmodeled_cargo_secrets`
- `crates/codegen/src/contract/check/difference_tests.rs#large_diffs_report_totals_truncate_details_and_redact_secret_file_names`
- `crates/codegen/src/diagnostic_render/tests.rs#label_help_and_note_limits_are_bounded_without_proportional_rendering`
- `crates/codegen/src/diagnostic_render/tests.rs#all_fields_debug_and_display_redact_paths_uris_credentials_and_terminal_controls`
- `crates/codegen/src/manifest/tests.rs#diagnostics_do_not_expose_secrets_or_manifest_source_text`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#sg_cli_07_malformed_cargo_metadata_is_redacted_and_returns_exit_two`
- `crates/cli/tests/rust_contract.rs#sg_cli_07_manifest_and_tool_failures_form_one_redacted_report`

## SG-CRATE-01

- `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`

## SG-CRATE-02

- `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`
- `crates/cli/tests/rust_enum_binding.rs#enum_02_05_generic_path_bindings_compile_and_mapping_failures_are_masked`

## SG-CRATE-03

- `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`
- `crates/cli/tests/rust_generation.rs#generated_public_contracts_compile_and_missing_ones_are_compile_errors`

## SG-CRATE-04

- `crates/cli/tests/rust_trait_errors.rs#ri_01_02_traits_compile_with_external_domain_values_and_errors`
- `crates/codegen/tests/sibling_contract.rs#sg_crate_04_keyword_and_metadata_name_and_stdlib_shadow_standalone`

## SG-CRATE-05

- `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_nested_legacy_and_record_mismatch_without_input_values`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#rejects_consumers_that_mix_core_types_from_another_source`
- `crates/codegen/tests/sibling_contract.rs#sg_crate_01_03_standalone_and_three_crate_enum_contract_compile_separately`
- `crates/cli/tests/rust_traits.rs#ri_01_08_private_adapters_and_assembly_stages_are_hidden_from_rustdoc`

## SG-CRATE-06

- `crates/codegen/src/manifest/io_tests.rs#sg_crate_06_preserves_the_original_schema_digest_during_handle_validation`
- `crates/codegen/tests/sibling_contract.rs#sg_crate_06_manifest_comment_changes_provenance_while_preserving_semantic_binding`
- `crates/cli/tests/rust_enum_binding.rs#enum_06_comment_changes_preserve_config_digest_but_change_descriptor_drift`

## SG-FS-01

- `crates/codegen/src/contract/write/tests.rs#sg_fs_01_generate_is_absent_so_update_is_required`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_01_create_and_rename_does_not_overwrite_existing_entries_with_empty_dir`
- `crates/cli/tests/rust_contract.rs#sg_fs_01_real_cli_rejects_empty_directories_and_unknown_files_for_all_operations`

## SG-FS-02

- `crates/codegen/src/contract/write/input_tests.rs#sg_fs_02_rejects_output_nested_inside_another_schema_before_parent_creation`
- `crates/codegen/src/doctor/source/tests.rs#ignores_symlinks_and_fifos`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_02_parent_creation_rejects_links_and_empty_removal_preserves_nonempty_directories`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_02_lock_detects_parent_replacement_without_switching_trees`
- `crates/codegen/src/filesystem/tests.rs#sg_fs_02_open_detects_path_replacement_behind_an_existing_descriptor`
- `crates/codegen/src/filesystem/tests.rs#sg_fs_02_read_detects_file_growth_and_syntax_changes`
- `crates/codegen/src/filesystem/tests.rs#sg_fs_02_validates_relative_components_and_bytes_before_resource_limits`
- `crates/codegen/src/filesystem/tests.rs#sg_fs_02_rejects_operations_after_an_ancestor_is_replaced`
- `crates/codegen/src/filesystem/tests.rs#sg_fs_02_rejects_parent_and_final_symlinks_and_fifos_with_open_descriptors`
- `crates/codegen/src/filesystem/tree_tests.rs#sg_fs_02_tree_rejects_file_count_individual_and_total_byte_limits_before_allocation`
- `crates/codegen/src/filesystem/tree_tests.rs#fs02_tree_rejects_changes_after_reading_files_or_ancestor_entries`
- `crates/codegen/src/filesystem/tree_tests.rs#sg_fs_02_tree_captures_inventory_and_path_sequence_bytes_with_empty_directory`
- `crates/codegen/src/filesystem/tree_tests.rs#sg_fs_02_tree_detects_nested_directory_replacement_during_traversal`
- `crates/codegen/src/filesystem/tree_tests.rs#sg_fs_02_tree_counts_unselected_entries_toward_depth_and_entry_limits`
- `crates/codegen/src/filesystem/write_tests.rs#sg_fs_02_tree_rejects_entry_collisions_and_path_limits_before_writing_parts`
- `crates/codegen/src/manifest/io_tests.rs#sg_fs_02_rejects_manifest_and_sdl_symlinks_and_oversized_inputs`
- `crates/codegen/src/manifest/io_tests.rs#sg_fs_02_manifest_debug_output_excludes_captured_bytes_and_raw_sdl`
- `crates/codegen/tests/sibling_cli/doctor_inputs.rs#sg_fs_02_rejects_consumer_manifest_and_parent_symlinks_before_cargo`
- `crates/codegen/tests/sibling_cli/ownership.rs#sg_fs_02_marker_symlinks_are_neither_tracked_nor_accepted_by_check_or_update`

## SG-FS-03

- `crates/codegen/src/contract/write/input_tests.rs#input_failure_keeps_the_old_tree_and_clears_only_candidate_properties`
- `crates/codegen/src/contract/write/input_tests.rs#sg_fs_03_detects_manifest_and_sdl_changes_after_candidate_creation_and_preserves_live_output`
- `crates/codegen/src/contract/write/input_tests.rs#sg_fs_03_real_cargo_compile_failure_preserves_old_output_and_manifest_bytes`
- `crates/codegen/src/contract/write/term_tests.rs#sg_fs_03_hold_both_cancellation_and_rollback_failure`
- `crates/codegen/src/contract/write/tests.rs#sg_fs_03_precommit_failures_restore_the_previous_output_at_every_install_step`
- `crates/codegen/src/contract/write/tests.rs#sg_fs_03_failed_initial_verification_removes_the_new_output`
- `crates/codegen/src/contract/write/tests.rs#failed_install_and_rollback_preserve_old_backup_and_both_causes`
- `crates/codegen/src/contract/write/tests.rs#failed_candidate_install_preserves_both_live_and_backup_trees`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_03_post_rename_sync_failure_returns_moved_handle_for_restoration`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_03_successful_rename_returns_a_new_handle_and_invalidates_the_old_handle`
- `crates/codegen/tests/sibling_cli/ownership.rs#sg_fs_03_schema_shrink_removes_unused_ownership_shards_and_restore_artifacts`

## SG-FS-04

- `crates/codegen/src/contract/write/cleanup_tests.rs#failed_cleanup_prevents_a_subsequent_run`
- `crates/codegen/src/contract/write/cleanup_tests.rs#successful_cleanup_keeps_new_live_tree_and_removes_restore_artifacts`
- `crates/codegen/src/contract/write/cleanup_tests.rs#sg_fs_04_extractor_restores_old_bytes_and_rejects_existing_or_corrupt_archives`
- `crates/codegen/src/contract/write/cleanup_tests.rs#failed_restore_keeps_the_new_live_tree_and_complete_old_backup`
- `crates/codegen/src/contract/write/cleanup_tests.rs#sg_fs_04_archive_preserves_all_old_bytes_after_partial_backup_cleanup`
- `crates/codegen/src/filesystem/write_tests.rs#sg_fs_04_rejects_archive_content_and_path_swaps_before_unlinking`
- `crates/codegen/src/filesystem/write_tests.rs#sg_fs_04_stream_enforces_byte_limits_and_existing_entries_and_leaves_failed_new_file`
- `crates/codegen/src/filesystem/write_tests.rs#sg_fs_04_tree_preserves_all_unknown_files_and_deletes_only_known_trees`

## SG-FS-05

- `crates/codegen/src/cancellation/tests.rs#sg_fs_05_converts_term_and_int_into_isolated_process_cancellation`
- `crates/codegen/src/contract/write/term_tests.rs#sg_fs_05_interrupted_commit_restores_the_previous_live_bytes`
- `crates/codegen/src/contract/write/tests.rs#sg_fs_05_concurrent_output_rejects_stage_without_modifying_unrelated_files`
- `crates/host/src/filesystem/unix/lock.rs#lock_release_does_not_depend_on_inherited_descriptor_close`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_05_lock_excludes_a_competing_process_until_release`
- `crates/codegen/src/filesystem/mutation_tests.rs#sg_fs_05_lock_is_exclusive_across_clones_and_can_be_reacquired_after_drop`
- `crates/codegen/tests/sibling_cli/term.rs#sg_fs_05_cli_termination_during_format_or_cargo_preserves_old_bytes_and_exits_two`

## SG-FS-06

- `crates/codegen/src/contract/check/difference_tests.rs#sg_fs_06_reports_changed_missing_and_added_files_by_name`
- `crates/codegen/src/contract/check/difference_tests.rs#sg_fs_06_preserves_custom_manifest_paths_in_update_and_generate_guidance`
- `crates/codegen/src/contract/check/tests.rs#sg_fs_06_reports_extra_missing_and_changed_files_without_modifying_bytes`

## SG-PROC-01

- `crates/codegen/src/cancellation/tests.rs#sg_proc_01_panicking_scope_is_removed_and_other_threads_can_recover`
- `crates/codegen/src/cancellation/tests.rs#sg_proc_01_propagates_external_cancellation_to_nested_scopes_during_recovery`
- `crates/codegen/src/cargo_diagnostics/redaction_tests.rs#cargo_stream_parse_failures_use_the_same_diagnostic_redaction_boundary`
- `crates/codegen/src/cargo_diagnostics/tests.rs#sg_proc_01_fails_closed_when_cargo_output_or_diagnostic_count_exceeds_limits`
- `crates/codegen/src/cargo_diagnostics/tests.rs#sg_proc_01_rejects_missing_duplicate_failed_and_malformed_completion_records`
- `crates/codegen/src/contract/write/term_tests/process.rs#sg_proc_01_terminates_formatter_and_cargo_descendants_and_preserves_the_live_tree`
- `crates/codegen/src/doctor/metadata/tests.rs#sg_proc_01_byte_and_instances_and_opaque_id_limits_are_rejected`
- `crates/codegen/src/doctor/source/tests.rs#sg_proc_01_fails_closed_for_source_and_non_utf8_content`
- `crates/codegen/src/doctor/source/tests.rs#sg_proc_01_source_cache_enforces_per_file_and_total_byte_limits`
- `crates/codegen/src/process/cancel_tests.rs#sg_proc_01_reaps_parent_and_descendant_processes_after_cancelled_pipe_closure`
- `crates/codegen/src/process/cancel_tests.rs#sg_proc_01_already_cancelled_token_prevents_tool_spawn`
- `crates/codegen/src/process/tests.rs#sg_proc_01_parent_reaps_remaining_descendants_after_completion`
- `crates/codegen/src/process/tests.rs#proc_01_deadline_terminates_a_blocked_child_and_closes_its_pipe`
- `crates/codegen/src/process/tests.rs#sg_proc_01_stdin_stdout_stderr_is_bounded_at_the_same_time`
- `crates/codegen/src/process/tests.rs#proc_01_stops_writing_large_input_when_the_child_does_not_read_stdin`
- `crates/codegen/src/process/tests.rs#sg_proc_01_reports_io_failure_when_the_child_closes_stdin_early`
- `crates/codegen/src/process/tests.rs#sg_proc_01_recovers_after_independent_stdout_and_stderr_overflow`
- `crates/host/src/process/unix/tests.rs#sg_proc_01_signal_failure_confirms_completed_child_and_group_are_reaped`
- `crates/host/src/process/unix/tests.rs#sg_proc_01_signal_failure_does_not_claim_success_while_process_survives`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#doctor_termination_reaps_descendants_and_preserves_the_sdk_tree`

## SG-PROC-02

- `crates/codegen/src/doctor/metadata/tests.rs#sg_proc_02_rejects_duplicate_aliases_when_the_graph_contains_an_sdk_name`
- `crates/codegen/src/doctor/metadata/tests.rs#sg_proc_02_resolves_renames_and_dependency_kinds_without_mixing_build_dependencies`
- `crates/codegen/src/doctor/metadata/tests.rs#sg_proc_02_rejects_missing_optional_duplicate_and_broken_graph_references`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#sg_proc_02_missing_lockfile_rejects_update_requests_without_creating_a_lockfile`
- `crates/codegen/tests/sibling_cli/doctor_inputs.rs#sg_proc_02_inputs_changed_during_cargo_are_not_reported_as_success`
- `crates/codegen/tests/sibling_cli/doctor_inputs.rs#sg_proc_02_accepts_sdk_dev_aliases_for_all_targets_and_rejects_build_aliases`
- `crates/codegen/tests/sibling_cli/doctor_workspace.rs#sg_proc_02_all_targets_checks_consumer_test_targets_by_default`
- `crates/codegen/tests/sibling_cli/doctor_workspace.rs#sg_proc_02_dependency_feature_specifications_match_cargo_semantics`
- `crates/codegen/tests/sibling_cli/doctor_workspace.rs#sg_proc_02_resolves_target_packages_from_virtual_workspace_globs_and_exclusions`
- `crates/cli/tests/rust_doctor.rs#sg_proc_02_doctor_compiles_a_real_consumer_alias_with_locked_all_target_options`
- `crates/cli/tests/rust_doctor.rs#sg_proc_02_passes_manifest_features_and_all_targets_to_real_cargo`
- `crates/cli/tests/rust_doctor.rs#sg_proc_02_static_drift_does_not_start_cargo`
- `crates/cli/tests/rust_doctor.rs#sg_proc_02_rejects_dependencies_on_another_generated_crate_before_cargo_check`
- `crates/cli/tests/rust_doctor.rs#sg_proc_02_real_consumer_missing_resolver_returns_the_rustc_error_as_a_diagnostic`

## SG-PROC-03

- `crates/codegen/src/contract/write/input_tests.rs#cache_inside_sdk_rejects_cargo_check_and_preserves_the_old_tree`
- `crates/codegen/src/contract/write/tests.rs#sg_proc_03_real_cargo_checks_isolated_candidates_and_reuses_the_external_cache`
- `crates/codegen/src/filesystem/write_tests.rs#sg_proc_03_cache_allows_missing_targets_but_rejects_parent_links_and_files`
- `crates/codegen/src/filesystem/write_tests.rs#sg_proc_03_overlap_is_determined_by_both_the_existing_inode_and_the_absent_suffix`
- `crates/codegen/src/process/tests.rs#sg_proc_03_distinguishes_start_failures_from_pre_spawn_input_limits`
- `crates/codegen/tests/sibling_cli/doctor_failures.rs#doctor_rejects_a_cache_inside_the_sdk_before_compilation`

## SG-INFO-01

- `crates/codegen/src/generator_info/tests.rs#sg_info_01_round_trips_every_field_between_json_and_the_rust_model`
- `crates/core/src/generator/tests.rs#sg_info_01_clean_dirty_unknown_only`
- `crates/core/src/generator/tests.rs#sg_info_01_rejects_invalid_semver_rustc_banners_paths_and_free_text`
- `crates/core/src/generator/tests.rs#sg_info_01_bounds_all_fields_and_does_not_leak_values_in_errors`

## SG-INFO-04

- `crates/codegen/src/contract/check/tests.rs#sg_info_04_preserves_and_exactly_compares_different_host_profile_and_unknown_versions`
- `crates/cli/tests/rust_contract.rs#sg_info_04_real_cli_does_not_update_different_records_and_reports_notice_without_cargo`

## SG-INFO-05

- `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_renderer_byte_differences_even_when_inventory_hash_matches`
- `crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_nested_legacy_and_record_mismatch_without_input_values`
- `crates/codegen/src/generator_info/tests.rs#sg_info_05_nullable_including_all_keys_missing`
- `crates/codegen/src/generator_info/tests.rs#sg_info_05_unknown_duplicate_schema_disagreement_with_secret_denial`

## SG-VERIFY-01

- `scripts/check-generated-crates.py#def exact(`
- `scripts/sibling_repository.py#def verify(`
- `scripts/test-sibling-repository.py#test_current_surface_and_every_removed_path`

## SG-VERIFY-02

- `scripts/full_schema_corpus.py#def verify(`
- `scripts/test-full-schema-corpus.py#test_self_contained_full_schema`
- `tests/fixtures/full-schema/consumer/src/tests.rs#converts_external_unit_domain_types_from_full_projection_back_to_wire`
- `tests/fixtures/full-schema/consumer/src/tests.rs#full_sdl_arguments_cross_the_generated_adapter_with_domain_type_safety`

## SG-VERIFY-03

- `crates/core/tests/executable_contract.rs#public_api_can_select_custom_scalar_documents_for_validation`
- `crates/core/tests/executable_contract.rs#public_api_supports_separate_validation_selection_and_variable_coercion`
- `crates/core/tests/executable_contract.rs#public_contract_exposes_executable_diagnostic_codes_and_limits`
- `crates/operation/tests/audit_identity.rs#hit_identity`
- `crates/operation/tests/audit_identity.rs#miss_identity`
- `crates/operation/tests/audit_identity.rs#missing_telemetry`
- `crates/operation/tests/audit_identity.rs#parallel_identity`
- `crates/operation/tests/audit_identity.rs#provider_failure_identity`
- `crates/operation/tests/audit_identity.rs#redaction`
- `crates/operation/tests/audit_identity.rs#sampling_drop`
- `crates/operation/tests/audit_identity.rs#sink_failure_identity`
- `crates/testkit/tests/lifecycle_conformance.rs#life_01_to_08_axum_runs_the_common_endpoint_suite`
- `crates/testkit/tests/lifecycle_conformance.rs#life_01_to_08_direct_runs_through_a_common_endpoint_suite`
- `crates/testkit/tests/lifecycle_conformance.rs#life_01_to_08_hyper_runs_through_a_common_endpoint_suite`
- `crates/testkit/tests/lifecycle_conformance.rs#life_05_direct_execution_starts_with_the_request_plugin_not_the_http_phase`
- `crates/testkit/tests/lifecycle_conformance.rs#obs_04_full_observer_queue_does_not_change_the_response`
- `crates/testkit/tests/rate_conformance.rs#rate_02_stage_b_rejection_does_not_roll_back_stage_a_and_skips_resolver`
- `crates/testkit/tests/rate_conformance.rs#rate_04_timeout_drop_and_fail_open_closed`
- `crates/testkit/tests/rate_conformance.rs#production_structural_limit_applies_without_a_rate_08_limiter`
- `scripts/check-large-consumer-conformance.sh#large-consumer-suite.sh`
- `scripts/usability_dependencies.py#if packages["chigiri_core"]["dependencies"]:`
- `scripts/validation_plan.py#for operation in ("clippy", "test", "doc"):`

## SG-VERIFY-04

- `scripts/check-generated-crates.py#def quality(`
- `scripts/test-validation.py#test_completion_requires_all_commands_to_pass_against_unchanged_source`
- `scripts/test-validation.py#test_rejects_missing_duplicate_out_of_order_failed_skipped_and_cancelled_results`
- `scripts/validation_plan.py#def plan(`
- `scripts/test-dependency-policy.py#test_registered_only_and_runtime_remains_root`
- `scripts/test-dependency-policy.py#test_source_publish_dependency_and_graph_mutations`

## SG-VERIFY-05

- `scripts/test-cargo-consumer-gates.py#test_cancellation_stops_before_the_next_command`
- `scripts/test-cargo-consumer-gates.py#test_each_quality_command_failure_stops_the_gate_with_nonzero_status`
- `scripts/test-production-acceptance-trace.py#test_missing_duplicate_and_unrecorded_changes`
- `scripts/test-required-jobs.py#test_failure_skip_cancel_missing_and_empty_completion`
- `scripts/test-required-jobs.py#test_malformed_duplicate_and_unknown_jobs`
- `scripts/test-sibling-repository.py#test_current_surface_and_every_removed_path`

## SG-VERIFY-06

- `scripts/performance_evidence_regression.py#test_dirty_short_harness_evidence_cannot_satisfy_formal_load_or_soak`
- `scripts/test-release-fuzz-toolchain.sh#run-release-fuzz.sh`
- `scripts/validation_plan.py#check-production-performance-harness.sh`

## SG-LAYOUT-01

- `crates/codegen/src/contract/mod.rs#sg_layout_01_rendered_path_and_rust_over_400_lines_are_allowed`
- `crates/codegen/src/render/tests_layout.rs#sg_layout_01_renders_eighty_field_resolvers_in_one_public_trait`
- `crates/codegen/tests/sibling_cli/layout.rs#sg_layout_01_real_cli_generates_updates_and_checks_outputs_over_400_lines`
- `crates/codegen/tests/sibling_contract.rs#sg_layout_01_splits_large_query_mutation_input_and_enum_outputs_by_semantic_unit`
- `scripts/test-generated-ownership.py#test_generated_401_lines_are_exempt_and_counted`

## SG-LAYOUT-02

- `scripts/test-generated-ownership.py#test_arbitrary_marker_outside_registry_does_not_exempt`
- `scripts/test-generated-ownership.py#test_closed_marker_unknown_keys_and_invalid_fields_are_rejected`
- `scripts/test-generated-ownership.py#test_duplicate_json_and_oversize_marker_are_rejected`
- `scripts/test-generated-ownership.py#test_final_parent_manifest_symlink_and_fifo_are_rejected`
- `scripts/test-generated-ownership.py#test_handwritten_renderer_consumer_test_and_scaffold_are_not_exempt`
- `scripts/test-generated-ownership.py#test_missing_marker_and_file_and_changed_bytes_are_rejected`
- `scripts/test-generated-ownership.py#test_record_escape_duplicate_case_collision_and_utf8_are_rejected`
- `scripts/test-generated-ownership.py#test_registry_missing_unknown_manifest_and_overlapping_roots_fail`
- `scripts/test-generated-ownership.py#test_short_unknown_file_and_empty_directory_are_rejected`

## SG-LAYOUT-03

- `crates/codegen/src/render/tests_layout.rs#sg_layout_03_derives_roles_from_actual_operation_roots`
- `crates/codegen/src/render/tests_layout.rs#sg_layout_03_long_nested_types_keep_methods_together`
- `crates/cli/tests/rust_traits.rs#ri_07_large_generated_semantic_units_keep_resource_bounds`
- `crates/cli/tests/rust_consumer.rs#large_fixture_output_is_split_by_owner_and_role_within_resource_limits`

## SG-LAYOUT-04

- `crates/codegen/src/render/tests_layout.rs#sg_layout_04_unrelated_growth_does_not_change_existing_model_and_trait_paths_or_bytes`
- `crates/codegen/tests/sibling_cli/layout.rs#sg_layout_01_real_cli_generates_updates_and_checks_outputs_over_400_lines`

## SG-LAYOUT-05

- `crates/codegen/src/writer/layout.rs#semantic_paths_reject_traversal_case_and_file_directory_collisions`
- `crates/codegen/tests/sibling_cli/layout.rs#sg_layout_05_rejects_normalized_directive_path_collisions_and_preserves_the_old_tree`
- `crates/codegen/tests/sibling_contract.rs#sg_crate_04_keyword_and_metadata_name_and_stdlib_shadow_standalone`

## SG-LAYOUT-06

- `crates/codegen/src/writer/layout.rs#sg_layout_06_rejects_total_byte_overflow_before_adding_a_file`
- `crates/codegen/tests/sibling_cli/layout.rs#sg_layout_06_update_over_output_estimate_limit_preserves_the_old_tree`
- `scripts/test-generated-ownership.py#test_actual_byte_limit_is_rejected_without_reading_a_sparse_file`
- `scripts/test-generated-ownership.py#test_directory_swap_and_entry_budget_fail_closed`

## SG-LAYOUT-07

- `crates/codegen/tests/sibling_contract.rs#sg_layout_07_semantic_formatting_preserves_public_contract_surfaces`
- `crates/codegen/tests/sibling_contract.rs#sg_crate_06_manifest_comment_changes_provenance_while_preserving_semantic_binding`
- `crates/cli/tests/rust_consumer.rs#res_01_abi8_preserves_typestate_structural_stability`
- `scripts/full_schema_corpus.py#schemaDigest`

## SG-LAYOUT-08

- `crates/codegen/src/contract/check/tests.rs#sg_fs_06_reports_extra_missing_and_changed_files_without_modifying_bytes`
- `crates/codegen/tests/sibling_cli/layout.rs#sg_layout_01_real_cli_generates_updates_and_checks_outputs_over_400_lines`
- `crates/codegen/tests/sibling_cli/ownership.rs#sg_fs_03_schema_shrink_removes_unused_ownership_shards_and_restore_artifacts`
- `scripts/check-generated-crates.py#def exact(`
