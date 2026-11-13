"""Verify the current production trace against its historical review record."""

import re
from pathlib import Path

from historical_trace import load_trace

COUNTS = {"DX": 6, "LIFE": 8, "OP": 9, "BUDGET": 2, "RATE": 8,
          "OBS": 9, "GOV": 4, "TEST": 4, "REL": 3}
HISTORICAL_EXPECTED = {f"{prefix}-{number:02d}" for prefix, count in COUNTS.items()
                       for number in range(1, count + 1)}
RETIRED = {"GOV-01", "GOV-03"}
EXPECTED = HISTORICAL_EXPECTED - RETIRED
# Record every approved evidence migration explicitly so historical review text remains immutable.
MIGRATIONS = {
    'DX-01': (
        'The books end-to-end test reaches Query, Mutation, error, and media handling from checked-in SDL; `scripts/check-books-quick-start.sh` verifies the documented workflow.',
        'The books end-to-end suite and `scripts/check-books-quick-start.sh` cover checked-in schema, query, mutation, errors, and media behavior.',
    ),
    'DX-02': (
        'The generated-public-contract fixture compiles when complete and fails to compile when required resolvers are missing.',
        'Codegen public-contract fixtures compile complete generated implementations and reject missing implementations.',
    ),
    'DX-03': (
        'The consumer typestate fixture proves generated resolver completeness. This historical program used the ABI current at the time; only its trace name was migrated on 2026-09-07.',
        'The ABI 6 vNext consumer verifies resolver typestate.',
    ),
    'DX-04': (
        '`pythia_testkit::developer_contract::same_endpoint_runs_direct_hyper_axum_without_server_api`',
        '`chigiri_testkit::developer_contract::same_endpoint_runs_direct_hyper_axum_without_server_api`',
    ),
    'DX-05': (
        'Codegen tests verify that generated server contracts have no client-artifact dependency.',
        'Server code generation excludes client artifacts.',
    ),
    'DX-06': (
        '`pythia_testkit::developer_contract::public_api_has_no_product_types`',
        '`chigiri_testkit::developer_contract::public_api_has_no_product_types`',
    ),
    'GOV-04': (
        '`pythia_testkit::governance_conformance::runtime_and_readiness_metadata_match`',
        '`chigiri_testkit::governance_conformance::runtime_and_readiness_metadata_match`',
    ),
    'LIFE-01': (
        'The direct, Hyper, and Axum lifecycle-conformance suites run the same LIFE-01 through LIFE-08 scenarios.',
        'Common direct, Hyper, and Axum lifecycle suites verify request start.',
    ),
    'LIFE-02': (
        'The shared lifecycle suites verify rejection before body polling and the fixed phase order.',
        'The same suites verify ordered phase entry and exit.',
    ),
    'LIFE-03': (
        'The shared lifecycle suites verify direct dropping of the timed-out phase future or stream.',
        'The same suites verify rejection and failure results.',
    ),
    'LIFE-04': (
        'The shared suites plus `bridges_expose_no_listener_route_or_shutdown` and eight compile-negative fixtures prove application ownership of server lifecycle.',
        'Common suites and `chigiri_testkit::public_contract::bridges_expose_no_listener_route_or_shutdown` verify the bridge boundary, with eight compile-negative fixtures.',
    ),
    'LIFE-05': (
        'The direct-execution case begins at the Request plugin and has no HTTP phase.',
        'Direct execution starts with the request plugin rather than an HTTP phase.',
    ),
    'LIFE-06': (
        'The HTTP production-public-contract test proves object-safe transport SPI without a body model in the request head.',
        'The HTTP production transport contract remains object safe and independent of product body models.',
    ),
    'LIFE-07': (
        'The shared lifecycle suites verify consistent rejection, timeout, and cancellation behavior across all three paths.',
        'Common direct, Hyper, and Axum suites verify cancellation.',
    ),
    'LIFE-08': (
        'The shared lifecycle suites verify response-hook and terminal behavior across all three paths.',
        'Common direct, Hyper, and Axum suites verify completion and close behavior.',
    ),
    'OBS-01': (
        'Lifecycle, endpoint, and runtime tests close every success, rejection, timeout, cancellation, and dropped-future phase in fixed order while fixing request ID and sampling once at request start.',
        'Lifecycle, endpoint, and runtime tests close every phase in fixed order for success, rejection, timeout, execution destruction, and cancellation.',
    ),
    'OBS-02': (
        'Runtime tests mask independent failure sources and notify observers without an ID when request-ID or error-ID factories fail; direct output-limit failures use the same correlated diagnostic path.',
        'Request/error ID factory failures notify observers without inventing IDs and preserve response semantics.',
    ),
    'OBS-03': (
        'Direct and HTTP output-limit tests correlate client `errorId` with observer source while masking internal source and preserving paths.',
        'Runtime and HTTP output-diagnostic tests correlate client `errorId` and observer records while masking sources for independent failures and limit errors.',
    ),
    'OBS-04': (
        'Observer failures and a full observer queue do not change GraphQL semantics or responses.',
        'Observer failures and full queues do not change GraphQL response semantics.',
    ),
    'OBS-05': (
        'Default debug, HTTP response/source debug, Axum telemetry, and OpenTelemetry tests exclude queries, variables, headers, PII, source, and identity values while retaining the request/operation/execution/provider hierarchy.',
        'Default debug, HTTP encoding, lifecycle, and OpenTelemetry tests exclude queries, variables, headers, IP/PII, sources, secrets, and identity values while preserving span relationships and error recording.',
    ),
    'OBS-06': (
        'Metric labels use a closed key set and a 64-byte value limit.',
        'Metric labels use a closed key set with at most eight labels and a 64-byte value limit.',
    ),
    'OBS-07': (
        'Queue tests prove non-blocking full behavior, drop counts, FIFO ordering, both disconnect directions, and bounded parallel-producer accounting.',
        'Queue tests cover nonblocking overflow/drop counts, FIFO behavior, disconnects, and concurrent producers delivering up to capacity while preserving cause-specific counts and FIFO order.',
    ),
    'OBS-08': (
        '`scripts/check-observability-contract.sh` compiles the `observability-opt-in-consumer` dependency fixture.',
        '`scripts/check-observability-contract.sh` verifies the `observability-opt-in-consumer` dependency fixture.',
    ),
    'OBS-09': (
        'Field-success events appear only when field instrumentation is explicitly enabled.',
        'Field instrumentation emits success events only when explicitly enabled.',
    ),
    'OP-01': (
        '`pythia_operation::identity::op_01_exact_document_bytes` verifies the stable digest of the exact document bytes.',
        '`chigiri_operation::identity::op_01_calculates_a_stable_digest_from_exact_document_bytes`',
    ),
    'OP-02': (
        'Identity tests separate selected operation names from document keys; runtime and Axum regressions turn an over-limit name into a bounded validation error without panic.',
        'Operation identity, runtime limits, and Axum tests require a selection name for multi-operation documents and return a bounded validation error for oversized names.',
    ),
    'OP-03': (
        'Catalog tests reject duplicate, invalid, and wrong-schema entries and support digest lookup for opaque aliases.',
        'The catalog rejects duplicate IDs, invalid documents, unknown schemas, and schema mismatches.',
    ),
    'OP-04': (
        'Persisted-operation tests reject an APQ hash mismatch before any store write.',
        'APQ hash mismatch is rejected before store writes.',
    ),
    'OP-05': (
        'APQ-unknown and Safelist-unknown produce distinct outcomes; Safelist compares opaque aliases and raw-document digests.',
        'Persisted-operation tests distinguish APQ unknown from safelist unknown and compare opaque aliases with raw-document digests.',
    ),
    'OP-06': (
        'IdOnly mode rejects supplied text and resolves a known ID to the exact document.',
        'ID-only requests reject text and resolve known IDs to the exact stored document.',
    ),
    'OP-07': (
        'Provider-timeout tests drop the future and apply the configured failure policy.',
        'Provider timeouts drop the future and preserve the configured failure policy.',
    ),
    'OP-08': (
        'Debug and public errors reveal no raw query, persisted ID, or provider source; Audit mode records bounded hit, miss, digest, revision, and sink-failure events.',
        'Debug/public errors omit raw queries, IDs, and provider sources; audit records preserve digest, revision, hit/miss, and sink failures.',
    ),
    'OP-09': (
        '`pythia_testkit::developer_contract::endpoint_runs_without_client_tooling`',
        '`chigiri_testkit::developer_contract::endpoint_runs_without_client_tooling`',
    ),
    'RATE-01': (
        'Stage A rejects before body polling with 429 and a consistent `Retry-After`.',
        'Stage A returns 429 and aligned `Retry-After` before reading the body.',
    ),
    'RATE-02': (
        '`pythia_testkit::rate_conformance::rate_02_stage_b_refuses_to_roll_back_stage_a_with_resolver_zero`',
        'Stage B rejection does not roll back Stage A when no resolver starts.',
    ),
    'RATE-03': (
        'Concurrent token consumption never exceeds capacity; impossible over-capacity costs are rejected; provider conformance rejects non-atomic implementations.',
        'An over-capacity cost is rejected at construction rather than producing an impossible retry, and concurrent consumption is atomic.',
    ),
    'RATE-04': (
        'Rate-conformance tests explicitly cover timeout drop plus fail-open/fail-closed, and Stage B drops only the provider future before applying failure policy.',
        '`chigiri_testkit::rate_conformance::rate_04_timeout_drop_and_fail_open_closed` and `crates/testkit/tests/rate_conformance/stage_b.rs#rate_04_stage_b_timeout_drops_the_provider_and_applies_the_failure_policy` verify future cancellation and fail-open/fail-closed policy.',
    ),
    'RATE-05': (
        'Stage A returns a `Retry-After` consistent with the post-consumption snapshot.',
        'Stage A response tests preserve 429 body ordering and `Retry-After`.',
    ),
    'RATE-06': (
        'Transport and `Debug` output reveal neither tokens nor quota values.',
        'Transport debug output omits tokens and quota values.',
    ),
    'RATE-07': (
        'Key count and expiry remain bounded; capacity exhaustion and policy mismatch return explicit rejection without changing existing bucket state.',
        'Capacity and expiry are bounded; capacity exhaustion is an explicit rejection rather than a provider failure; and a same-key policy mismatch is rejected without mutating the bucket.',
    ),
    'RATE-08': (
        'Rate-conformance test verifies production structural limits remain active without a limiter.',
        'Production structural limits remain active when no limiter is configured.',
    ),
    'REL-01': (
        '`scripts/check-package-fresh-consumer.sh` verified MSRV/stable formatting, Clippy, tests, and rustdoc.',
        '`scripts/check-cargo-consumer.sh#consumer-quality-five-commands` verifies repository consumers with MSRV/stable fmt, check, test, Clippy, and rustdoc.',
    ),
    'REL-02': (
        '`scripts/check-release-artifacts.sh` verified isolated-HEAD manifests and lockfiles, SBOM graph, provenance, checksums, and exact sources; verifier regressions covered manifest, lockfile, SBOM, provenance, and link/device tar rejection.',
        '`crates/codegen/src/contract/check/tests.rs#sg_info_05_rejects_nested_legacy_and_record_mismatch_without_input_values` verifies current marker, ABI, and provenance consistency; ADR-0027 removed distribution-only archive and SBOM checks.',
    ),
    'REL-03': (
        'Section 5 records three independent review tracks and their rereviews.',
        'The three-review finding/re-review table in section 5 of the production review record.',
    ),
    'TEST-01': (
        'Direct, Hyper, and Axum lifecycle suites plus `scripts/check-neutral-endpoint-contract.sh` exercise the same endpoint contract.',
        'Common endpoint lifecycle suites and `scripts/check-neutral-endpoint-contract.sh` verify transport neutrality.',
    ),
    'TEST-02': (
        '`pythia_testkit::provider_conformance` covers third-party operation and rate providers, direct dropping of timed-out futures, clock reversal and expiry contracts, rejection of malformed provider metadata, and atomic rejection under concurrent access.',
        'Third-party operation/rate provider suites cover public contracts, timeout cancellation, clock reversal/expiry, malformed metadata rejection, and atomic concurrent behavior.',
    ),
    'TEST-03': (
        'Differential conformance covers the strict allowlist negative suite and the Axum synthetic corpus with zero unallowlisted structural differences.',
        'The strict differential allowlist suite and Axum synthetic corpus require zero unapproved structural differences.',
    ),
}


