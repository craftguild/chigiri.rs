# Frozen production readiness acceptance trace

This table is the 53-row acceptance trace from the historical production readiness
review, copied byte-for-byte when the Markdown review ledger was retired.
Trace checkers compare the current trace in `docs/spec-conformance.md`
with it. Do not edit it; the complete review record is Beads issue
`lab01-c43.8`.

| ID | Test / review evidence | Current result |
| --- | --- | --- |
| DX-01 | The books end-to-end test reaches Query, Mutation, error, and media handling from checked-in SDL; `scripts/check-books-quick-start.sh` verifies the documented workflow. | Automated pass |
| DX-02 | The generated-public-contract fixture compiles when complete and fails to compile when required resolvers are missing. | Automated pass |
| DX-03 | The consumer typestate fixture proves generated resolver completeness. This historical program used the ABI current at the time; only its trace name was migrated on 2026-09-07. | Automated pass; historical-program qualification |
| DX-04 | `pythia_testkit::developer_contract::same_endpoint_runs_direct_hyper_axum_without_server_api` | Automated pass |
| DX-05 | Codegen tests verify that generated server contracts have no client-artifact dependency. | Automated pass |
| DX-06 | `pythia_testkit::developer_contract::public_api_has_no_product_types` | Automated pass |
| LIFE-01 | The direct, Hyper, and Axum lifecycle-conformance suites run the same LIFE-01 through LIFE-08 scenarios. | Automated pass |
| LIFE-02 | The shared lifecycle suites verify rejection before body polling and the fixed phase order. | Automated pass |
| LIFE-03 | The shared lifecycle suites verify direct dropping of the timed-out phase future or stream. | Automated pass |
| LIFE-04 | The shared suites plus `bridges_expose_no_listener_route_or_shutdown` and eight compile-negative fixtures prove application ownership of server lifecycle. | Automated pass |
| LIFE-05 | The direct-execution case begins at the Request plugin and has no HTTP phase. | Automated pass |
| LIFE-06 | The HTTP production-public-contract test proves object-safe transport SPI without a body model in the request head. | Automated pass |
| LIFE-07 | The shared lifecycle suites verify consistent rejection, timeout, and cancellation behavior across all three paths. | Automated pass |
| LIFE-08 | The shared lifecycle suites verify response-hook and terminal behavior across all three paths. | Automated pass |
| OP-01 | `pythia_operation::identity::op_01_exact_document_bytes` verifies the stable digest of the exact document bytes. | Automated pass |
| OP-02 | Identity tests separate selected operation names from document keys; runtime and Axum regressions turn an over-limit name into a bounded validation error without panic. | Automated pass |
| OP-03 | Catalog tests reject duplicate, invalid, and wrong-schema entries and support digest lookup for opaque aliases. | Automated pass |
| OP-04 | Persisted-operation tests reject an APQ hash mismatch before any store write. | Automated pass |
| OP-05 | APQ-unknown and Safelist-unknown produce distinct outcomes; Safelist compares opaque aliases and raw-document digests. | Automated pass |
| OP-06 | IdOnly mode rejects supplied text and resolves a known ID to the exact document. | Automated pass |
| OP-07 | Provider-timeout tests drop the future and apply the configured failure policy. | Automated pass |
| OP-08 | Debug and public errors reveal no raw query, persisted ID, or provider source; Audit mode records bounded hit, miss, digest, revision, and sink-failure events. | Automated pass |
| OP-09 | `pythia_testkit::developer_contract::endpoint_runs_without_client_tooling` | Automated pass |
| BUDGET-01 | Cost tests independently enforce structural and operation budgets, coordinate overrides, bounded integer multipliers, and literal/variable list multipliers before resolver start. | Automated pass. Security review pass. |
| BUDGET-02 | Cost tests reject checked overflow, including alias/fragment/variable expansion, schema defaults, and nested cumulative list ratios, before execution. | Automated pass. Security review pass. |
| RATE-01 | Stage A rejects before body polling with 429 and a consistent `Retry-After`. | Automated pass |
| RATE-02 | `pythia_testkit::rate_conformance::rate_02_stage_b_refuses_to_roll_back_stage_a_with_resolver_zero` | Automated pass |
| RATE-03 | Concurrent token consumption never exceeds capacity; impossible over-capacity costs are rejected; provider conformance rejects non-atomic implementations. | Automated pass |
| RATE-04 | Rate-conformance tests explicitly cover timeout drop plus fail-open/fail-closed, and Stage B drops only the provider future before applying failure policy. | Automated pass |
| RATE-05 | Stage A returns a `Retry-After` consistent with the post-consumption snapshot. | Automated pass |
| RATE-06 | Transport and `Debug` output reveal neither tokens nor quota values. | Automated pass |
| RATE-07 | Key count and expiry remain bounded; capacity exhaustion and policy mismatch return explicit rejection without changing existing bucket state. | Automated pass. Security review pass. |
| RATE-08 | Rate-conformance test verifies production structural limits remain active without a limiter. | Automated pass |
| OBS-01 | Lifecycle, endpoint, and runtime tests close every success, rejection, timeout, cancellation, and dropped-future phase in fixed order while fixing request ID and sampling once at request start. | Automated pass |
| OBS-02 | Runtime tests mask independent failure sources and notify observers without an ID when request-ID or error-ID factories fail; direct output-limit failures use the same correlated diagnostic path. | Automated pass. Security review pass. |
| OBS-03 | Direct and HTTP output-limit tests correlate client `errorId` with observer source while masking internal source and preserving paths. | Automated pass. Security review pass. |
| OBS-04 | Observer failures and a full observer queue do not change GraphQL semantics or responses. | Automated pass |
| OBS-05 | Default debug, HTTP response/source debug, Axum telemetry, and OpenTelemetry tests exclude queries, variables, headers, PII, source, and identity values while retaining the request/operation/execution/provider hierarchy. | Automated pass. Security review pass. |
| OBS-06 | Metric labels use a closed key set and a 64-byte value limit. | Automated pass |
| OBS-07 | Queue tests prove non-blocking full behavior, drop counts, FIFO ordering, both disconnect directions, and bounded parallel-producer accounting. | Automated pass |
| OBS-08 | `scripts/check-observability-contract.sh` compiles the `observability-opt-in-consumer` dependency fixture. | Automated pass |
| OBS-09 | Field-success events appear only when field instrumentation is explicitly enabled. | Automated pass |
| GOV-01 | Schema-governance tests prove semantic-equivalence diffs are empty, API changes are classified, and nested nullability uses input/output variance. | Automated pass |
| GOV-02 | Governance conformance revalidates every operation by exact ID and reports result-shape and cost deltas. | Automated pass |
| GOV-03 | Unknown schema changes fail closed, while the schema CLI distinguishes policy failure exit 1 from tool failure exit 2. | Automated pass |
| GOV-04 | `pythia_testkit::governance_conformance::runtime_and_readiness_metadata_match` | Automated pass |
| TEST-01 | Direct, Hyper, and Axum lifecycle suites plus `scripts/check-neutral-endpoint-contract.sh` exercise the same endpoint contract. | Automated pass |
| TEST-02 | `pythia_testkit::provider_conformance` covers third-party operation and rate providers, direct dropping of timed-out futures, clock reversal and expiry contracts, rejection of malformed provider metadata, and atomic rejection under concurrent access. | Automated pass |
| TEST-03 | Differential conformance covers the strict allowlist negative suite and the Axum synthetic corpus with zero unallowlisted structural differences. | Automated pass |
| TEST-04 | `scripts/check-fuzz-regressions.sh` runs six committed corpora. | Reviewer pass; manual rerun on the final candidate remained TODO. |
| REL-01 | `scripts/check-package-fresh-consumer.sh` verified MSRV/stable formatting, Clippy, tests, and rustdoc. | Automated pass; rerun required for each final commit. |
| REL-02 | `scripts/check-release-artifacts.sh` verified isolated-HEAD manifests and lockfiles, SBOM graph, provenance, checksums, and exact sources; verifier regressions covered manifest, lockfile, SBOM, provenance, and link/device tar rejection. | Tar-focused regression and security review passed; current-HEAD reproduction remained TODO. |
| REL-03 | Section 5 records three independent review tracks and their rereviews. | All three tracks passed; unresolved Critical / High / Medium / Low findings: 0. |