def rows(document, expected, label):
    found = re.findall(
        r"^\|\s*((?:DX|LIFE|OP|BUDGET|RATE|OBS|GOV|TEST|REL)-[0-9]{2})\s*\|\s*([^|]+?)\s*\|",
        document,
        re.MULTILINE,
    )
    if len(found) != len(expected) or {row for row, _ in found} != expected:
        raise ValueError(f"The {label} production trace does not contain its exact IDs.")
    return {row: evidence.strip() for row, evidence in found}


def validate(repository, spec, history):
    current = rows(spec, EXPECTED, "current")
    previous = rows(history, HISTORICAL_EXPECTED, "historical")
    for row in sorted(EXPECTED):
        if row not in MIGRATIONS:
            if current[row] != previous[row]:
                raise ValueError(f"Production trace changed outside the approved migration set: {row}")
            continue
        if (previous[row], current[row]) != MIGRATIONS[row]:
            raise ValueError(f"Migrated trace does not match the recorded mapping: {row}")
        references = [
            reference
            for reference in re.findall(r"`([^`]+)`", current[row])
            if "#" in reference
        ]
        if not references:
            continue
        if len(references) != 1:
            raise ValueError(f"The migration requires one exact path marker: {row}")
        name, separator, marker = references[0].partition("#")
        path = Path(name)
        if not separator or not marker or path.is_absolute() or ".." in path.parts:
            raise ValueError(f"Migrated path/marker is invalid: {row}")
        target = repository / path
        if not target.is_file() or marker not in target.read_text(encoding="utf-8"):
            raise ValueError(f"Migrated evidence target or marker is missing: {row}")


def main():
    repository = Path(__file__).resolve().parent.parent
    validate(repository, (repository / "docs/spec-conformance.md").read_text(),
             load_trace(repository, "production-readiness"))
    print("Production acceptance trace passed: 51 active IDs and 47 explicit evidence migrations.")


if __name__ == "__main__":
    main()
