# Chigiri v1 Implementation Handoff

Current compiler policy: the [ADR-0059 amendment](adr/0059-craftguild-standards-adoption.md#rust-195-unification-amendment-2026-11-07)
requires Rust 1.95.0 for all first-party builds, SDK generation, local checks
and CI, with declared minimum Rust 1.95. Earlier 1.85/Mock-only/stable lane
statements below retain historical scope and do not override this policy.

Historical execution records and implementation prompts are retained privately.
Public ADRs, this document and explicit test fixtures define the current
contract; historical Passed statements do not establish a current gate result.
See the [documentation index](README.md) and run the current validation gates.

This document divides implementation work among agents and engineers.
[`architecture.md`](architecture.md) and the ADRs are the normative sources for
design decisions, [`spec-conformance.md`](spec-conformance.md) defines the
applicable specification coverage, and
[the implementation rules](implementation-rules.md) define the required working
method. If implementation requires a different design decision, update or add
an ADR instead of changing the contract only in code.

For new review rounds, findings, corrections, and independent verification, use
Beads under the [review workflow](review-workflow.md). This supersedes historical
instructions in this document to append reviews to the former Markdown ledger.
Existing review records and their finding IDs remain historical evidence.

The ADR-0026 through ADR-0028 program in Sections 15 and 16, and the independent
ADR-0029 experiment in Section 17, are implemented. Earlier work-package and
Passed records are retained as history; they are not independent current test
evidence. Do not apply the former generated ABI, CLI, module-output, or
publisher completion conditions to the replacement program. The mock
generalization in Section 18/ADR-0030 (Accepted / Implemented) replaces only
Section 17's fixed-fixture limitation.

Run `./scripts/check-implementation-rules.sh` before starting work, immediately
after each subtask, after `cargo fmt`, and immediately before handoff.
ADR-0059 applies the shared implementation-only tokei Code contract. Tests,
test helpers and fixtures are excluded by role; generated ownership remains required. Canonical generated output follows
ADR-0028's semantic-unit layout. Under ADR-0043, all added or changed
first-party documentation, comments, rustdoc, diagnostics, examples, and
generator-owned prose must be English.
The old checker's generated-file line-limit rejection does not extend the
current generated-output contract. It also does not exempt overwrite safety or
any other quality failure.

## 1. Work packages and dependency order

### WP-0: Workspace/quality gates

Deliverables:

- Edition 2024 / MSRV 1.85 workspace
- the six-crate workspace and books example defined by the original
  architecture baseline
- `unsafe` prohibition, fmt/clippy/test/MSRV CI
- no external normal dependency for the core
- the historical workspace-local generator Cargo alias, since removed by
  ADR-0048
- an implementation-rules checker that also runs at the start of CI
- the ADR-0043 English-language checklist for first-party prose and rustdoc

Completion conditions:

- Each initial crate builds with MSRV and current stable Rust.
- `cargo tree -p chigiri_core --edges normal` has no external crate.
- Tests cover the checker's success path, its line-limit failure exit status,
  and exit status 2 for checker configuration errors.

### WP-1: Source, Value, lexer/parser

Dependence: WP-0.

Deliverables:

- `SourceMap`, `Span`, stable diagnostics
- independently ordered `Object`, `Value`, and `Id` types
- common lexer
- SDL AST/parser
- executable AST/parser
- parser limits

Completion conditions:

- Tests cover Section 1 of the conformance matrix.
- Invalid or untrusted input does not cause a panic.
- Multi-source diagnostics report the correct path, line, and column.

### WP-2: Semantic schema/composition/type-system validation

Dependence: WP-1.

Deliverables:

- semantic schema lookup model
- definitions/extensions composition
- built-in scalars/directives
- Section 3 validation passes
- introspection descriptor metadata

Completion conditions:

- WP-2 adds positive and negative tests for every Section 2 conformance rule
  owned by schema composition or type-system validation, so every Required rule
  is eventually traceable to a test.
- WP-3 owns runtime scalar, list, and OneOf coercion. WP-5 owns `@skip`,
  `@include`, and ordered-result execution semantics. Gate E completes the
  remaining Required coverage in Section 2.
- Invalid SDL never exposes a partial schema through the public API; it returns
  diagnostics only.

### WP-3: Executable validation/coercion

Dependence: WP-2.

Deliverables:

- Section 5 validation passes
- operation selection
- raw variable coercion
- argument/input/default/OneOf/custom scalar coercion SPI
- complexity/limit summary

Completion conditions:

- The applicable conformance-matrix entries are complete.
- Validation is separable from variable coercion, and validated documents can
  be reused.

### WP-4: Codegen/public resolver contract

Historical baseline: ADR-0028/Section 16 supersedes the generated-file line and
sharding conditions below. The replacement implementation uses semantic units,
retains the 400-line limit for handwritten code, and preserves product quality
and resource testing.

Dependence: WP-2. It can be parallel to WP-3.

Deliverables:

- schema directory discovery/composition
- canonical Rust renderer
- stable generated re-exports from the output directory and `mod.rs`
- generated `SchemaTypes`, inputs/enums/OneOf/interface/union wrappers
- generated resolver traits and type-erased dispatch adapters
- `Resolvers::new`
- a static schema descriptor and ABI guard
- `generate`/`check`/process-level transactional output

Historical completion conditions (superseded where noted above):

- Golden generation is deterministic.
- Under the former rule, formatting triggered sharding at 350 lines and every
  generated `.rs` file remained below 400 physical lines.
- Under the former rule, one indivisible item between 350 and 400 lines received
  the required header in a dedicated shard.
- Missing-field and missing-group compile-fail fixtures fail as expected.
- Invalid SDL leaves existing output unchanged.

### WP-5: Executor/introspection

Dependence: WP-3. Generated dispatch interface matches the first type contract with WP-4.

Deliverables:

- field collection
- normal bounded future poll set
- serial Mutation root
- resolver dispatch
- result coercion/value completion/null propagation
- response/errors
- introspection execution

Completion conditions:

- Conformance matrix Section 5 and concurrency tests are filled.
- The core has no async-runtime dependency and spawns no tasks.

### WP-6: Middleware/cache/runtime builder

Dependence: WP-5.

Deliverables:

- `ExecutableSchema<C>` / builder
- Request/Operation/Field plugin chains
- context types and consuming `Next` tokens
- document cache
- error/introspection/limits policies

Completion conditions:

- Tests cover onion ordering, short-circuiting, error/value transformation, and
  cache eviction.
- The field plugin runs under concurrent execution.

### WP-7: HTTP codec

Dependence: WP-1's Value/Request/Response contract. It can be parallel to WP-5.

Deliverables:

- serde_json <-> core Value conversion
- POST/GET decode
- percent/form query decode
- Accept/Content-Type/status mapping
- body-independent HTTP request/response parts

Completion conditions:

- Codec-level tests cover Section 7 of the conformance matrix.
- There is no Hyper/Axum type in the codec public API.

### WP-8: Legacy Tokio/Hyper reference server and books example

Dependency: WP-4, WP-6, WP-7

The results:

- HTTP/1.1 accept/connection service
- body limit, context factory, context/execution timeout, shutdown/drain
- books SDL/domain/resolvers/plugin/server
- quick-start README and curl examples

Conditions of completion:

- The ephemeral port integration test is going through Query/Mutation/error/media cases.
- The generated output directory in example is compatible with `chigiri rust check` and also complies with the repository checker.

This WP is historical v1 implementation work. The 2026-08-27 revision of ADR-0004 superseded the contract in which Chigiri owned listening, accepting, and shutdown. The current production target is a handler-only Endpoint and bridge; WP-P2 performed that migration.

## 2. Public-contract-first checkpoints

The following is agreed upon in the compile-only skeleton before the implementation details.

1. `Value`, `GraphQlRequest`, `GraphQlResponse`, `GraphQlError`, `FieldError`
2. `SchemaTypes`, generated input/output mapping
3. Resolver trait RPITIT signature and application `async fn` implementation
4. the owned `ResolvedObject`/`ResolvedValue` bridge, resolver future, and borrow lifetime
5. `Plugin<C>` and the three consuming `Next` types
6. `ExecutableSchema<C>::execute`
7. HTTP context factory, neutral `chigiri::Endpoint`, body/deadline SPI, opt-in bridge

For checkpoint 4, first build a compile-time and runtime spike covering owned-object downcast, resolver-future borrow lifetimes, retained list-item errors, and the complete Query → object → nested field → list/interface path. Move on to the executor body only after that path type-checks.

## 3. Test organization

Tests use normative contracts and explicit version-controlled fixtures, not
review records or approval states. They must run without Beads or the former
Markdown ledger. Historical baselines cannot override later ADRs or establish
current passing results; see the [review workflow](review-workflow.md#boundary-with-automated-tests).

- Unit test: the same crate/module for each algorithm module.
- spec fixture: `crates/core/tests/spec/<section>/`
- parser/diagnostic golden: `crates/core/tests/golden/`
- codegen golden: `crates/codegen/tests/golden/`
- compile pass/fail fixture: `crates/codegen/tests/compile/`
- HTTP codec: `crates/http/tests/`
- endpoint/bridge integration: `crates/http/tests/`, `crates/hyper/tests/`, `crates/axum/tests/`
- end-to-end example assertions: `examples/books/tests/`

ADR-0059 measures implementation with tokei Code and excludes test modules and
test-only shell fixtures. Keep tests organized by specification section, rule or
scenario; do not compress statements or delete necessary explanatory comments.

The compile-fail test runs `cargo check` in a subprocess with an isolated `CARGO_TARGET_DIR`, points it at a fixed crate without adding dependencies, and verifies the exit status and stable diagnostic fragments.

## 4. Review gates

### Gate A: Language/schema

WP-1/2 is complete. Valid SDL composes into a semantic schema, and invalid SDL
is rejected with source diagnostics. The checker and the language review
required by the rules in force at that historical gate both passed. ADR-0043
now requires English first-party prose.

### Gate B: Generated contract

Complete compile spike of WP-4. Domain-related type, signature of Query/Mutation/object/list/interface/union/custom scalar is validated. The entire generated output meets 400 line restrictions.

### Gate C: Core execution

WP-3/5/6 is complete. Direct `ExecutableSchema::execute` is going through GraphQL compliance tests and middleware tests.

### Gate D: HTTP endpoint vertical slice

WP-7 and WP-8 are complete. The books application exercises HTTP behavior,
plugins, production limits, and fixed error masking. The reference-server
ownership used by this historical v1 gate does not carry forward into the
production gate after ADR-0004's revision.

### Gate E: v1 readiness

- All the Required in the conformance matrix are test links.
- MSRV/current stable CI green.
- generated drift check green.
- implementation-rules checker green.
- The language review required by that historical gate completed; ADR-0043 now
  requires English for new or changed comments and rustdoc.
- core dependency check green.
- Reproduce CI and the documented quick start from a fresh clone of the Git HEAD commit.

## 5. Non-negotiable constraints

- Do not introduce an external GraphQL parser or executor merely to shorten the
  implementation.
- Do not expose async-runtime, HTTP, or Serde types through the core.
- Do not defer missing-resolver detection to startup validation.
- Do not collapse omitted and explicit null into one `Option` state.
- Do not execute all Query fields serially.
- Do not execute Mutation root fields concurrently.
- Do not partially update generated output for invalid SDL.
- Do not enable introspection or detailed errors by guessing from environment
  variables.
- Keep implementation at or below 400 tokei Code lines under ADR-0059, excluding
  tests and test-only fixtures without minifying or deleting needed comments.
  Canonical generated output follows ADR-0028 and verified shared ownership.
- Except for the explicit ADR-0043 exclusions, write every added or changed
  comment and rustdoc item in English.

## 6. Implemented vNext work packages

This is the work that was completed following WP-0 to WP-8 in v1 and accepted/implemented in ADR-0007.
The design judgment, public API, diagnostic, timeout, acceptance oracle is the normative source of ADR-0007.

### WP-V0: Fixture and consumer gate

Dependence: v1 Gate E.

- Add a synthetic consumer fixture with 70 objects, 750 fields, 19 inputs, 65 enums, and five custom scalars.
- Fix the public re-export surface.
- Add `scripts/check-vnext-adoption.sh` and run the consumer's check, Clippy, and rustdoc gates on MSRV and stable.
- After completion, we record the size conditions for ADR-0007 `RES-01` and the Section 4 command as green.

### WP-V1: Built-in SDL import

Dependence: WP-V0.

- Apply the ADR-0007 Section 5 comparison rules to the five built-in directives in the September 2025 GraphQL specification.
- Implement `SDL-01` through `SDL-05` while preserving existing `SF2004` diagnostics and source spans.
- Do not hand-maintain a synchronized async-graphql snapshot; fixtures must prove that `generate` and `check` handle the definitions.

### WP-V2: Resolver typestate and ABI 3

Dependence: WP-V0.

- Derive a shared erased group, constant-index linear stage, and 16-object sharding from the public-contract-first compile fixture.
- The implementation uses no `unsafe`, `Any` downcast, runtime string map, or
  startup-time fallback for missing resolvers.
- Move to ABI 3 in one step and update all golden files, books examples, and checked-in generated source in the same subtask.
- Cover RES-01 through RES-03.

### WP-V3: Domain enum binding

Dependence: WP-V2.

- Implement strict TOML configuration, `--config`, `SF4011`/`SF4012`, and SHA-256 provenance first.
- Compile the `EnumBindingError` skeleton and schema-local binding trait before implementing the coercion bridge.
- Parameterize arguments and inputs with `T`, including the bound enum in the generated type graph.
- Use `ENUM-01` through `ENUM-06` to test GraphQL literals, variables, generated descriptors, and drift independently.

### WP-V4: Axum adapter

Dependency: WP-V0. This package may proceed in parallel with WP-V1 through WP-V3 after the public hook skeleton is reviewed.

- Add `crates/axum` without leaking Axum, Tokio, or Tower dependencies outside the adapter.
- Implement request and response hooks, a context factory, typed rejection, and append-only headers.
- Exercise body processing and all five timeout phases, using guards to verify that every affected future is dropped.
- Cover AXUM-01 through AXUM-07.

### WP-V5: Adoption example and differential comparator

Dependencies: WP-V1, WP-V3, and WP-V4.

- Add authentication, a request-scoped loader, query-aware rate limiting, and cookie handling to `examples/axum-adoption`.
- Implement the structural comparator and reasoned allowlist schema from ADR-0007 Section 9.
- Apply `DIFF-01` to the complete synthetic operation corpus.

### WP-V6: Release and handoff

Dependence: WP-V0 to WP-V5.

- version/ABI, the compatibility table, the exact version or the 40-digit Git SHA installation, documenting the `--locked` fresh consumer.
- Update architecture Sections 4, 5, 6, 13, 15, and 16 to describe the implemented ABI 3 design.
- Change the status in Section 9 of spec-conformance to Required and trace the full acceptance ID to the actual test name.
- Change ADR-0007 to `Implementation: Implemented` only after two independent
  review tracks and the fresh-clone gate have completed.

## 7. vNext review gates

### Gate V-A: Import and consumer quality

WP-V0/V1 is complete. A large, unprocessed, compatible SDL and consumer pass through the warning-free gate of MSRV/stable.

### Gate V-B: Generated ABI 3

WP-V2/V3 is complete. If the resolution is missing, enum mapping drift appears on compile-time, and the entire generated source is unified by ABI 3.

### Gate V-C: Axum lifecycle

WP-V4 is complete. Codec unification, rate-limit rejection, DataLoader context, cookie header, all of timeout/drop has been tested.

### Gate V-D: Migration proof

WP-V5 is complete. The synthetic corpus has zero structural differences outside
the allowlist, and the fixture contains no secrets or product source.

### Gate V-E: vNext readiness

WP-V6 is complete. The requirements in ADR-0007 Sections 15.1 through 15.3
are satisfied: no unimplemented or skipped tests, no consumer-side lint
allowances, no manual SDL conversion, and no mixed ABI 2/3 output remain.

## 8. vNext exclusions

- Do not replace resolver typestate with a runtime registry.
- Do not discover configuration implicitly from the current or parent
  directories.
- Do not map an enum mismatch to a client error, panic, or unknown-string
  fallback.
- Do not expose raw request or response bodies to the Axum hook.
- Do not let a response hook change status, body, codec ownership, or protected
  headers.
- Do not leave a timed-out future running as a detached task.
- Do not copy product source, credentials, or personal data into repository
  fixtures.

## 9. Production work packages (historical baseline)

This section records the production work-package plan that preceded ADR-0016
through ADR-0019 and the ABI/preset design of that period. The current
single-endpoint builder, generated ABI 5, and completion criteria in Section 12
take precedence over this historical baseline.

### WP-P0: Design acceptance and public contracts

- Review ADR-0004 and ADR-0008 through ADR-0013 for crate direction, semver,
  ABI, phase ownership, cancellation/drop behavior, provider SPI, client
  boundaries, and HTTP status/media contracts.
- Place identity and observer primitives in the std-only `chigiri_core`
  skeleton, and the transport-neutral codec/SPI skeleton in `chigiri_http`.
- Verify the object-safe contracts of `chigiri_operation`, `chigiri_policy`,
  `chigiri_observability`, `chigiri_production`, and `chigiri_testkit` with compile
  fixtures.
- Fix the public schema snapshot/diff model and the `SchemaInfo` skeleton.
- Record the rationale for ABI 4, its transition unit, and the WP-P7 fuzz,
  load, and benchmark conditions before implementation results exist.
- Amend ADR-0004 to establish the application-owned server boundary around
  `chigiri::Endpoint`, `RequestBody`, `DeadlineFactory`, and opt-in bridges.
  Compile fixtures must enforce that boundary and reject a standalone server
  API.

Completion requires the entire skeleton to compile on the MSRV while proving
`Send + Sync`, borrowing lifetimes, object safety, and bounded construction.
Positive and negative fixtures must also verify error/status/media behavior,
timeout-drop ownership, and observer noninterference.

### WP-P1: Developer workflow and generated ABI 4

- Preserve the canonical six-step workflow and compile-time resolver
  completeness.
- Carry the schema digest through generated descriptors into runtime
  `SchemaInfo`, and advance the generated ABI to 4 as one coordinated change.
- Update runtime constants, the generator, golden output, the books example,
  the Axum example, and the large consumer together.
- Trace DX-01 through DX-06 to concrete tests without adding a client artifact
  or runtime resolver map.
- Update the quick start's six stages from an application-owned server call to
  a neutral endpoint call, while leaving listener, routing, and shutdown with
  the application-facing bridge. Enforce the boundary with compile tests.

### WP-P2: Production lifecycle

- P2a first migrates the public endpoint, body, deadline SPI, and bridge
  contracts, then completes the DX vertical slice for Gate P-A.
- P2b completes the 14-phase lifecycle, timeout/drop behavior, and CSRF
  admission before the LIFE acceptance suite for Gate P-B.
- Move the façade-private transport pipeline beneath `chigiri::Endpoint` and
  unify it with the transport-neutral 14-phase lifecycle through
  `chigiri_http`, `RequestBody`, `DeadlineFactory`, and codec/SPI contracts.
- Use one shared order for transport admission, source resolution, decoded
  hooks, context creation, core plugins, observers, and encoding.
- Implement the production CSRF/XS-Search policy, per-phase and overall
  deadlines, bounded body polling, and direct drop of timed-out futures and
  streams.
- Hyper and Axum bridges convert only request, body, timer, and response data.
  Listener, routing, connection, and shutdown ownership remains in the
  application; the books example must demonstrate that ownership.
- Keep Hyper, Axum, and Tokio out of the direct consumer dependency tree.
  Compile fixtures must prove that `chigiri_hyper::service(endpoint)` and
  `chigiri_axum::handler(endpoint)` attach only to an application-owned server.
- Remove the former Axum hook and `chigiri_hyper::Server` double
  pipeline/shim, then satisfy LIFE-01 through LIFE-08.
- Because this work predates a production release, do not add compatibility
  shims for the old hook, server, ABI, or skeleton.

### WP-P3: Operation identity and persisted operations

- Compute a schema digest, document digest, and operation key for every direct
  and HTTP operation.
- Implement a bounded immutable catalog, a bounded APQ store, Apollo APQ
  extraction, five source modes, and revision/readiness reporting.
- Define provider timeouts, per-mode failures, and exact error/status/media
  behavior, then satisfy OP-01 through OP-09.

### WP-P4: Cost and two-stage rate limit

- Implement checked cost arithmetic and unit, coordinate, list, and catalog
  policies, plus bounded quota keys in `chigiri_policy`.
- Apply a list multiplier to the complete sub-selection after fragment and
  alias expansion, checking nested multiplication for overflow. Never clamp;
  reject an invalid or excessive cost before resolver execution.
- Implement a fake clock and bounded in-memory token bucket with atomic
  consumption and provider-conformance coverage.
- Key-capacity exhaustion and single-key policy mismatch must neither become a
  provider failure nor mutate a bucket.
- Combine Stage A before body consumption with Stage B before resolver
  execution, then satisfy BUDGET-01 through BUDGET-02 and RATE-01 through
  RATE-08.

### WP-P5: Error identity and observability

- Implement internal failure IDs, bubbling correlation, the reserved
  `errorId` extension, and an error observer covering every lifecycle phase.
- A request-ID or error-ID factory failure must not trigger repeated ID
  generation.
- Implement lifecycle events, reduction, sampling, closed metrics, and a
  bounded non-blocking exporter.
- Add opt-in field instrumentation and the `chigiri_tracing` and
  `chigiri_opentelemetry` bridges, then satisfy OBS-01 through OBS-09.

### WP-P6: Schema governance and testkit

- Implement semantic schema snapshots and diffs, closed change
  classification, CLI snapshot/check commands, and exit codes 0/1/2.
- Implement immutable `SchemaInfo` and catalog-impact validation.
- Move the comparator into `chigiri_testkit` and add endpoint, bridge, provider,
  observer, and drop-conformance suites.
- Satisfy GOV-01 through GOV-04 and TEST-01 through TEST-04.

### WP-P7: Security, fuzz, performance, and packaging

- Run the six fuzz targets and the load, soak, and benchmark programs specified
  by ADR-0013 Section 2.9.
- Add adversarial-resource, endpoint timeout/drop, application drain,
  cache/queue, and cardinality regressions.
- Enforce production defaults for field-list items, request output nodes, and
  response bytes, masking internal failures.
- Before distribution, verify the source tarball with a fresh consumer,
  dependency advisory/license checks, SBOM, provenance, and source-consistency
  gates. Reject unusual archive members such as symlinks, hard links, and
  devices.
- Produce changelog, migration, MSRV/deprecation, and `SECURITY.md` material,
  then satisfy REL-01 through REL-03.

### WP-P8: Integration, review, and handoff

- Use the `chigiri_production` preset to integrate operation, policy, and
  observer components into a neutral endpoint, then run the same conformance
  suite through the endpoint, Hyper bridge, and Axum bridge.
- Conduct independent reviews of architecture/API/DX,
  security/resources/privacy, and conformance/release/operations.
- Correct and re-review every finding. Critical, high, and medium findings must
  be zero; any low finding must be corrected or explicitly accepted.
- Complete Gates P-A through P-E, reproduce final HEAD in a fresh clone,
  publish the application-owned-server quick start in the README, and only then
  update the ADR implementation status.

## 10. Production review gates

### Gate P-A: Developer contract

The historical completion criterion is WP-P1 plus WP-P2a: all DX cases pass,
ABI 4 has no drift, resolver completeness remains compile-time enforced, and
the neutral endpoint/Hyper/Axum vertical slice exposes no bridge-owned server
lifecycle API.

### Gate P-B: Admission and operation governance

The historical completion criterion is WP-P2 through WP-P4: every LIFE, OP,
BUDGET, and RATE case passes, including adversarial browser, unknown-operation,
and high-cost cases.

### Gate P-C: Diagnostics and privacy

The historical completion criterion is WP-P5: all OBS cases, source masking,
reduction, cardinality, and queue-backpressure checks pass.

### Gate P-D: Change and distribution

The historical completion criterion is WP-P6 and WP-P7: all GOV, TEST, and REL
cases pass together with fuzz/load/soak/benchmark and
package/SBOM/provenance gates.

### Gate P-E: Independent production review

The historical completion criterion is WP-P8: all three review areas and their
re-reviews pass, and final HEAD, the source artifact, and a fresh clone are
reproduced.

## 11. Production acceptance trace plan

Test names are fixed during implementation. `docs/spec-conformance.md` and
Beads review `lab01-c43.8` must use the same fully qualified
names. Planned owner paths are:

| ID range | Primary owner path |
| --- | --- |
| DX-01-DX-06 | `chigiri_testkit/tests/developer_contract.rs`, books/large consumer compile fixture |
| LIFE-01-LIFE-08 | `chigiri_testkit/tests/lifecycle_conformance.rs`, run through the neutral endpoint and Hyper/Axum bridges |
| OP-01-OP-09 | `crates/operation/tests/identity.rs`, `catalog.rs`, `apq.rs`, provider suite |
| BUDGET-01-BUDGET-02 | `crates/policy/tests/cost.rs` |
| RATE-01-RATE-08 | `crates/policy/tests/rate_limit.rs`, testkit provider suite |
| OBS-01-OBS-09 | `crates/observability/tests/`, testkit redaction/cardinality suite |
| GOV-01-GOV-04 | `crates/codegen/tests/schema_governance.rs`, catalog impact test |
| TEST-01-TEST-04 | `crates/testkit/tests/` and endpoint/bridge/provider-side suite invocations |
| REL-01-REL-03 | `scripts/check-production-readiness.sh` and production-review evidence |

At the 2026-08-27 baseline, the implementation-rules checker, vNext adoption,
MSRV/stable formatting, Clippy, tests, rustdoc, and `git diff --check` all exited
successfully. The existing changes were limited to `docs/README.md` and
ADR-0008 through ADR-0013; no pre-production implementation failure was
present.

## 12. Developer usability program work packages

This section implements ADR-0016 through ADR-0019 as one breaking change. The
repository consumer moves to the new contract at the end of each work package,
and the implementation checker runs immediately before and after each package.

Status: WP-U0-WP-U6 complete / Gate U-A-U-E Passed.

The 2026-08-30 designer review reported three medium and one low finding. After
those findings were corrected, the final repository gate, independent
re-review, and fresh-clone verification were rerun and confirmed zero findings
at every severity. The official performance certification remains a separate
release gate. Gate P-D/P-E remains Not Passed.

### WP-U0: Public contract and baseline capture

- Assign each API, INJ, PROJ, and LIMIT acceptance ID to a concrete
  `tests/public_contract`, compile-fixture, or testkit owner path.
- Run the current books example, Axum adoption example, large consumer, quick
  start, and automated-readiness checks, recording behavior and diagnostics
  before the change.
- Establish compile-positive and compile-negative public skeletons for
  `Endpoint::builder`, `Components`, projection stages, and all four limiter
  algorithms before implementation.
- Inventory removed legacy API objects and crate-dependency inversions, and
  lock the no-shim decision into tests and documentation.

### WP-U1: Façade inversion, one builder, and safe baseline

Dependence: WP-U0.

- Move the current runtime to `chigiri_runtime` and then move `chigiri` to the application façade.
- Implement one `Endpoint::builder` that accepts generated `Resolvers` and the existing `ExecutableSchema`.
- ADR-0016 Section 2.3 is implementing the basic configuration, context typestate, rate coverage, and redacted effective configuration.
- Remove the required multi-phase routing and old import paths from current preset/schema/HTTP builder, and update the entire repository consumer.
- Neutral, direct, Hyper, and Axum paths use the same Endpoint and preserve application-owned server lifecycle.

### WP-U2: Typed component graph and override semantics

Dependence: WP-U1.

- Implement a closed `Components` namespace, sequence-based single slots, explicit collection operations, and final validation.
- Route every builder convenience method to the same internal leaf operation and add differential tests between convenience and injection forms.
- Verify same-slot replacement, Stage A/B coexistence, post-build immutability, dangerous weakening, and effective-configuration redaction.
- Reject `Any`, string registries, and secondary construction entry points through compile-negative tests and API source inspection.

### WP-U3: Official limiter algorithms and SPI

Dependence: WP-U2.

- Implement token-bucket, fixed-window, sliding-window-counter, and leaky-bucket-meter algorithms with a bounded in-memory provider.
- Stage A/B sugar, default failure closed/500 ms, deep provider injection are connected to the same component graph.
- Change policy/SPI to a type that cannot express invalid state and delete the old policy shim.
- Test algorithm boundaries, checked overflow, convergence, expiry, capacity, policy mismatch, clock rollback, timeout, and drop behavior.
- Run the third-party provider suite in four official implementations, keeping existing query concurrency boundaries separate from time quotas.

### WP-U4: Projection resolver and generated ABI 5

Dependencies: WP-U1, WP-U2. WP-U3 can be used in parallel implementations, but the integration gate requires both.

- Generate a linear typestate projection plan to a non-root object and receive it in the same assembly as the complete resolver trait.
- Add compile-fail fixtures for missing, duplicate, out-of-order, incorrectly
  typed, and root-level projections.
- Shard and hide private generated helpers, and test public rustdoc plus IDE completion at every typestate stage.
- Update the generator, runtime, golden output, books, Axum, custom-scalar fixture, and large consumer to ABI 5, leaving zero ABI 4 generated outputs or shims.
- Add direct and HTTP tests spanning completion, plugins, cost, limits, and diagnostics.

### WP-U5: Documentation, examples, and migration

Dependence: WP-U1 to WP-U4.

- Order the README quick start as SDL → generation → projection or full resolver → single builder → application-owned bridge.
- The books show the standard workflow and official limiters; the Axum adoption
  guide shows deeply typed injection. Do not introduce shortcuts available only
  to examples.
- Synchronize architecture Sections 10, 15, 18, and 20 with the implementation. Mark ADR-0016 through ADR-0019 Implemented only after Gate U-E completes.
- The pre-release breaking-change note records the removed APIs, their
  replacements, and the commands required to regenerate ABI 5 output.
- Update specification-conformance Section 12 from the planned path to the
  fully qualified test name.

### WP-U6: Review and reproducibility

Dependence: WP-U0 to WP-U5.

- In Review A, we independently audit the public API, IDE/rustdoc, usage description, crate/server boundary, and sugar/inject equivalence.
- Review B independently audits compile-time completeness, resource and security defaults, rate-limit atomicity, concurrency and drop behavior, and redaction.
- Resolve findings at every severity and repeat both reviews until the unresolved count is zero.
- MSRV/stable, all targets/features, generated drift, large consumer, fast start, automated readiness, all through the final HEAD.
- Reproduce the final HEAD isolation fresh clone from the committed lockfile, and clean the source clone and the original worktree.

### Gate U-A: One safe construction contract

Passed. The built-schema second graph was removed. Worker-local request identity,
the final repository gate, the independent re-review, and the fresh-clone run
reconfirmed the single safe construction contract.

### Gate U-B: One customization graph

Passed. Focused tests and the full repository gate reconfirmed the single graph
for built and generated inputs, Stage B ordering, and final diagnostics with
slot and origin. Independent re-review and fresh-clone verification also passed.

### Gate U-C: Complete generated DX

Passed. WP-U4 is complete. PROJ-01 through PROJ-14 are green, only ABI 5 is
present for this historical program, and both projection and complete-trait
forms are compile-time complete.

### Gate U-D: Rate implementations

Passed. WP-U3 is complete. LIMIT-01 through LIMIT-16 are green, and the four
algorithms, Stage A/B, provider suite, and bounded-memory/drop contracts hold.

### Gate U-E: Usability program readiness

Passed. WP-U5 and WP-U6 are complete. After the 2026-08-30 findings were
corrected, all 54 acceptance IDs, both independent reviews, fresh-clone
verification, and the repository-wide automated gate were rerun. The formal
15-minute load/soak, 31-by-100 benchmark, and 30-minute fuzz runs are separate
release gates requiring explicit approval. Changes must record invalidated
performance evidence and the remeasurement TODO.

## 13. DX and rate boundary hardening work packages

This section implements the pre-release breaking program in ADR-0020 through ADR-0024. It preserves ADR-0016 through ADR-0019, generated ABI 5, the single Endpoint, and Stage A/B execution semantics while strengthening tooling and crate boundaries. Do not create compatibility shims for old Rate imports, constructors, or scripts.

Status: WP-D0-WP-D7 complete / Gate D-A-D-E Passed.

### WP-D0: Contract and baseline

- Assign all 69 acceptance IDs to owner paths: SCAF-01 through SCAF-10,
  DIAG-01 through DIAG-15, RSPI-01 through RSPI-12, RMEM-01 through RMEM-16,
  and LCON-01 through LCON-16.
- It records the current CLI human diagnostic, projection public surface, crate graph, package/SBOM set, and 70/750 consumer time/bytes.
- First fix the compile skeleton for `RateLimiter`, its future, key, policy and decision types, memory configuration and provider, structured diagnostics, and CLI options.
- The deleted object import/constructor/script and the generated ABI 5 variables are placed in the contract scan.

### WP-D1: Structured diagnostics and doctor

Dependence: WP-D0.

- Move codegen errors to the structured report while preserving stable codes, source order, and diagnostic limits.
- Implement human and JSON Lines format 1 without placing source text, absolute
  workspace paths, ANSI escapes, or secrets in JSON.
- Implement doctor checks for static schema, configuration, and drift; manifest and package discovery; locked Cargo JSON execution by default; and exit statuses 0, 1, and 2.
- Cargo stdout/stderr is simultaneously bounded drain, and when the limit is exceeded, the child is terminated and waited.
- It maintains rustc originals, and only matches group/field/root error and wrong output to `SF4018`/`SF4019` under high reliability conditions.

### WP-D2: Explicit projection scaffold

Dependence: WP-D1.

- Connect `chigiri rust scaffold --object NAME` to the normal generator's schema, configuration, and naming model.
- Explain `project_*` candidate to argument field with an executable `SF4020` with an `resolve_*` placeholder in SDL order.
- stdout-only, byte deterministic, 64 MiB upper limit, repository/file unchanged by process test.
- Do not add derives, reflection, a runtime registry, default resolvers, or `--write`.

### WP-D3: Standalone rate SPI

Dependence: WP-D0. It can be parallel to WP-D1/D2.

- Add std-only `chigiri_rate` crate and move only the type that appears on the SPI signature of ADR-0022 from `chigiri_policy`.
- Keep `RateLimiter` as an object-safe atomic-decision interface and use `RateLimitFuture` without HTTP dependencies.
- The re-export, third-party, minimal consumer and provider compliance façade is connected to the new crate.
- `chigiri_policy` will leave only the cost policy, stage A/B, timeout/failure/observer composition and delete the old SPI re-export.

### WP-D4: Official in-memory adapter

Dependence: WP-D3.

- Add `chigiri_rate_memory` with regular dependencies only on `atomic-waker` and `chigiri_rate`.
- Preserve the safe default configuration, System/Manual clocks, all four
  official providers, and their algorithm and accounting semantics.
- Cover the bounded store, expiry, capacity, mismatched and over-cost decisions, rollback, conversion, nonblocking behavior, repeated polling, drop, and the shared provider suite.
  Test the algorithm under locking and concurrent contention.
- Connect both façade sugar and deep injection to the new provider while
  preserving the component graph, stage order, last-write-wins behavior, and
  default fail-closed 500 ms policy.
- Remove the old provider, clock, configuration exports, and constructor shims from `chigiri_policy`.

### WP-D5: Consumer, package, and documentation migration

Dependence: WP-D1 to WP-D4.

- Migrate books, Axum adoption, the benchmark fixture, testkit, and compile fixtures to the new diagnostic and rate imports.
- Present scaffold, doctor, standard rate-limit convenience APIs, the third-party SPI, and direct use of the official memory provider in one README learning path.
- Synchronize workspace packages, the lockfile, source archive, fresh consumer, SBOM, provenance, and checksum verifier with the new two-crate layout.
- Update migration, release, and runbook documentation with removed imports, constructors, and scripts; ABI 5 invariance; invalidated formal evidence; and remeasurement TODOs.

### WP-D6: Large-consumer conformance

Dependence: WP-D1 to WP-D5.

- Update the canonical 70/750 fixture to ADR-0024 without changing production or the full-schema fixture source.
- Integrate `check-large-consumer-conformance.sh` with drift, scaffold, doctor, MSRV and stable, the five-command workflow, rustdoc surface, and provider suite.
- Use temporary negative fixtures to test one case each for missing group, field, and root; wrong order; wrong output; drift; and ABI mismatch.
- Keep the fixed thresholds of 180 seconds, 64 MiB, and 400 lines, update the closed evidence, and leave no legacy script shim.

### WP-D7: Review and reproduction

Dependence: WP-D0 to WP-D6.

- In Review A, we independently audit CLI/API/DX, diagnostic confidence, projection clarity, public rustdoc, and crate/import migration.
- In Review B, we independently audit the SPI atomicity, blocking/drop, bounded stream/store, clock/expiry, reduction, and package/SBOM.
- Correct findings at every severity and have the same independent perspectives
  re-review the corrections.
- Run MSRV/stable, all targets/features, the 69-ID trace, the large consumer,
  fast-start checks, and automated readiness against final HEAD.
- Reproduce the final commit in an isolated fresh clone from the committed
  lockfile, leaving both the original and clone worktrees clean.

### Gate D-A: Actionable tooling

Passed. Focused tests and the large consumer confirmed all 25 SCAF/DIAG IDs,
including structured human and JSON diagnostics, Windows drive and authority
file URIs, complete surface redaction, Cargo-backed doctor checks, and explicit
stdout ownership.

### Gate D-B: Minimal provider SPI

Passed. The boundary script confirmed a std-only SPI with zero third-party
dependencies, the minimal external consumer, all 12 RSPI IDs, and no remaining
legacy SPI surface.

### Gate D-C: Official memory adapters

Passed. The shared and inherent suites confirmed all 16 RMEM IDs: four
algorithms, safe defaults, bounded storage/clock/concurrency, registration of
competing waiters, bounded wake-up on release, and waiter removal on drop.

### Gate D-D: Large-consumer proof

Passed. Every LCON ID, MSRV/stable check, fixed threshold, and closed-evidence
check passed across the 15-crate package, source, SBOM, provenance, and checksum
set. A clean candidate successfully recreated the fresh consumer.

### Gate D-E: DX/rate hardening readiness

Passed. WP-D7, the exact 69-ID trace, Reviews A and B with zero findings at all
severities, and fresh-clone reproduction of the final commit all completed.
ADR-0020 through ADR-0024 were updated to `Implementation: Implemented`.
The formal benchmark, 15-minute load/soak run, and 30-minute run for each fuzz
target remain manual release gates and were not executed by this program.
Therefore Gates P-D and P-E remain **Not Passed**, and this result does not claim
production readiness.

## 14. Correlated operation audit identity work packages

This section implements ADR-0025 while preserving ADR-0010 audit fail-open
behavior, ADR-0012 identity/sampling/redaction rules, and generated ABI 5. It
connects `OperationAuditEvent` to the existing request lifecycle without
consumer-specific state, task-local storage, or a global identity map.

Status: WP-OA0-WP-OA4 complete / Gate OA-A-OA-D Passed.

### WP-OA0: Contract and baseline

Status: complete.

- Assign AUDIT-ID-01 through AUDIT-ID-08 to their planned owner paths and run
  the existing OP, OBS, production, and DX acceptance baseline.
- Record the public API, lifetime, and dependency boundaries of
  `OperationAuditEvent`, `RequestEnvelope`, `RequestTelemetry`, and
  `RequestIdentity`.
- Freeze generated ABI 5 output, schema digests, normal crate dependencies, and
  HTTP behavior before making changes.
- Create the audit-identity module/file and compile-test skeleton before adding
  behavior tests to `persisted.rs`.

### WP-OA1: Public event and source propagation

Dependence: WP-OA0.

Status: complete.

- Add only a borrowed `RequestIdentity<'a>` and `identity()` accessor to
  `OperationAuditEvent<'a>`.
- Keep event construction crate-private. Do not add separate request-ID or
  trace-ID getters or an owned duplicate of the identity.
- Have source resolution derive `identity(None)` from the existing envelope
  telemetry; without telemetry, use an empty identity.
- Do not regenerate request/trace IDs, resample, move audit delivery before
  parse/validation, or feed operation identity back into source resolution.
- Ensure `Debug` for the event and identity hides identifier values and cannot
  leak a raw document or provider source.

### WP-OA2: Delivery, failure, and concurrency conformance

Dependence: WP-OA1.

Status: complete.

- Exercise catalog hit, miss, provider-unavailable, sink-failure, telemetry,
  and sampling-drop paths.
- Provider or sink failure remains fail-open for the request and notifies the
  error observer exactly once. Audit and error events must carry the same
  envelope identity; test request ID, trace ID, sampling, and operation `None`
  separately.
- Verify exactly-once audit/observer delivery with and without telemetry,
  provider failure, sink failure, empty identity, and default recording while
  preserving request continuity.
- Use a barrier-based parallel test with distinct request IDs, trace IDs, and
  digests to detect cross-wiring, omission, or overlap.
- Exactly-once audit delivery must not change APQ, Safelist, IdOnly, or Disabled
  source semantics.
- With a failing registry or sink, embedded and Axum endpoints must preserve
  ordinary data, byte-exact status/body/headers, and the standard `Debug`
  contract. `LifecycleExporter::ErrorRecord` must not expose request IDs, trace
  IDs, raw documents, or provider/sink sources.

### WP-OA3: Documentation and repository gates

Dependence: WP-OA1 to WP-OA2.

Status: complete.

- Synchronize ADR-0025, architecture Section 22, this handoff, and
  spec-conformance Section 14.
- Trace AUDIT-ID-01 through AUDIT-ID-08 individually to fully qualified tests or
  checks; do not leave planned owner placeholders.
- Add a closed trace checker that fails on missing, duplicate, or unknown IDs
  and on owner-removal mutations.
- Document operation `None` during source resolution, non-response sampling,
  borrowed lifetimes, and sink-side copying in public rustdoc.
- Re-run the production acceptance trace, generated-drift checks, dependency
  boundaries, package/source/SBOM/provenance checks, and fresh consumer.
- If changing the public-event hot path invalidates official performance
  evidence, record the required manual remeasurement in the runbook and review.

### WP-OA4: Independent review and reproduction

Dependence: WP-OA0 to WP-OA3.

Status: complete.

- In Review A, we independently audit public API, lifetime/object security, source phase ordering, ABI/dependency/response variables and rustdoc.
- In Review B, we independently audit the delivery of security audits, sampling, failure exactly-once, conversion cross-wire, and redaction.
- Correct findings at every severity and have the same independent perspective
  re-review each correction until no finding remains.
- Run MSRV/stable focused and workspace tests, Clippy, rustdoc, and every
  acceptance/repository gate against the final candidate.
- Reproduce the exact final commit in an isolated fresh clone from the committed
  lockfile, leaving the original and clone worktrees clean.

### Gate OA-A: Public contract

Passed. WP-OA0/OA1 established `identity()` as the single public surface and
covered source-phase operation `None`, absent telemetry, ABI, dependencies, and
response invariance. MSRV/stable compilation, rustdoc, and independent Review A
confirmed the contract.

### Gate OA-B: Security and correctness

Passed. WP-OA2 and AUDIT-ID-01 through AUDIT-ID-08 confirmed noninterference
from sampling drops, provider/sink fail-open behavior, failure and telemetry
absence, exact error-observer correlation, zero parallel cross-wiring, real
Axum response invariance, and standard diagnostic reduction. Focused tests and
independent Review B both passed.

### Gate OA-C: Repository conformance

Passed. WP-OA3 closed all eight trace IDs and mutation cases, reran the existing
OP/OBS/production/DX traces, and verified generated output and dependency
boundaries. The exact final commit reproduced the package, source, SBOM,
provenance, fresh-consumer, and 400-line checks.

### Gate OA-D: Audit identity readiness

Passed. WP-OA4, Reviews A and B with zero findings at every severity, automated
readiness, release conformance, and fresh-clone reproduction all passed against
the exact final commit with clean worktrees. ADR-0025 was updated to
`Implementation: Implemented`. The formal benchmark, 15-minute load/soak run,
and 30-minute run for each fuzz target remain manual release gates and were not
executed by this program. Therefore this program does not pass Gates P-D or P-E
and does not claim production readiness.

## 15. Sibling generator program work packages

Status: Accepted / Implemented. See the validation record for executed scope and unexecuted environments.
Implementation instructions are in the sibling-generator goal prompt.
The product contract is defined by [ADR-0026](adr/0026-generated-contract-crate-and-manifest-cli.md) and [ADR-0027](adr/0027-lightweight-validation-and-generator-build-identity.md). Section 16 and [ADR-0028](adr/0028-semantic-generated-layout-and-handwritten-line-budget.md) add generated-file partitioning and line-budget rules.

| Work package | Deliverables | Primary completion gate |
| --- | --- | --- |
| WP-S0 | Baseline; old surface; publisher and shared-test inventory; validation records | Work boundary, document precedence, and line checks |
| WP-S1 | config-v2, Git/path sources, GeneratorInfo, help, version, and init | SG-A and SG-D focused tests |
| WP-S2 | Standalone generated crate, ABI 6, type binding, and three-crate fixture | SG-B, standalone checks, and negative consumer checks |
| WP-S3 | Bounded generation, update, comparison, doctor, snapshot, diff, and scaffold | SG-C, metadata-aware exact checks, JSON, and redaction |
| WP-S4 | Migration of all examples, testkit, fixtures, and CI; publisher removal; validation | No old surface remains and the shared product gate is preserved |
| WP-S5 | full-schema corpus, final validation, independent read-only Reviews A/B, and handoff | SG-A, SG-F, and every SG-ID trace |

Run focused tests first for each subtask. Consolidate the full product suite in
the final step; do not require repeated execution against unchanged source or
cache removal.
The gate retains zero normal dependencies for core, MSRV/stable coverage, the
400-line handwritten limit, generated semantic-unit layout, English rustdoc,
and the additional unsafe-code prohibition.
Generated-output corrections must flow through the producer → regular CLI → reproduction sequence; do not hand-edit generated output.

### Finalization

Map every SG-A through SG-F gate to the real tests in specification-conformance
Sections 15 and 16, and resolve every outstanding finding from independent
Reviews A and B. After the final source change, retain the validation tree,
including untracked artifacts, and record results together with both executed
and unexecuted environment jobs.
Package publishing, SSH fetches, clean commits, commit or push operations, publication, and tagging are not completion gates.
If the official performance/load/long fuzz separate gate is not running, it will remain in that state and will not be confused with Product Transfer Pass.
CI calls Linux/macOS the common `validate.sh`, and it names `Product validation required` as the required job.
An `always()` aggregate job inspects both operating-system results and validation output, rejecting failure, skip, cancellation, or missing results.
The remote settings of branch protection will not change.
Inspecting a workflow before CI runs does not count as a completed hosted CI
run.

Run the repo-owned full-schema acceptance corpus. Application Cargo, binding
and CLI migrations are separate integration work. Do not stage, commit, or push without explicit user instructions.

## 16. Semantic generated layout amendment work package

Status: Accepted / Implemented. [ADR-0028](adr/0028-semantic-generated-layout-and-handwritten-line-budget.md) applies across WP-S2 through WP-S5. See the validation record. The additional goal and overall goal remain as implementation-request history.

1. Separate the 400-line handwritten-code check from the generated-ownership
   inventory check.
2. Change writer/renderer to model/role/responsibility base, and remove all unnecessary numbered packing.
3. Focused tests prove that generated output over 400 lines is accepted when
   owned correctly, handwritten output at 401 lines is rejected, false claims
   of generated ownership are rejected, and semantic-unit layout produces only
   localized diffs.
4. Regenerate every fixture, example, testkit output, large consumer, and full-schema corpus through the regular CLI while preserving API, ABI 6, and digest contracts.
5. Run SG-LAYOUT-01 through SG-LAYOUT-08, then rerun existing SG-A through SG-E plus the added SG-F gate.
   Independent Review A is the layout/public contract, B is the ownership/resource limit/exclusion loop.

Success is not defined by retaining the old “generated files under 400 lines” rule or merely inventing a new semantic layout.
Existing traces such as PROJ-11 and the large-consumer evidence do not
automatically satisfy the new assertions, and historical run markers are not
reclassified as passes under the new policy.
The rest of the product quality/security gate, stage/commit banned, external applications read-only, publisher non-essential cannot be changed.

## 17. Experimental Luau mock CLI

Status: Accepted / Implemented. Gates M-A through M-D and MOCK-01 through MOCK-16 Passed in local macOS validation.
The contract and complete acceptance IDs are in [ADR-0029](adr/0029-experimental-luau-mock-cli.md). Give a new implementation agent the implementation goal prompt.
This program evaluates `init` and `serve` using a fixed blog fixture; it does not authorize arbitrary-schema or production AI implementation.
The production path continues to prohibit dynamic resolver maps; scripts run only behind the fixed generated trait used by this experiment.

| Work order | Result | Completion evidence |
| --- | --- | --- |
| WP-M0 | Baseline and conditions, Mock 1.95, integration wiring, adoption of the existing VM spike, types, and host calls | MOCK-01; fixed 1.95.0 and stable checks and API |
| WP-M1 | Fixed-SDL contract crate, TOML `init`, typed resolver, worker, and HTTP `serve` | Gate M-A; real binary execution |
| WP-M2 | Post CRUD, relation snapshots, transactions spanning both models, and input and return-value tests | Gate M-B with independent success and failure assertions |
| WP-M3 | resource budgets, cancellation and worker recovery, CORS/admission, integration with existing common validation, tests, and handoff | Gate M-C/M-D |

Do not move Luau values across a `Send` boundary or run concurrent VMs on HTTP runtime threads.
Use focused tests to reach the first working HTTP iteration instead of repeatedly running the full workspace suite while diagnosing failures.
Finally, run common validation, exact and standalone quality checks for the regular product, and existing GraphQL and HTTP regressions.
Loop and excessive-allocation tests require an external watchdog. A forced stop does not count as successful worker recovery or a Passed result.

Implementation records map MOCK-01 through MOCK-16 to concrete tests and commands and record the application and experimental environment, binary size, build and startup time, RSS, representative diagnostics, unexecuted CI, and unverified results. Do not mark the ADR or architecture Implemented before completion.
Do not add SSH, packaging, a clean commit, publishing, tagging, long-duration load, or fuzzing as hidden completion conditions.
External applications remain unchanged. Stage, commit, or push only under separate explicit instructions.

WP-M0's MSRV incompatibility with M0-01 and independent VM spike were kept in the history of experimental-luau-mock-validation.md.
2026-09-08's mock-only MSRV 1.95 approval was reflected in the package/default-members/common verification, and WP-M0 to M3 were completed.
Real HTTP verification covers binary roots, bidirectional relations, rollback of both models, cancellation, shutdown, recovery, and worker join.
The initial run used fixed Rust 1.95.0 and passed 6 unit tests, 27 CLI/HTTP tests, 5 parent VM tests, the existing Rust 1.85 checks, and all stable checks.
The canonical mock-blog retained ABI 6 and MSRV 1.85; all-SDK exact/quality checks and the Mock consumer doctor passed.
Common validation confirmed 64 commands and source invariants. The validation record is the single evidence entry point for the final source.

Reviews A and B found stop/deadline races, string-metatable paths, and missing assertions. After correction and re-review, no findings remained.
Common validation exposed incorrect `default-members` in the build-info isolation fixtures. The fixtures were corrected and verified with 13 cases plus an additional read-only review.
The release binary measured 9,281,696 bytes; cold and rebuild times were 31.70 and 0.41 seconds; startup took 0.70 seconds. The same record contains RSS and diagnostic measurements, executed commands, start and finish HEADs, the untracked-file inventory, and unexecuted Linux and remote CI. Publishing and committing were not completion conditions; the dirty worktree state was retained for designer handoff.

An additional Medium design-review finding about native-stack exhaustion through `pcall` was corrected by enforcing a simultaneous re-entry limit of 16.
The debug suite passed 6 unit, 30 CLI/HTTP, and 5 VM-parent tests; three additional release tests also passed.
Real HTTP tests confirmed restoration of both models, record order, and counters, recovery on the same server, and worker join after excessive re-entry and caught `MemoryError` failures.
Additional Reviews A and B reported zero remaining code or test findings. The limit was added to the ADR, and the common plan grew to 65 commands including the release regression.
The validation record's follow-up section contains the final-source certificate and the post-correction designer reviews.

## 18. Runtime schema Mock IR work packages

Status: Accepted / Implemented. The normative contract is [ADR-0030](adr/0030-runtime-schema-mock-ir-and-directives.md). The goal prompt remains as implementation-request history.
WP-RSM0 through WP-RSM5, Gates G-A through G-E, and RSM-01 through RSM-22 are complete.
The program connected owned schemas, the shared IR, generic store, typed Luau, shared workers, and CLI v1, replacing the fixed implementation.
The same binary was verified with the blog domain, a renamed second domain, and offline HTTP operation without the original SDL.
Final validation passed 118 unit tests, 41 HTTP tests, 7 parent VM tests, 3 release re-entry tests, 2 general parent VM tests, regular Rust 1.85, Mock 1.95, stable, and all-SDK checks.
The 66-command common validation, measurements, and independent read-only Reviews A and B with post-fix re-review are complete.
See the validation record for test-to-assertion mappings, commands, final-source comparison, and unexecuted CI.

| Work order | Result | Completion evidence |
| --- | --- | --- |
| WP-RSM0 | Measured baseline, constraints, existing Mock and regular-product boundaries, and ownership for 22 planned IDs | Read-only research, priorities, and restrictions |
| WP-RSM1 | Connect the current executor to owned runtime schemas, type names, and dispatch | Two schemas, instance separation, typed ABI 6 protection |
| WP-RSM2 | Directives and IR, generic store and value conversion, scalars, seeds, and relations | G-A and G-C positive and negative focused checks |
| WP-RSM3 | IR-derived Luau types, CRUD and manual resolvers, self-contained `init` and `serve` | Same-binary blog execution, every operation, and rejection of unimplemented entries |
| WP-RSM4 | Second domain and rename, abstract types and payloads, cancellation, budgets, and recovery | Gates G-B through G-D with real CLI, HTTP, and VM recovery |
| WP-RSM5 | Remove the fixed implementation; complete common validation, review, correction, and re-review | G-E and individual evidence for every RSM ID |

Changing schema input must not require generating Rust or Cargo projects, starting a special fixture, adding schema-specific Rust branches, or using fixed Luau types.
Missing owned-schema, value-conversion, and host-API capabilities are authorized implementation work, not external blockers.
Any additional MSRV, ABI, VM, or security-boundary change requires a minimal reproduction and a proposed replacement for designer review.

Run focused tests during development and common validation against the final source, reusing the cache safely.
The implementation retains product 1.85, Mock 1.95.0, and stable coverage; all
SDK exact/quality checks; the 400-line handwritten limit; and English rustdoc.
Independent read-only Review A covers schema, GraphQL, and state semantics; Review B covers VM, resources, CLI, and dependency boundaries. Both perspectives re-review corrections.
Record new evidence in `docs/runtime-schema-mock-validation.md` without overwriting historical MOCK decisions.
Keep external applications read-only. Do not stage, commit, push, publish, or tag, and do not restore publisher or SSH work as completion requirements.

## 19. Mock model API / generated reference work packages

Apply ADR-0031 and the implementation goal.
WP-MR0 covers the baseline and inventory; MR1 the four-way contract; MR2 QueryPlan and count; MR3 owner-relation editing; MR4 reference documents and paging; and MR5 the v2 migration, real HTTP and VM checks, common validation, and review.
Run the implementation-rules checker after every subtask and after formatting, and run common validation against the final source.
Complete Gates MR-A through MR-E and the individual assertions for all 27 IDs.
Run independent read-only Reviews A and B, correct findings at every severity,
and have both reviewers recheck the corrections. Record comparable test
results, untracked artifacts, and unexecuted CI in the
ADR-0031 validation record.
Do not add retrospective MR evidence to ADR-0030's earlier completion record or
mark this program Implemented/Passed before all of its gates are complete.

WP-MR0 through WP-MR5, MR-01 through MR-27, and Gates MR-A through MR-E are complete locally.
Common commands, measurements, independent Reviews A and B, all-severity corrections, and re-review ran against the final source.
No findings remain. Validation passed 159 library tests, 6 CLI tests, 44 HTTP tests, 6 reference tests, 7 parent VM tests, and release recovery checks.
After common validation, changes were limited to seven status and evidence documents; file hashes tied the evidence to the verified implementation source.
See the validation record and `target/mock-model-reference-handoff/` artifacts for details. Results remain in the unstaged worktree; remote Linux and macOS CI are unconfirmed.

## 20. Mock project / portable export work packages

Apply ADR-0032 and the implementation goal.
WP-MP0 covers the baseline and inventory; MP1 configuration VM, external SDL, stable types and modules, and the owned snapshot; MP2 `init`, `check`, `serve`, JSONL, and HTTP recovery; MP3 B/U/N and transactions; MP4 reference and export; and MP5 legacy-route removal, common validation, independent review, measurements, usage, and recovery procedures.
Run focused checks after every subtask and the implementation-rules checker after each subtask, after formatting, and before handoff.
After implementation, independent read-only Reviews A and B evaluate every severity finding, verify corrections, and re-review the result.

WP-MP0 through WP-MP5, all 32 IDs, and Gates MP-A through MP-E are complete locally. Every finding from independent Reviews A and B was corrected and re-reviewed; none remain.
The 73-command common validation, debug and release checks, two-domain equivalence, macOS external-network disconnection, and build, update, and export measurements passed.
The ADR-0032 validation record contains the concrete assertions, initial failures, and corrections.
The record identifies the final source, 82 untracked files, and unexecuted OS/remote CI checks.
After completion, only evidence documents changed, and source hashes retained the connection to common validation.
The 2026-09-09 designer review Passed. Comparison of the complete worktree, including untracked files, plus independent tests found no new finding at any severity.
The designer review used independent comparison and did not rerun the full 73-command suite. Linux and remote CI remain separate, unexecuted gates.
Before choosing the scope of a future commit, compare the 82 untracked files recorded at the first handoff with the current list and account for every addition or removal. Ensure that project implementation artifacts and test-return files are not adopted accidentally.
Do not change external applications or stage, commit, push, publish, or tag; hand off the
worktree changes instead.

## 21. Agent-assisted Mock inference work packages

Apply ADR-0033 and the implementation goal.
WP-AI0 covers the baseline, capabilities, and budgets; AI1 traits, context, candidates, partial inspection, and the correction loop; AI2 the Codex process and fake executable; AI3 proposals, concurrency, transactions, and B protection; AI4 CLI, TUI, PTY, and HTTP; and AI5 real Codex execution in two domains, common validation, independent Reviews A and B, post-fix re-review, measurements, and handoff.
WP-AI0 through WP-AI5 are complete locally. AI-25 ran only after explicit permission and did not reuse ADR-0032 Passed evidence.
Run focused verification and the implementation-rules checker after every subtask, after formatting, and before handoff.
Real Codex input is kept separate from common validation and requires explicit user permission and a dedicated fixture.
The ADR-0033 validation record summarizes results, failures and corrections, and unexecuted environments.

The final 75-command common validation passed without misclassifying a timeout as exit status 0.
It passed 328 Mock library tests, 10 CLI unit tests, 50 HTTP tests, and 17 inference tests. Parent tests explicitly executed the otherwise ignored child processes, while the real-Codex preparation test remained intentionally ignored.
The matrix covered debug and release builds, suspension, rollback, recovery, normal Rust 1.85, Mock Rust 1.95, stable Rust, exact and quality checks for every SDK, generated ABI 6, and the existing resource budgets.
Real-inference verification covers full-file approval for domain/rename changes, offline HTTP adoption, and the macOS external-network-block canary.
Independent reviewers A and B examined six product cases, three supplemental-script cases, three coverage cases, and two documentation cases. After correction and re-review, no findings remained at any severity.

All 28 IDs and Gates AI-A through AI-E Passed locally. Under the authorized execution procedure, AI-25 made four calls, produced ten adopted candidates across two dedicated domains, and passed 28 fixed HTTP cases covering project, restart, and bundle behavior.
After adoption, the portable result was verified without the previous proposal, the Codex executable, the original project, or its SDL files. The external-network canary also confirmed that network access remained blocked.
Linux and macOS remote CI and Linux PTY and recovery behavior remain unexecuted.
Designer review Passed with no new findings at any severity. Independent testing and retained authentic Codex evidence were verified without rerunning the entire 75-command suite.
At implementation handoff, HEAD still matched the starting `main`, the index was empty, 29 tracked files had changes, and 104 files were untracked.
Before the designer-review record was updated, HEAD advanced to `279de4bb…`; all 133 files were verified against the committed inventory and saved content.
Changes after common validation were documentation-only decision records, compared against the fully tested source, fixtures, scripts, and measured binary.
The ADR-0033 validation record contains the full inventory, initial failures, recovery, and measurements. Its Designer Review section maps the decision to current HEAD.
Shared crates and live application source were unchanged. The worktree was handed off without staging, commit, push, publication, or tagging.

## 22. Generated Luau formatting work packages

Status: Accepted / Implemented. Apply [ADR-0034](adr/0034-generated-luau-formatting.md).
This work began as a separate task after ADR-0033 implementation and design review and did not add FMT evidence retroactively to earlier Passed results.
Give new implementers the ADR-0034/0035 integrated goal.
Under the integrated goal, complete FMT0–FMT3 and their focused checks before proceeding to ADR-0035, then combine FMT4 and DX35-5 in the final stage.
Do not mark the full FMT/DX35 inventory Passed at an intermediate checkpoint;
verify every ID against the final source first.

| WP | Implementation unit | Primary acceptance |
| --- | --- | --- |
| WP-FMT0 | Baseline protection, StyLua and real-source conformance, and dependency-license compliance | FMT-01, FMT-04, and FMT-22 |
| WP-FMT1 | Fixed policy, shared formatter, static generation, and B/U/N digest integration | FMT-A and FMT-B |
| WP-FMT2 | AI candidates, correction loop, proposal v2, isolated worker, and `apply` conformance checks | FMT-C |
| WP-FMT3 | Semantic preservation, original retention, PTY and HTTP checks, resource collection, cancellation, and recovery | FMT-D and FMT-21 |
| WP-FMT4 | Final common validation, measurements, independent Reviews A and B, corrections, re-review, and handoff | FMT-E and the complete ID trace |

Format newly generated Luau before inspection and saving. Never overwrite user-edited U, originals, recovery data, or exported resolver text merely to normalize formatting.
Compare formatted static candidate N with U by exact bytes. `apply` reads, reviews, and installs the exact approved candidate bytes.
Do not mask code or fixture failures with raw-storage fallback, larger budgets, type or null exemptions, or removal of rollback tests.

Prioritize focused implementation and tests, and run the 400-line checker after each subtask, after formatting, and before handoff.
Run full verification in the final stage and record formatter effects on binary size, elapsed time, and observed memory.
The common plan checks ordinary Rust 1.85, Mock Rust 1.95, stable, every SDK's exact/quality contract, ABI 6, dependency isolation, and narrowly approved license exceptions.
Review A independently covers semantic and type preservation, storage, and B/U/N. Review B covers resources, process ownership, approval, and licenses.
Maintain usage and migration guidance plus an FMT validation record mapping every ID to assertions, failures and corrections, untracked files, and recovery material.
FMT0 through FMT4, automated checks, measurements, reviews, and handoff are complete; FMT-A through FMT-E Passed locally.
All 84 commands, including the approved dependency policy, passed.
The integrated validation record contains individual assertions, corrected failures, and independent Reviews A and B.
The additional process finding B-2 was corrected and rereviewed; no finding remains at any severity.
Linux and macOS remote CI remain unexecuted. Any additional real-AI testing requires separate explicit authorization.
Real Codex calls, changes to external applications, publisher/SSH work, and
stage/commit/push/publish/tag operations are outside the completion conditions.

## 23. CLI TUI and embedded GraphiQL work packages

Status: Accepted / Implemented. Apply [ADR-0035](adr/0035-cli-tui-and-embedded-graphiql.md).
As an ADR-0034 follow-up, this work must verify its own baseline and must not
replace earlier Passed records or independent untracked work with current
results.
DX35 IDs are a separate inventory from ADR-0008's earlier DX IDs. The integrated implementation goal may use Section 22's intermediate checkpoint.
Do not pause intermediate implementation work merely to wait for an external
review or commit. Final independent Reviews A and B must still confirm the
contracts of both ADRs.

| WP | Implementation unit | Primary acceptance |
| --- | --- | --- |
| WP-DX35-0 | Baseline; complete CLI, terminal, and HTTP inventories; library, MSRV, and license checks; embedded assets | DX35-01, DX35-06, and DX35-13 |
| WP-DX35-1 | Clap definitions, diagnostics, help and version, examples and benchmarks, and removal of the old parser | DX35-A |
| WP-DX35-2 | Ratatui and shared terminal ownership, built-in reference viewer, approval state machine, and PTY checks | DX35-B |
| WP-DX35-3 | Optional GraphiQL assets, Endpoint and snapshot, neutral components, and both bridge helpers | DX35-C |
| WP-DX35-4 | Mock flag, exact own-origin handling, startup display, existing HTTP tests, and minimal manual UI procedure | DX35-D |
| WP-DX35-5 | Common validation, measurements, independent Reviews A and B, correction and re-review, usage, migration, and handoff | DX35-E and the complete ID trace |

The migration must not merely move handwritten help text to another location;
the entire ANSI screen implementation moves to ratatui.
The HTML is not treated as browser verified, but if you transfer it to manual verification, you will specify the status.
Do not copy clap's raw diagnostics. Preserve typeahead and paste separation, require full-text review before approval, and keep candidate bytes identical through approval and installation.
Enabling GraphiQL also exposes introspection. The general Endpoint remains unpublished by default, and GraphiQL does not weaken authentication, CSRF, rate-limit, or safelist rules.
It changes no server ownership beyond route configuration and verifies feature-disabled assets, regular Rust 1.85, Mock 1.95, and ABI 6.

Split new handwritten frontend glue by responsibility and keep each file within 400 lines. Verify the inventory and licenses of vendored and generated assets.
Run focused tests with existing dependency caches first, run the implementation-rules checker after each subtask, after formatting, and before handoff, and run common validation against the final source.
Review A independently covers CLI, TUI, approval, ownership, and ADR-0034 regressions. Review B covers HTTP, assets, GraphiQL, dependencies, and browser boundaries.
Maintain usage procedures and a DX35 validation record that maps all 30 IDs to individual assertions, commands and results, failures and corrections, measurements, and unexecuted scope.
Limit browser work to the one-browser, one-fixture manual smoke test in ADR Section 7.3 and provide a reproducible procedure for users.
Add a short UI, completion, Query, and Mutation example to an existing document. Do not add a dedicated E2E runner, browser dependency, or CI job.
Once Rust and HTTP automation and the reproducible procedure are complete, record pending manual UI execution separately; it does not block implementation acceptance.
Real PTY approval and terminal restoration remain mandatory. Use a fake backend so validation requires no real AI transmission or charges.
DX35-0 through DX35-5, functional checks, measurements, reviews, and manual UI procedures are complete; DX35-A through DX35-E Passed locally.
The user explicitly approved three additional GraphiQL licenses for DX35-0 on 2026-09-09, and the scoped dependency-policy checks passed.
After recording that approval, all 84 commands in the common validation plan passed and the source snapshots before and after the run matched.
The earlier 73-command result, 13 cases after fixture correction, and subsequent 9-command result remain historical evidence and are not added together as a substitute for the final run.
See the integrated validation record for individual evidence and source diffs.
The designer review received on 2026-09-09 passed. It found no new integration issue between ADR-0034 and ADR-0035 at any severity.
The review covered the complete worktree, including untracked assets, vendored files, split modules, and tests. The validation record distinguishes the reviewer's independent reruns from inspection of the common 84-command evidence.
GraphiQL manual verification and remote CI remain unexecuted, but are not unresolved implementation findings. Report local results, designer approval, and unexecuted Linux/macOS remote CI separately.
Changes to live application source, publisher or SSH work, and stage, commit, push, publish, or tag operations are not completion conditions.

## 24. Mock Luau hooks and JSON logging work packages

Status: Accepted / Implemented. Apply [ADR-0036](adr/0036-mock-luau-hooks-and-json-logging.md).
It starts as a separate task after the implementation and review of 0034/0035. Protect existing source/untracked/recovered data, and do not overwrite parallel work.
Past Passed is not used as a proof of HK entry. All HK-ID and Gate HK-A to E are local Passed.
The ADR-0036/0037 integrated goal connects this work to Section 25. Intermediate checkpoints did not wait for external review or commits; final common validation and independent Reviews A and B covered the integrated source.
Individual tests cover configuration and ownership, bundle v4, concurrent VM and worker behavior, shared read-only context and parent values, safe `FieldError` summaries, HTTP plugins, and numeric statuses.
The JSONL writer and access log use a bounded child pipeline; tests cover closed and broken sinks, continued response handling, and child collection under the shared deadline.
The six-slot state contract is shared with API/reference v3, including real-CLI rejection of missing required initial values.
Fake-backend correction-loop tests cover valid and invalid hook candidates, proposal v3, non-TTY storage, drift, installation, and rollback. Common events drive TTY and non-TTY inference communication displays, with real pipe and PTY tests and existing approval recovery.
Debug and release real-HTTP tests verified six-slot resource exhaustion, shutdown, and recovery of both models; the release hook HTTP target was added to the 85-command common plan.
Every slot is covered for worker cancellation, dropped futures, invalidation, queue capacity, remaining deadline, and shutdown. The final 85-command common plan passed.
Across two domains and a rename, three inference batches proposed 33 hook, resolver, and seed candidates. Communication display, complete diffs, and final approval covered every accepted candidate.
Across project, restart, and bundle modes, 115 HTTP assertions covered authentication, state, field rejection, after-hook observation, byte-identical installation, and management-metadata retention in debug, release, and the macOS external-network-blocked canary.
DTOs retain per-field snapshots across all six slots, across request commits, concurrent mutations, and rollback.
Debug and release real-HTTP tests now reverse completion deterministically for
the same query and verify after-hook order and access status.
UX37's TUI and final integrated acceptance are complete. All 60 IDs and 10 gates passed locally, with zero unresolved findings.

| WP | Implementation unit | Primary acceptance |
| --- | --- | --- |
| WP-HK0 | Baseline, six slots and context, existing deadline and snapshot integration, format migration, and dependency boundaries | HK-01, HK-06, HK-07, and HK-25 |
| WP-HK1 | State types and initial values, project and management formats, type generation, B/U/N and `.dist`, startup checks, and export | HK-A |
| WP-HK2 | Rust plugin and `next`, before and after hooks, numeric HTTP rejection, read-only store/request/result, snapshots, and commit | HK-B, HK-13, and HK-14 |
| WP-HK3 | `ctx.log`, HTTP access logs, stdout JSONL, bounded writer loss and recovery, Origin, and CORS | HK-C |
| WP-HK4 | Shared reference, hook candidates and proposal v3, all-inference communication display, complete approval and installation, and two-domain HTTP | HK-D |
| WP-HK5 | Final common validation, recovery and measurements, independent Reviews A and B and re-review, migration, usage, and handoff | HK-E and the complete ID trace |

Rust owns state and progress and never waits on `next` while retaining a Lua VM.
Only `request.before` and `operation.before` may return atomic state patches. Before hooks may reject; every `after` hook is observation-only.
An `after` hook cannot turn success into a GraphQL error or roll back a committed mutation root.
Numeric rejection status applies to the HTTP response. Tests cover valid rejection, invalid status normalized to 500, maximum status across multiple rejections, and rejection with status 200.
General Endpoint status selection remains unchanged; Mock's explicit override is a distinct behavior from normal GraphQL-over-HTTP status selection.
Do not reset the deadline for each hook. Preserve worker and writer ownership, stopping, and collection during cancellation or disconnection.
Disabled hooks still protect their editable files. Document the required
`update`/`check` workflow after type, state, or registration changes, together
with the manual migration procedure for old formats.
AI may not modify configuration, registration, or state definitions; adopted
hook changes remain classified as U.
Show the actual sent and received content for hook, resolver, and seed inference.
Keep the bounded communication events separate from protocol validation and
candidate approval.
Without adding persistent communication history or raw process dumps, connect
TTY and non-TTY display loss and cancellation to the existing I/O and recovery
contracts.

Extend the existing HTTP, VM, PTY, transaction, and fake-backend helpers, and
lead each implementation increment with focused tests.
Run the implementation-rules checker after each subtask, after formatting, and before handoff; aggregate common validation against the final source.
Preserve regular Rust 1.85, Mock 1.95, stable, ABI 6, all-SDK exactness and quality, and dependency boundaries.
Review A independently covers authority, state, GraphQL, HTTP, snapshots, and commits. Review B covers resources, cancellation, logging, ownership, approval reasons, and communication display.
Map assertions, commands, and results to all 36 IDs and record disabled-versus-enabled measurements plus new-file and recovery inventories.
Usage procedures cover authentication and state, field rejection, `after`
logging, stdout pipes, log loss, inference communication display, and
non-destructive migration of an old project.
Implementation does not require real AI calls, a new browser E2E or CI foundation, publisher or SSH work, or changes to external applications.
Report local validation, unexecuted operating systems and remote CI, and designer approval separately.
We connected WP-HK1's setup/state, hook type/module generation and management identity to B/U/N, readiness/bundle, runtime and reference.
We connected the valid hook's logic/proposal v3 and all-infer communications to the UX37 UI, completing the integrated input.
The integrated validation record consolidates all 60 IDs, individual assertions, and final evidence.

## 25. Mock TUI usability and Unicode work packages

Status: Accepted / Implemented. The normative contract is [ADR-0037](adr/0037-mock-tui-usability-and-unicode.md).
The existing AI/FMT/DX35/HK is implementing a different 24-ID inventory and Gate UX37-A to E. At the time of design addition, it is all Not run.
The integrated goal connects this program to ADR-0036's seven communication-event forms.
Unlike the 0037 alone, a coexistence test of the communication body and fixed progress in the final source is required.

| Work package | Scope | Primary acceptance |
| --- | --- | --- |
| WP-UX37-0 | Baseline the existing UI, keys, approval, deletion, resources, and ADR-0036 integration/assignment | Difference and preservation boundaries for every ID |
| WP-UX37-1 | Shared ratatui layout, text, checkboxes, status/footer, lists, search, reference view, and resize | UX37-A |
| WP-UX37-2 | Unicode editor, grapheme/cell/source mapping, paste handling, security indexing, and real PTY | UX37-B |
| WP-UX37-3 | Bounded diff and complete source-view coverage | UX37-C |
| WP-UX37-4 | Progress, fake infer/apply, cancellation/recovery, non-TTY behavior, and existing-format recovery | UX37-D |
| WP-UX37-5 | Final common validation, measurement and manual procedures, screen documentation, independent review, and handoff | UX37-E and the complete ID trace |

Screen examples and key behavior follow ADR-0037 Section 3. While an editor has
focus, `j` and `k` are text rather than navigation commands. Display width is
kept separate from source bytes; arbitrary non-ASCII text, combining
characters, and emoji remain intact, while only dangerous control characters
are visualized. Diffs retain complete content and never present an end jump,
clip, or resized viewport as a complete source view. `NO_COLOR` and narrow
terminals still show the information required for approval. The boundaries
between unselected, transmitted, adopted, and finally applied states,
concurrent-edit detection, cancellation/recovery, and bounded I/O may not be
weakened. ADR-0036 communication UI may coexist when present, but ADR-0037 does
not add hook or communication access by itself.

Focused implementation and tests precede one heavy common-validation run
against final source. Run the 400-line checker before and after each subtask and
at handoff, together with product 1.85, Mock 1.95, stable, ABI 6, and full SDK
quality gates. Review A covers screen behavior, Unicode, keys, and colorless
display. Review B covers source identity, view coverage, resources,
cancellation, and restoration. The final report maps each ID to its assertion,
command, and result and records changed, untracked, and recovered files plus
measurement conditions. A short fake-fixture procedure exercises non-ASCII
entry, lists, colored diffs, `j`/`k`, `NO_COLOR`, and cancellation. Automated
gate success and human readability approval remain distinct; manual UI, remote
CI, and designer approval must be reported separately. Real AI communication,
changes to external applications, a new browser test base, SSH/publisher work, and
commit/push/publish/tag operations are outside the program.

The implementation connected the common screen, list, status/footer, Unicode
editor, and complete text shown before transmission. Large pastes, single-cell
insertion, and ordinary character sequences use bounded read units, while
ratatui renders cell differences. Real-PTY tests covered 15,000 characters,
original UTF-8 and transmitted-byte hashes, cancellation, and reset. The
bounded-LCS unified diff, byte views of both sources, line notation, and
colorless display were verified through real PTY adoption/apply. Local
acceptance passed. The 2026-09-10 ADR-0036/0037 designer review also passed with
zero findings at every severity. Human UI/IME and remote CI remain unconfirmed;
ADR-0037 Sections 6.1 and 6.2 and the
integrated validation record retains exact
limits and results. At handoff, 136 untracked files contained the complete
records and content for commit `1328aec75c7abf792ddfbc2c60340d4a1e60383a`.
The same review record contains the hook, writer, split-TUI-module, return-test,
and ADR-0037 inventories and comparisons.


## 26. Simplified Mock directives / model API work packages

Implement ADR-0038 and the approved SD-0 contract in the same Mock binary, IR, API, and project.
SD-A covers vocabulary, models, and seeds; SD-B the seven operations and relations; SD-C constraints and regex; SD-D formats, migration, two domains, and references; and SD-E full recovery, measurements, independent review, and handoff.
See ADR-0038 validation record Section 32. Current integrated local acceptance is consolidated here and in the ADR-0039 validation record.
The proposed 3/4–1/4 allocation of the after-observation budget
was not adopted.
The next step is ADR-0039. Acceptance of that new contract must not be inferred
from the successful ADR-0038 candidate.

Do not add a backward-compatibility reader or alias. Preserve existing
U/`.dist`/backup/proposal material, initialize a new empty directory, and
migrate manually.
Preserve B/U/N, edit protection, concurrent-change rejection, and rollback in the new format.
Create and exercise the copyable project described in `mock-project-workflow.md` through the same runner used by the common plan. Split the full Mock test workload into bounded 1.95/stable invocations and compare every subject with Cargo metadata.
Do not skip product Rust 1.85, ABI 6/SDK checks, VM termination and
cancellation, or worker-recovery coverage.
Record local release measurements, remote CI, unexecuted CI, and final human design approval as separate evidence categories.

## 27. Request-atomic Mock execution work packages

[ADR-0039](adr/0039-request-atomic-mock-execution-and-soft-hard-limits.md) and the dedicated goal define this follow-up.
RA-0 preserves the measured baseline; RA-A covers the request candidate, commit, and cancellation; RA-B covers Soft and Hard limits plus failed responses and diagnostics; RA-C covers formats and every CLI path; and RA-D covers final validation, measurements, independent review, and handoff.
The inventory began as Required / Not run. RA-0, RA-A through RA-D, and RA-01
through RA-18 have now all passed locally.
The 102-command common validation, same-release-binary measurements, and independent Reviews A and B with post-fix re-review are complete.
The rejected 3:1 observation-budget split is not reopened, nor is the superseded per-root commit contract retained.
Reuse ADR-0038's model, API, and limit contracts; supersede only the execution
semantics explicitly changed by ADR-0039.
Preserve safe recovery, generated ABI 6, regular MSRV, and edit protection.
Implementation evidence, corrected failures, RA-ID source and validation mappings, and unexecuted results are collected in the dedicated record.
Compare the resulting documentation diff with the implementation source while
preserving the existing index and recovery data.
The transfer is "Local Implementation Admission Completed / Designer Final Review Passed" (2026-09-14).
Remote CI, human UI/IME validation, and real AI connection remain unconfirmed. The designer-review record distinguishes re-execution from read-only review.

## 28. Mock scalar representation work packages

The normative contract is [ADR-0040](adr/0040-mock-scalar-representation-directive.md), and implementation scope is defined by the dedicated goal prompt.
REP-A covers vocabulary, AST, IR, and value inspection; REP-B identity, canonical generation, and non-destructive migration; and REP-C documentation, regular-product non-regression, final validation, and handoff.
REP-01 through REP-12 and Gates REP-A through REP-C Passed locally on 2026-09-15. Preserve the starting HEAD and existing changes rather than resetting to the design baseline.
It keeps the old management information, U/B/N, `.dist` seed, recovery data and records explicit transitions to the new directory.
Focused tests, the 102-command common validation, same-release-binary measurements, and independent Reviews A and B with post-fix re-review are complete.
Do not rewrite historical Passed evidence. Record current implementation results, important untracked artifacts, and environment failures in `mock-scalar-representation-validation.md`.
Reference-improvement research remained
pending when ADR-0040 was accepted and was not included opportunistically in
that work. Section 29 treats its later approval and implementation separately.
Handoff status is “ADR-0040 local implementation acceptance complete / Designer Review Passed (2026-09-15).” Remote CI, human UI/IME checks, and real AI connection remain unexecuted.

## 29. Mock reference topics and navigation work packages

The normative contract is
[ADR-0041](adr/0041-mock-reference-topics-and-navigation.md). It connects the
short text entry, full text/JSON, topic-part JSON, directive catalog, and TUI
navigation to one `ReferenceDocument` and presentation view. ADR-0040's
completion evidence remains unchanged. REF-01 through REF-15 passed locally on
2026-09-15.

| Work package | Scope | Primary acceptance |
| --- | --- | --- |
| WP-REF0 | Dirty-tree baseline; existing reference, CLI, TUI, and logical-identity inventory; ADR and line inventory | Preconditions for REF-01, REF-14, and REF-15 |
| WP-REF1 | Semantic directive catalog, presentation metadata, complete JSON v6, and rejection of old readers | REF-A; REF-03 through REF-06; REF-09 |
| WP-REF2 | Closed topic/name grammar, sub-30-line landing page, man-style list/detail text, and self-contained topic JSON | REF-B; REF-01, REF-02, REF-07, REF-08, REF-10, and REF-11 |
| WP-REF3 | `CHIGIRI-MOCK-REFERENCE(1)` list/detail TUI, Enter/Backspace navigation, `(END)`/`--More--`, existing scrolling/search, TestBackend, real PTY, and terminal restoration | REF-C; REF-12 through REF-14 |
| WP-REF4 | Context/proposal identity, document reuse, focused and full verification, measurement, review, and handoff | REF-D; REF-11 and REF-15 |

Implementation remains at or below 400 tokei Code lines under ADR-0059, with
tests excluded by role. Directive syntax comes from the semantic
schema; option values come from the executable and vocabulary enums. The
presentation layer stores only stable-name prose and positive/negative examples
that cannot be derived from those sources.

Topic text and JSON first select from the completed document and then render the
selection. An unknown `NAME` is rejected as a usage error before stdout is
written. With no topic, JSON output and `reference all --format json` are
byte-identical complete documents, and context/proposal identity always uses
the complete version. Every topic document contains the values needed for its
display and has no unresolved references.

The TUI receives only physical pages produced by the common presentation view.
It maintains separate state for the selected topic, item, detail page, list
position, text scroll, and search position. `all` opens the complete text in the
same viewer; Backspace restores the previous layer and selection. Text and TUI
share man-style headings, endings, and key guidance, without roff or an
external pager. Existing bounds, secure Unicode rendering, `NO_COLOR`, narrow
terminal input constraints, signal handling, bounded I/O, and terminal-device
recovery remain in force. JSON, `--no-pager`, and non-TTY output never starts
the TUI and never consults external `PAGER`.

Focused verification proceeds through directive drift/examples,
selection/text/topic JSON, TUI state, and CLI/PTY behavior. Final-source gates
cover Mock 1.95/stable, the ordinary 1.85/stable workspace, formatting, Clippy,
rustdoc, all canonical SDK exact/quality checks, ABI 6, and the common
validation suite. Automated checks do not substitute for human TUI/IME review,
remote CI, or designer review. Real-AI connections and publish operations are
outside this program.

WP-REF0 through WP-REF4 and Gates REF-A through REF-D received local
implementation acceptance. The 2026-09-15 designer review requested changes.
The 2026-09-16 re-review covered the TUI, directive-rule text, and the ordinary
`SEE ALSO` command. The remaining U+061C rendering issue was corrected with a
regression covering all 12 `Bidi_Control` characters. Implementation correction
and regression verification are complete; final design confirmation remains
pending as recorded by ADR-0041. See the
dedicated verification record.

## 30. Mock type-check batching and init performance

Apply [ADR-0042](adr/0042-mock-typecheck-batch-and-init-performance.md).
The direct change is limited to batch type checking of independent modules, removal of repeated common-definition loading, and init-phase progress display.
Do not change `luaur` third-party source or manifests.
Do not retroactively apply earlier programs' independent-review or distribution
gates. Verify all mock tests, formatting, Clippy, rustdoc, exact SDK checks, and
isolated-process measurements while confirming that generated output for the
existing SDL is unchanged.
Record execution results and unexecuted scope in the performance validation record.

## 31. Composable agent-ready Mock authoring work package

ADR-0044 replaces the unreleased embedded inference workflow with deterministic
primitives for external coding agents.

- `project/agent_guidance.rs` supplies the `AGENTS.md` created by `init`.
- `reference/topic_schema.rs` builds exact type and resolver topic documents;
  model selection remains in the existing topic builder.
- `cli/check.rs` owns text and `chigiri-mock-check/v1` output for full and target
  validation.
- `project/partial.rs` remains the shared target validator. It checks current
  selected bytes and computes whole-project readiness separately.
- `reference` writes directly to stdout; the CLI TUI and terminal dependencies
  are removed.
- Inference, Codex transport, proposal/adoption, and their validation targets
  are removed from the source tree and validation plan.

When extending this surface, keep target identity derived from the active
project inventory, keep diagnostics sanitized, and never equate target validity
with whole-project readiness.

## 32. Stateful Mock authoring work package

ADR-0045 is implemented as a clean replacement of the unreleased ADR-0044 Mock
formats. Maintenance work must keep these responsibility boundaries:

- `api/runtime.rs`, sandbox registration, checker declarations, and
  `reference runtime` are one executable capability contract.
- `ir`, `project/storage.rs`, public dispatch, reference visibility, and bundle
  rendering keep public schema and private storage schema roles separate.
- request snapshots, `clock.rs`, runtime provenance, seed reference expansion,
  and business rejection share the existing request budget and atomic candidate.
- project generation binds the capability version, clock, state, generated
  types, and both schema roles; export emits bundle v7 with resolved seeds.
- CLI/check and runtime logs use fixed structured diagnostic vocabulary and
  logical module paths. Raw compiler/VM text, source values, secrets, and
  physical paths are never public diagnostic fields.

The acceptance order is capability/checker parity, native watchdogs, request
and provenance, private storage/onboarding, clock/seeds/business rejection,
structured diagnostics/version rejection, then the fresh-project integrated
HTTP scenario. Run formatting, Clippy, rustdoc, full workspace tests, generated
contract checks, dependency/validation scripts, and the implementation-rules
checker after any affected stage and immediately before handoff.

## 33. Standard time and timeleap work package

ADR-0046 replaces only the ADR-0045 clock boundary. Keep the implementation
split into the following reviewable packages:

1. Pin native `os` behavior, numeric admission, the version inventory, and the
   TL-01 through TL-17 evidence map.
2. Install bounded `os.time`, `os.date`, `os.difftime`, and `os.clock` from the
   capability registry with checker, declaration, reference, phase, and native
   nil/error parity.
3. Capture one server policy per request before `request.before`, share its
   local override across VMs, permit mutation only in request preparation, and
   seal it before fields.
4. Provide a per-server host controller with serialized candidate execution,
   final cancellation fencing, publish-once success, startup-baseline reset,
   atomic request capture, and same-process server isolation.
5. Cut over project v6, bundle v8, API v7, capabilities v2, reference v8/topic
   v4, and metadata v7 through canonical generation. Remove `ctx.clock`, the
   clock reference topic, string fixed timestamps, and all old readers.
6. Exercise real HTTP requests through the in-process controller, run the full
   repository gates, map final evidence to every TL-ID, and obtain independent
   architecture/security and regression/workflow reviews.

World control is host-side and instance-local. Do not add an HTTP admin route,
special header, business mutation, CLI IPC surface, implicit world promotion,
or a force-System option. Preserve current-format transaction, `.dist`,
recovery, diagnostics, data rollback, and export-identity guarantees. The
validation record is `docs/timeleap-validation.md`; ADR implementation status
becomes Implemented only after its required evidence and both reviews pass.

## 34. Mock authoring boundary follow-up work packages

The implementation goal prompt
preserves the self-contained task, validation, and review instructions used for
this implementation.

[ADR-0047](adr/0047-mock-authoring-type-and-output-boundaries.md) is Accepted /
Implemented. AB-WP0 through AB-WP4 are complete, and the final-source evidence
map is in
Mock authoring boundaries validation:

1. AB-WP0: establish repository-owned reproductions and an AB-01 through AB-16
   evidence map; record the then-current source and version inventory.
2. AB-WP1: remove redundant record-slot intersections and supply checked query
   context for inline predicates, order arrays, indexed calls, and proven
   aliases, preserving negative checks and source mapping.
3. AB-WP2: carry host-owned stored-value origin through native projection and
   nested parents, preserving private-field secrecy, snapshot semantics,
   provenance checks, budgets, and atomic rollback.
4. AB-WP3: propagate precise sanitized diagnostics, resolve actual script
   locations, add named hook selection and executable guidance, and cut over
   log v3 and reference v9/topic v5 with exact contract fixtures.
5. AB-WP4: run fresh-project and second-domain HTTP/export/recovery acceptance
   plus repository gates; record final-source evidence and unexecuted scope.

The implementation adds no mock-side public-field override and does not
automatically rewrite source SDL. The sibling `graphql` project remains
motivation only, not a test dependency or corrective-edit target. The AB gates
were established by repository-owned fixtures and final-source validation, not
historical SA/TL evidence.

## 35. Chigiri brand and single-binary CLI work packages

Status: Accepted / Implemented. Apply
[ADR-0048](adr/0048-chigiri-brand-and-single-binary-cli.md); the exact evidence
and unexecuted environments are in the
Chigiri validation record.

1. CH-WP1 removed the dedicated Rust governance commands, APIs, formats, and
   gate while retaining ABI/layout/descriptor behavior and Mock update state.
2. CH-WP2 introduced the Rust-1.95 `chigiri_cli` adapter and sole `chigiri`
   product binary, with direct codegen/Mock library dispatch and a
   same-executable hidden writer.
3. CH-WP3 completed the package, import, format, operational identity, fixture,
   script, and documentation cutover and regenerated all eight owned Rust SDKs
   through the producer.
4. CH-WP4 ran relocated Rust and Mock workflows, closed CH-01 through CH-11,
   resolved two independent review tracks, and passed the 134-step
   final-source validator.

Normal libraries and generated consumers remain on Rust 1.85. The combined
CLI and Mock remain outside default members on Rust 1.95. Release packaging,
publishing, signing, installers, formal performance qualification, long fuzzing,
and unexecuted platforms are not implied by local implementation completion.

## 36. Mock authoring feedback work packages

The planned successor in Section 38 removes only completion tracking and its
dependent inventory contract at implementation cutover. Preserve this section's
completed evidence; do not rebuild removed inventory as a successor requirement.

Status: Accepted / Implemented. Apply
[ADR-0049](adr/0049-mock-authoring-modules-and-feedback.md). Shared `require`
and its dependent features have been withdrawn. The old release gate and
blocked handoff are historical. AF-WP0, AF-WP2, AF-WP3, and AF-WP4 are complete:
the public check-v3 and inventory-v1 surfaces, guidance, retained regressions,
measurements, two review tracks, and the complete local validator passed.

1. AF-WP0 reconciles the reduced scope, existing partial source/evidence,
   exact report fixtures/version inventory, and comparable release timings.
2. AF-WP2 completes terminal refinement, safe diagnostics, scoped reference
   navigation, and the unfinished-entry engine.
3. AF-WP3 exposes check v3 and inventory v1, updates generated guidance/help,
   and completes public CLI and integrated acceptance.
4. AF-WP4 runs retained regression and product gates, comparable release
   measurements, independent architecture/security and regression/workflow
   reviews, fixes and re-review, and records final source identity and limits.

AF-WP1 is retired. Keep flow handling, source mapping, diagnostic authority,
reference selection, descriptor enumeration, and CLI presentation as named
responsibilities. There is no module dependency probe or loader work to perform.
Run the implementation-rules checker before work, after each package, after
formatting, and before handoff. Use only unmodified, non-yanked crates.io
releases, exact direct dependency versions, and recorded registry checksums.
No added vendor, local patch, fork, Git dependency, copied checker internals,
or unsafe wrapper is allowed. Preserve first-party unsafe prohibition,
the two MSRV lanes, generated ABI 6, and existing ownership/recovery protections.

ADR Section 5 authorizes the public report changes without another staged
amendment. All other listed format identities remain unchanged; no project
migration, library sample, `runtime modules`, target expansion, or module export
is part of this work. Neither inventory nor target success authorizes serving.
Do not add a Mock test runner, daemon, persistent cache, or sibling dependency.

The active acceptance set is AF-08 through AF-11 and AF-14 through AF-16.
Those active rows pass locally on macOS. AF-01 through AF-07 and AF-12/AF-13
are retired, not passes or blockers.
Preserve earlier measurements and module probes under their original scope;
the reduced-scope evidence records final-source review and validation separately.
Do not wait for upstream multi-source APIs or start ADR-0050 automatically.

## 37. Portable host boundary and tiered-build work packages

Status: Accepted / Complete. [ADR-0050](adr/0050-portable-host-boundary-and-tiered-builds.md)
is implemented. The validation record preserves native validator snapshots,
focused closure evidence, artifact identities, and unexecuted runtime scope.

1. HOST-WP0 inventories platform imports, build commands, dependencies, tests,
   artifacts, and focused baseline reproductions.
2. HOST-WP1 adds the Rust-1.85 `chigiri_host` crate and migrates codegen to its
   portable path, secure filesystem, locking, process, and interruption facade.
3. HOST-WP2 migrates Mock project operations, child ownership, server signals,
   and the bounded common writer transport without changing formats or runtime
   behavior.
4. HOST-WP3 adds the Make/Cross/cargo-xwin build-host matrix, prerequisite
   preflight, separate native/distribution outputs, local artifact verification,
   and developer documentation.
5. HOST-WP4 completes focused regression, native Tier 1 full validation, Tier 2
   build/link evidence, both review tracks, fixes, and the validation record.

Keep OS branches and direct host imports inside sealed `chigiri_host` backends.
Tier 1 consists of macOS arm64 and Linux x86_64/glibc 2.17; both require native
complete validation and required CI lanes. macOS x86_64 and Windows x86_64 MSVC
are build-only Tier 2 targets, requiring release build/link and structural
evidence, not test-executable linking or native runtime checks. Never weaken an
atomic, no-follow, identity, locking, or process-lifetime guarantee to make a
Tier 2 operation appear supported; fail closed instead.

Native `release`/`debug` must leave existing `dist` bytes unchanged. Only named
distribution targets place candidates there, and Linux always uses the pinned
glibc-compatible environment. Four-target `cross` requires a prepared macOS
host. Keep local `verify-dist` structural/native Tier 1 smoke results separate
from full native Tier 1 gates, with foreign Tier 1 and all Tier 2 runtime checks
explicitly unexecuted with reasons.
Prepare `chigiri_host` package metadata and contents for future publication;
do not publish crates or require registry credentials in this work package.

## 38. Mock contract-validation simplification work packages

Status: Complete. Applied
[ADR-0051](adr/0051-mock-contract-validation-without-completion-state.md).
CV-01 through CV-09 have local macOS arm64 evidence in
the validation record. This did not
rerun or relabel ADR-0049's historical acceptance.

1. CV-WP0 captured current behavior, exact contract/version fixtures, and the
   mapping from superseded completion tests to retained safety regressions.
2. CV-WP1 removed resolver and hook completion state from generation, public
   candidates, static/runtime descriptor checks, bundle metadata, and reference
   types, then cuts over the formats specified in ADR Section 5.
3. CV-WP2 removed the unfinished inventory command/API/report and init/update
   completion counts, then updates canonical examples, guidance, quick start,
   and migration instructions. Add no replacement completion mechanism.
4. CV-WP3 proves CV acceptance and retained product gates, completes independent
   architecture/security and regression/workflow reviews, resolves findings,
   and records final-source evidence and unexecuted environments.

Keep module shape validation distinct from body behavior. Resolver descriptors
contain only `resolve`; hook descriptors contain only `run`. Generated resolvers
fail intentionally through the existing `ctx.fail` contract, and hook scaffolds
remain disabled no-ops. Validation must not invoke entrypoints to classify
completion. Missing/invalid active modules, failed initialization, bad seeds,
drift, cancellation, and resource violations remain admission failures.

Do not remove request-local state, patches, registration, structural file
inventories, ownership metadata, or hash-based update/recovery protections.
Old inputs are rejected before writes; migration uses fresh init and explicit
source adaptation. Keep check-v3 scope/readiness semantics and retained AF
refinement/navigation tests. Remove AF inventory requirements only from current
post-cutover gates, preserving their historical results.

Follow implementation rules before work, after each package, after formatting,
and before handoff. Regenerate owned fixtures through canonical producers.
Documentation-only publication does not change product examples or establish
test passes. Do not start ADR-0050, add dependencies, publish, stage, or commit
merely because this ADR has been accepted.

## 39. Adopted schema and local-generation work packages

Status: Accepted / Implemented; local handoff complete. Apply
[ADR-0052](adr/0052-adopted-schema-snapshots-and-mock-codegen.md); its AS-01
through AS-10 acceptance inventory supersedes only the source-authority and
generation-workflow portions of earlier Mock programs.

1. AS-WP0: capture current behavior, changed format surfaces, ownership rules,
   and focused normalization/source-authority reproductions.
2. AS-WP1: share the existing parser in a bounded normalizer; extend the snapshot
   with adopted public SDL; separate public freshness and internal identities;
   confine private sources and establish coherent project readers.
3. AS-WP2: reuse the protected generator and transaction for update/codegen;
   add read-only comparison and route project validation/runtime/export through
   adopted SDL. Cut over exactly the formats in ADR Section 8.
4. AS-WP3: project descriptions into reference, update generated guidance and
   migration instructions, exercise all AS cases, and record final-source
   validation plus resolved review findings.

Do not require local generation freshness before a writer can reconcile edited
inputs. Do require valid previous ownership. Project reference must remain
available with invalid authored modules/seeds, while refusing stale local
contracts and incomplete transactions. Never add an implicit external-source
fallback, compatibility reader, snapshot editor, or completion inventory.

This assignment is local macOS arm64 only. Focused boundary tests and the
affected Mock/CLI/reference/HTTP targets are recorded in the
ADR-0052 validation record. Remote
environments and the full-product validator remain unexecuted without changing
product support tiers. No staging, commit, remote execution, or release
publication is part of this handoff.

## 40. Trait-only Rust integration work packages

Status: Accepted / Implemented; local handoff complete. RI-WP0 through RI-WP3,
RI-01 through RI-08, focused quality gates, and both independent reviews passed
within the scope recorded in validation.
Apply [ADR-0053](adr/0053-trait-only-resolvers-and-application-boundaries.md)
as one ABI 7 cutover. Do not reapply historical projection/scaffold requirements
from Sections 12, 13, or 15 to the replacement field implementation model.

1. RI-WP0: inventory projection/scaffold surfaces, consumers, diagnostic hints,
   and owned outputs. Establish compile-positive/negative contracts for
   `ObjectResolver<C, T, E>` and mandatory `Resolvers::builder(mapper)` with one
   error type across groups. Preserve existing typed argument/parent/output
   shapes and group completeness.
2. RI-WP1: remove projection generation, public stages/setters, the specialized
   future alias, and the scaffold command/library API. Implement typed mapper
   retention in generated adapters, with one conversion per resolver-returned
   error. Keep core/runtime errors and completion outside that conversion.
3. RI-WP2: migrate handwritten consumers to complete trait implementations,
   regenerate all canonical output through ABI 7 producers, and preserve
   ownership/transaction protections. Update doctor guidance, maintained
   acceptance checks, large-consumer coverage, CLI help, and validation plans.
4. RI-WP3: update quick start and migration guidance around ordinary Rust,
   explicit domain mapping, one error policy, and existing context factories.
   Compile both named-function and captured-service closure recipes. Close
   RI-01 through RI-08 with focused evidence and review records. Existing
   closure records remain historical; new reviews use Beads.

The schema-wide error type is `Error + Send + Sync + 'static`; mapping is an
API-owned `Fn(E) -> FieldError + Send + Sync + 'static`. Register it once at
assembly start, require the same `E` for each group, and keep the finished
`Resolvers<C, T>` and runtime dispatch free of application error types.
An explicit identity function supports an application using `FieldError`.

Do not add output DTOs, automatic bindings, a replacement getter trait, macros,
domain annotations, source merging, a replacement scaffold, or compatibility
paths. Preserve execution/error semantics, per-request context ownership,
preparation rejection/internal masking, response hooks, and cancellation.
Application domain/RPC redesign and subscription support are separate work.

Validation uses local macOS arm64 and available MSRV/stable toolchains. Run
affected compile, generator/CLI, consumer, runtime/HTTP, exact, formatting,
Clippy, rustdoc, and implementation-rules checks within existing bounds.
The manual IDE smoke check is supplementary; record its versions and outcome
or mark it unexecuted. Do not require a full-product validator, remote Linux,
cross-build, distribution, load test, or fuzz campaign for this assignment.
Existing support tiers and historical evidence remain unchanged.

Executed implementation evidence is linked above. Record new review findings
and corrections in Beads under the [review workflow](review-workflow.md);
historical plans are not new platform or IDE evidence.

## 41. Subscription and bounded delivery work packages

Status: Accepted / Implemented with local macOS acceptance. Apply
[ADR-0054](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md).
Architecture Section 49 defines the target boundary; Conformance Section 41
lists SU-01 through SU-12. All six work packages have current focused evidence,
canonical/quality gates and both accepted whole-goal reviews. The
validation record maps
executed source-bound checks, final identities and excluded qualification.
Review decisions remain in Beads `lab01-uid.4.9` and `lab01-uid.4.10`;
historical publication evidence is not a pass.

1. SU-WP0: inventory current rejection, generation, stream/HTTP, context/hook,
   and Mock transaction boundaries. Before product code changes, publish
   reviewed concrete contracts, complete Rust/Mock examples, pinned protocol
   profiles/modes, authentication/terminal/header behavior, and a closed
   changed/preserved ABI/Mock format and migration table in ADR-0054. Preserve
   Section 5's accepted safety defaults/hard maxima while completing remaining
   finite internal bounds, scopes, and accounting. Keep current evidence separate;
   illustrative API spellings do not override the accepted numeric tables.
   Track and review this bounded package in Beads before proceeding.
2. SU-WP1: implement typed source initialization and sequential event execution,
   complete generated contracts/private adapters, fresh per-event preparation,
   existing error conversion and selection behavior, and neutral lifecycle.
   Preserve ordinary nested root-field contracts and core dependency limits.
3. SU-WP2: extend the one configuration graph with sugar/inject equivalence,
   common OR-composed evaluation, the accepted safety defaults and finite
   admission/queue/send controls, and fallible final configuration validation
   without panic or clamping. Implement server-only per-subscription overflow
   policy, cumulative metadata, and owned cleanup. Keep recoverable eviction
   distinct from rejection/termination.
4. SU-WP3: bridge the common endpoint to the three pinned protocols in both
   Hyper and Axum. Preserve application-owned listeners/routing, browser and
   authentication policy, bounded upgrade/control handling, multiplexed
   subscription isolation, and finite Query/Mutation behavior.
5. SU-WP4: add typed explicit Mock publish and subscription binding without
   retained VMs. Include pending publications in the complete request candidate,
   publish only after commit, isolate subscribers with fresh state/store/clock
   views, and preserve provenance, sandbox limits, restart, and seed reset.
6. SU-WP5: complete protected generation and the approved identity cutover,
   reference/guidance/export/migration, small Rust/Mock protocol walkthroughs,
   advanced Apollo metadata recipes, SU acceptance, and focused independent
   reviews. Record real source/client/protocol identities and unexecuted scope.

### SU-WP0 implementation freeze

ADR-0054 Section 9 is the complete preparation contract for the packages above;
this companion fixes the implementation cuts and handoff gates. Do not select a
different signature, bound, profile, or identity inside an individual crate.
The neutral Rust contract is generated ABI 8 with one complete subscription
resolver group, an owned `SubscriptionSource<T, E>`, setup once, and sequential
event resolution with a fresh prepared context each time. Event values are
`Send + Sync + 'static` because the generated `Future + Send` method borrows
them. The same GraphQL root
field retains its ordinary single-value resolver when that root object is used
as a nested output. `futures-core` 0.3.34 is the only new direct public stream
trait dependency and remains outside `chigiri_core`.

The shared controller must implement both Section 5 numeric tables and Section
9.3's completed internal inventory. In particular, it owns checked endpoint
byte reservations, endpoint/connection/setup permits, one maximum-sized encode
reservation before each source poll, per-subscription queued/in-flight
accounting, monotonic phase/age/progress timers, bounded control queues, fair
WebSocket turns, and one generation-keyed terminal cleanup owner. Every
WebSocket ID transfers atomically from pending setup to an active-operation
permit covering Query, Mutation, and Subscription; subscriptions also retain
the endpoint active-subscription permit. A five-second cleanup timeout moves
charged work into bounded quarantine and revokes emission but does not pretend
to preempt or release a blocked application future. SSE and multipart coalesce
to one pending heartbeat marker and reserve its frame only between whole event
entries. Protocol writers receive only
the controller's `Admit`, `Pause`, `EvictWhole`, `RejectNew`, or `Terminate`
decision and may not recreate limit policy. Endpoint clones share these owners;
separately built endpoints do not.

Official streaming workers use ADR-0054 Section 9.3's single completion-table
namespace with checked capacity `P + C * A + S + C`, reusing the admission-time
cleanup ticket for each generation. Local quarantine and the process reaper
exchange exclusive polling ownership of that same slot, not a copied record or
second orphan pool. The concrete join handle, physical permits, and byte owners
remain inseparable until actual join completion. The process-scope OS reaper
creates no Tokio runtime; endpoint shutdown drains only its own registration,
while final process drain rejects registrations, drains every endpoint, and
joins the reaper thread before application-owned runtimes are destroyed.
Unexpected final local/guard Drop must leave the record process-owned for
eventual join, never detached or prematurely uncharged. Poison retains owners
fail-closed and cannot count as successful drain. Polling and application-value
destruction occur outside locks. Optional runtime-specific facade integration
must not add Tokio to the neutral default or expose its types publicly.

Pending validation and active execution are sequential workers separated by a
full join and task-free atomic active transfer. Validation owns bounded ingress
but not the original timer futures, so an outside owner can enforce setup and
HTTP-overall expiry during synchronous coercion. Active setup and event
execution require the same independently enforceable monotonic budgets during
blocking application code; no timer is restarted at spawn or transfer, and idle
source waiting has no event-execution timeout. Before Phase A, HTTP preparation
reserves one maximum 64-KiB body/header result buffer and two exact fixed
500/Failed and 504/TimedOut callback candidates. Body bytes, UTF-8 header data,
and 16 bookkeeping bytes per header entry share that initial ceiling. Bundle
exhaustion returns provisional-finite 503 `SUBSCRIPTION_LIMIT_EXCEEDED` with one
Rejected observation, no Retry-After or eviction, exact pending/ticket release,
and zero validation/context/source/hook/completion work.

One armed HTTP initial completion remains inside the existing concrete source
task. Capacity-one command/result ports carry normal commit decisions and
checked returned parts, never an independently armed queued token. The
task-local ordered owner exists before initialization; context success moves
its completion there before source polling, and a new session moves there
before fallible deadline clears or hook awaits. Executor abort before setup
handoff must still dispose the source before the one precharged Failed attempt.
Early initializer failure honors an already selected terminal command rather
than dropping its token. The outer
owner polls the original Overall first, then linearizes Committing once;
completion outcome describes bounded preparation before synchronous callback
CPU and cannot be retroactively changed to timeout. The maximum reservation
covers callback execution and the result port; task-side pre-enqueue accounting
and outer receipt validation precede exact shrink. Oversize, arithmetic failure,
and callback/value panic produce a small closed failure signal and the reserved
masked 500 path without retrying completion.
The committed initial HTTP wire owner shares the existing exact-generation
active permit with source/cleanup, including an empty body. Acquire that lease
before fallible commit/shrink; a failed publication may not leave it in a
finite failure result. Source join alone never frees the slot while a wire
reference remains. The official supervisor must retain the same lease in each
body/event/control owner through last physical Drop, with no second permit.

HTTP source terminal composition consumes the matching pre-demand reservation
only after earlier event entries have physically finished. Errors fit that
reservation and the fixed control ceiling or fail closed without wire bytes.
SSE retains the current drop counter; multipart projects top-level transport
errors. A separate exact-reserved completion follows only after the previous
physical control drops. Natural completion reuses the source reservation or
reserves its known static length before encoding. Cancellation emits no control;
foreign reservations and unfinished events cannot become successful completion.

HTTP streaming uses one explicit application-polled aggregate over bounded
same-endpoint slots. Its bodies only hand off immutable physical frames; worker
and policy deadlines do not depend on body polling. Disconnect and demand-port
commit share one timeline, with wakes, source polls, observers, and response
destruction outside it, including unwind. Safe selected failures wait for prior
usable physical frames; retired event progress preserves the original deadline,
while accepted bytes retained physically are not a stalled send. Aggregate faults
produce safe body errors, not clean EOF. The application closes registrations,
drains the driver, and then drains its endpoint/runtime/process owners. Axum's
streaming service stays fallible outside infallible Router error conversion.

Mock's application-owned accepted TCP task inventory is checked from the same
effective endpoint graph as `HTTP_REQUESTS + S + C`; GraphQL request admission
and protocol/subscription quotas retain their own scopes. Explicit service
registration close must precede connection drain so retained service clones or
unpolled streaming bodies cannot keep the aggregate open. The application then
joins its connection/HTTP drivers, drains the endpoint guard while the Mock VM
worker remains available, stops that VM worker, and drains process supervision
before destroying its runtime, all under the one absolute shutdown deadline.

DropOldest pre-demand reservation never treats a virtual maximum as an actual
queued event. Encoding shrinks its original token to exact bytes immediately.
One candidate blocked by non-evictable physical bytes may wait without another
source demand, preserving queued survivors until capacity eviction can recover
admission. Queue expiry and send/shared-pressure deadlines remain independent.
Validated preparation metadata distinguishes finite bodies from selected-profile
initial errors: only the latter suppress Content-Length and exact size hints.
All empty/nonempty physical owners keep their charges through final Drop.

Fallible deadline/source commit checks precede exact byte transfer. Failed
shrink retains the maximum reservation for the masked finite response, without
new admission or another completion. If exact shrink remains unavailable, an
intact uncorrupted maximum token may own that bounded failure through last
physical Drop. Missing or corrupted physical ownership fails closed without
wire emission, records Failed once, and never publishes a source.

Timeout before Committing and true abandonment destroy the source/session in
that same task before one TimedOut or Failed completion attempt. Task-local
ordered Drop also covers runtime abort, separately containing source destructor,
callback, returned-value, and panic-payload unwind. The callback never runs in
outer Drop, under a lock, or on the reaper. A finite wire 504 has a separate
buffer owner from the late task's pre-owned callback inputs; abandonment moves
the unused buffer into the same retained cleanup generation. Blocking work keeps
the concrete join, timers, fallback charge, applicable buffer, and permits
through the five-second wait and quarantine until JoinReady and value disposal.
The same one-slot result port acknowledges terminal cleanup. Failure or missing
acknowledgment at local JoinReady selects the reserved masked 500/Failed without
retrying completion; acknowledgment after the five-second quarantine cannot
replace an already returned 504 or repeat observation. Abandonment emits nothing.
The official supervisor must drive initial deadlines independently of body
polling. WebSocket remains completion-free. Required real-worker proof includes a
dedicated synchronous blocked poll through the real five-second wait, last
Endpoint/registry/guard destruction without local cleanup polling, eventual
reaper join and exactly-once release, blocked setup/event deadline enforcement,
panic containment, exact slot capacity, multi-endpoint/runtime isolation,
restart/re-registration, and one-shot final process drain with an owned thread
join on both normal test completion and unwind.

Transport implementation is pinned to `graphql-ws` 6.4.1 at commit
`7281f1b8f09637ab383c1aab4d14247d683bc15d`, `graphql-sse` 2.6.3 at commit
`4cced455cb7a23beda9e9bf7a403aab270bd4e66`, the Apollo documentation deployment
`9d04abe4e703ef5acab42d0d`, and `@apollo/client` 4.3.5 at commit
`f3d28a55e5996429a57bf5367187bf8991464a89`. The bridge dependency qualification
uses the starting lockfile's Axum 0.8.9 and its `tokio-tungstenite` 0.29.0 line.
Only `graphql-transport-ws`, distinct-connections GraphQL-over-SSE, and Apollo
`subscriptionSpec=1.0` multipart are supported. Section 9.4 defines exact
negotiation, authentication inputs, headers, frames, heartbeats, cancellation,
terminal behavior, and unsupported modes.

Both WebSocket bridges share a borrowed opening-profile check before admission:
HTTP/1.1 GET, Host, Connection/Upgrade, singleton version 13 and canonical
16-byte nonce, and exact `graphql-transport-ws` selection. All offered
subprotocol tokens are unique across the combined request field lines; matching
and duplicate detection are case-sensitive. Syntax success is not a transport,
origin, CSRF, capacity, or initialization permit. The reviewed ecosystem library
still computes the RFC accept key and owns framing; the application owns HTTP
parser/header limits and handshake CPU budgets.

WebSocket `connection_init` authorization is a separate bounded neutral callback
before `connection_ack`; it sees only the admitted transport view and optional
bounded object. Its accept/reject/internal/timeout outcomes map to the frozen
4403/1011/4408 behavior. `RequestPhase` separately identifies finite HTTP,
WebSocket Query/Mutation, subscription setup, and subscription events. HTTP
streaming uses ADR-0054 Section 9.2's phase table for finite versus 200-stream
errors, source disposal, hook input/count, completion outcome, and masking.

WebSocket decoded-request and context-factory views use the existing Post and
GraphQlResponseJson logical execution policy, not the upgrade GET's safe-method
restriction. The retained raw transport method is still GET. RequestContext's
closed phase identifies one-shot/setup/event, with an empty raw body and no
HTTP response hook. This convention introduces no new hook SPI or implicit
admission, source validation, or media negotiation.
Operation-source resolution retains its envelope-only SPI.

Mock owns one typed binding per subscription and coordinate-specific typed
publication only from a write context. Publications are immutable members of
the complete mutation candidate. The commit winner transfers them in call order
to bounded host staging after the store swap; rollback and failed candidates
discard them. Fan-out and event VMs never run inside the commit fence, publisher
credentials/state never become subscriber input, and every subscriber event
uses a fresh read-only snapshot, clock-policy view, state, hooks, and budget.
Keep the root's published fields immutable while resolving declared relations
from sealed captured Records automatically through that fresh event snapshot.
Do not rebind the root to a current row or infer Record authority from DTO IDs.
Focused fixtures must change/delete/recreate a published root and independently
change its related records, covering both relation directions, missing targets,
private slots, read-only access, and consistent within-event snapshots.
The candidate owns one capped canonical typed/provenance capture and reserves
the exact dispatch slot/bytes before commit. Already allocated per-topic lost-
commit state uses a monotonic highest sequence shared by the topic, so repeated
losses catch generations registered between them without subscriber enumeration
in the store fence. Delivery metadata reserves
`extensions.chigiri.delivery.droppedTotal`; compatible sibling extensions are
preserved, the leaf is overwritten canonically, and non-object prefixes
terminate safely before final bounded encoding.

Implementation responsibility and its first proof target are fixed as follows:

| Package | Owning boundary | First required proof |
| --- | --- | --- |
| SU-WP1 | core validation; codegen contracts/private adapters; runtime/facade setup and event lifecycle | compile-positive/negative generated consumers plus deterministic setup-once, fresh-context, mapping, selection, and cancellation tests |
| SU-WP2 | facade configuration/quota owner/controller; neutral HTTP delivery values | construction edges, sugar/inject identity, evaluator tables/properties, reservation/timer races, DropOldest metadata, and cleanup |
| SU-WP3 | neutral HTTP streaming/upgrade SPI; Axum and Hyper adapters only | six small real bridge/profile combinations plus malformed input, heartbeat, slow sink, cancellation, and multiplexing cases |
| SU-WP4 | Mock IR/generation, candidate publications, post-commit dispatcher, topic index, and fresh event workers | real projects covering typed bind/publish, rollback/order, staging bounds, provenance, worker recovery, restart, and seed reset |
| SU-WP5 | protected writers, identity migration, reference/export/guidance, recipes, and final evidence | exact generation/ownership, old and mixed rejection, one-step project migration, all SU rows, quality gates, and two independent reviews |

SU-WP2 derives the shared-reservation pressure-wait ceiling from the validated
`max_queue_age`, arming it only after an actual maximum-event reservation
failure and clearing it only on success. It is distinct from idle source,
queue-residence, and send-progress timing; retries and heartbeats cannot extend
it. The event encoder reserves a fully charged private 16-byte JSON value slot
for `droppedTotal`. One synchronous queued-to-in-flight transition consumes the
opaque encoded owner and finalizes the current count with insignificant JSON
space padding, without another candidate, retained domain values, or partial
send mutation. Both details are specified in ADR-0054 Section 9.3 and require
deterministic timer and surviving-queue-entry fixtures, not only encoder unit
tests.

The identity cutover is indivisible: Rust ABI 7 to 8; Mock project 8 to 9;
bundle 9 to 10; host API 8 to 9; capabilities 2 to 3; full/topic reference
11/7 to 12/8; log 3 to 4; and update-state/schema-snapshot 9 to 10. Check,
directive, schema-sync, normalization, Luau-format, transaction, and recovery
identities remain unchanged as listed in ADR-0054 Section 9.6. Only `mock
update` may read a valid project v8 for one protected migration; every other
new executable reader rejects old or mixed identities.

ADR-0054 Section 9.6 also fixes log v4's four exact subscription record shapes,
generation/commit correlation, logical once points, typed terminal causes, and
privacy boundary. Use one shared 64-record subscription stdout queue, fair with
the separately reserved hook/access queues, and one added saturating
`log_loss.dropped.subscription` counter. Preserve the eight-entry stderr notice
queue, 16-KiB records, writer confirmation, and original shutdown deadline.
The closed internal observation owner must reach the common controller without
retaining a VM/payload or adding a public observer/configuration surface; no
serialization or logging occurs under commit/controller locks. Review this
exact refinement before executable logging cutover. Serializer, bounded-queue,
drop/terminal race, post-commit/rollback, privacy, and real Mock-wire fixtures
are required; existing `request_terminal` is not lifetime-log evidence.
The outside HTTP/WS owner must receive the existing-capacity setup decision
authority before initializer/VM work starts. Dispatch entry, cancellation, and
registration share one ordered fence; only the outside owner enqueues after
locks, registered setup before terminal, without waiting for blocked work.
Use ADR-0054's one typed decision-to-terminal mapping table, including distinct
execution versus delivery deadlines and an immutable winner through quarantine.

SU-WP1 may start only after the source-complete package receives the independent
architecture/security decision required by the review workflow and every
required finding is independently verified. The dedicated validation record
holds baseline commands, dependency evidence, and the SU assertion-to-test map;
Beads alone holds review status and findings.

No source initialization, publish, fan-out, or unbounded task may run inside the
atomic Mock store exchange. Never make a subscriber's eventual delivery failure
roll back a committed Mutation. Do not change completion-free validation or add
automatic notifications, scheduled scripts, external networking, persistence,
replay, acknowledgements, a client SDK, or a general Rust server.

Validate locally on macOS with small real applications and deterministic
clock/stalled-sink tests. Run focused affected compiler/runtime/HTTP/Mock,
canonical generation, MSRV/stable where applicable, formatting, Clippy, rustdoc,
and implementation-rule checks. Broad load/soak/fuzz tests, remote Linux,
cross-build/distribution, and a full-product validator are not required for
this assignment. This does not change support tiers or certify production
capacity. Keep all SU rows unexecuted until their actual evidence exists.

Use Beads for implementation packages, review rounds, findings, correction
evidence, and independent verification under the
[review workflow](review-workflow.md). Do not reopen the historical Markdown
ledger or claim an independent review from an implementer's own checks.

## 42. Subscription consumer ergonomics work packages

Status: Accepted / Implemented with local macOS acceptance under Beads
`lab01-8ph`. The validation record
records SE-WP1 through SE-WP4 evidence, final source artifacts and the scoped
independent reviews. Non-local platforms and broad qualification remain outside
this completed implementation assignment.
Contract: [ADR-0055](adr/0055-subscription-configuration-and-lifecycle-ergonomics.md).
Architecture Section 50 and Conformance Section 42 define the same target.
These packages improve Section 41's adoption surface without reimplementing
subscriptions or repeating its entire qualification campaign.

| Package | Responsibility | Focused proof |
| --- | --- | --- |
| SE-WP1 | Public associated bounds, shared validator definitions, and narrowly allowlisted numeric diagnostic evidence. | SE-01 through SE-03: unchanged defaults/boundaries, fit/overflow evidence, graph/origin parity, and redaction. |
| SE-WP2 | SDK-owned managed lifecycle, automatic fallible startup, Router-compatible safe failure mapping and thin native adapter integration. | SE-04 and SE-05: standard public route/hook consumers; runtime/startup/clone/failure races; no hook, Immediate and Graceful; finite and streaming work, deadlines, endpoint isolation and physical cleanup. |
| SE-WP3 | Whole-configuration sizing guide and a small native-router Axum Query/POST-SSE example. | SE-06 and SE-07: checked arithmetic/encoding, resolver-role and workspace/identity guidance, automatic cancellation, SDK-first native hooks and observable shutdown outcomes. |
| SE-WP4 | Focused quality checks, source-bound evidence and independent review. | SE-08: affected MSRV/stable compilation, tests, Clippy, rustdoc, formatting, implementation rules and diff checks; scoped Beads review. |

Implement in this order; guidance must use the actual public APIs. Do not copy
private maxima into a second registry or expose manual driver/guard/reaper
assembly as the ordinary integration. Required supervision starts automatically
under one SDK owner, never an untracked detached task or adapter-local policy.
Migrate affected internal users without duplicate manual/automatic driving.
Amend the initial closed-failure boundary only as specified: route-compatible
bounded masked failures, never infallible success, duplicate callbacks or an
uncharged fallback. Keep resource/protocol rules. The example uses protected
generation; do not edit canonical output or reintroduce scaffolding.

Use native server shutdown methods, not extension traits or a server-owning
`run`. No hook means cancellation on owner/runtime/process loss without a
coordinated drain guarantee. Immediate requests cancellation; Graceful uses
30 seconds by default, with a typed override and one SDK deadline that is never
reset between stages. Stop new GraphQL admission/demand and finish eligible
in-flight work, not the unstarted backlog. SDK drain precedes the native server
stop signal, so other routes can still accept requests during drain. Its budget
does not bound later server-wide shutdown or guarantee peer consumption.
Outcome inspection is optional; expiry/failure must never masquerade as drained.

This assignment uses bounded local macOS checks. Compile both bridges and
exercise changed lifecycle/upgrade paths across the existing protocols with
small fixtures. Do not drop prior safety assertions when migrating integration
tests; repeating every unchanged framing/property fixture, the full-product
validator, remote Linux, cross/distribution, load, soak or fuzz campaigns is not
required. Irreversible process shutdown fixtures must be process-isolated. Keep
independent review separate from tests, and do not use Beads, sibling reports or
historical evidence as acceptance oracles. Record actual source and unexecuted
scope without changing platform support tiers.

## 43. Distributed tracing propagation work packages

Status: TR-WP1 through TR-WP4 implemented / Focused local macOS acceptance. Contract:
[ADR-0056](adr/0056-distributed-tracing-context-propagation.md), Architecture
Section 51, and Conformance Section 43. This is a separate implementation
assignment from ADR-0055; use its shared lifecycle ownership rather than adding
a tracing supervisor, runtime, or adapter-specific task policy.

| Package | Responsibility | Focused proof |
| --- | --- | --- |
| TR-WP1 | Backend-neutral owned-scope SPI, one instrumentation slot and official opt-in OTel provider; capture/adopt the parent and separate standard suppression before handoff and request observation. | TR-01/TR-02/TR-08/TR-09: public registration, current/header precedence, parent-independent suppression, bounded carriers, legacy observers, dependency and ABI boundaries. |
| TR-WP2 | Direct/HTTP operation and provider scope activation across SDK task/poll/drop boundaries; suppression, sampling and ordinary tracing interoperability. | TR-03/TR-04/TR-06/TR-07/TR-09: actual downstream parentage, parent/suppression restoration, nested owning scopes, concurrency, ID failure, per-field opt-in, cancellation and terminal ownership. |
| TR-WP3 | Separate linked subscription-event traces, bounded retained setup SpanContext plus suppression and fresh event identity through existing protocol adapters. | TR-05/TR-06/TR-07/TR-08: both adapters/all profiles, suppressed setup/event scopes with or without a parent, publisher exclusion, metadata pressure and physical cleanup. |
| TR-WP4 | Maintained public consumer, migration guide, focused local quality gates and source-bound independent review. | TR-10 and integrated TR-01 through TR-09: actual source/dependency versions, MSRV/stable checks, in-memory recorder and small loopback downstream. |

Implement in order. Keep the instrumentation read-only, separate from user
plugins and observer slots; no generated edits, mandatory context field, backend
types in neutral signatures, or unbounded request-ID lookup. Context must be
active while application code runs, not only during observer callbacks.
Use standard propagators and per-poll activation, never a thread-local guard
held across await. Ordinary compatible tracing spans join the active OTel
context without requiring an SDK-created `tracing::Span::current()`.

Explicitly preserve standard telemetry suppression as bounded control separate
from SpanContext and sampling. Reconstructing a clean Context from the span
alone loses it. Check valid/no-parent and suppressed/unsuppressed cases, header
fallback, child handoffs, poll/drop restoration and fresh subscription events.
Suppressed scopes create no SDK backend spans but retain independent observer
delivery and ordinary GraphQL processing. Optional correlation loss must not
discard the suppression bit or copy arbitrary Context values to preserve it.

Preserve ADR-0055's deadlines, shutdown and actual worker ownership. Do not
retain live setup spans or arbitrary context values for subscriptions. Document
application-owned exporter shutdown, downstream carrier injection and explicit
propagation into user-created tasks. Sampling suppression and telemetry
capacity failure never become a new GraphQL business failure or force export.

Use bounded local macOS checks: affected tests/consumers and dependency checks,
MSRV/stable compilation where applicable, Clippy, rustdoc, formatting,
implementation rules and whitespace. Record actual source and unexecuted scope.
No remote, collector, cross/distribution, full-product, load/soak/fuzz or browser
campaign is required. Preserve support tiers, ABI 8 and all existing formats.
Tests use normative fixtures and an actual trace/link-aware recorder, never
Beads status or review prose. Track implementation/review evidence in Beads;
do not create a replacement Markdown review ledger.

## 44. Typed Mock event source and consumer work packages

Status: EV-WP1 through EV-WP4 implemented / Focused local macOS acceptance.
EV-01 through EV-12 have executable evidence and both independent reviews
accepted the implementation. See the
validation record.
Contract: [ADR-0057](adr/0057-typed-mock-event-sources-and-consumers.md),
Architecture Section 52 and Conformance Section 44. This is a Mock authoring and
delivery assignment, not a new general Rust broker or transport implementation.

| Package | Deliverables | Primary gates |
| --- | --- | --- |
| EV-WP1 | Freeze SDL-type selectors, destination-owned source/publish contracts, consumer configuration/modules, phase authority, finite limits and the exact clean-cutover matrix. | EV-01, EV-05, EV-12: generated/runtime contract negatives and no compatibility path. |
| EV-WP2 | Shared captured-event ownership, per-consumer progress, bounded source queues and the renamed post-commit pub/sub path. | EV-02, EV-08 through EV-10: broadcast, all-topic bounds, provenance, isolation, pressure and loss metadata. |
| EV-WP3 | Active independent sequence timers and fresh transactional named-consumer execution with immutable causal depth. | EV-03, EV-04, EV-06, EV-07, EV-11: timing, rollback, republish, continuation and actual cleanup. |
| EV-WP4 | Init/codegen/check/reference/export, maintained examples and coordinated fixture migration, focused validation and independent review. | Integrated EV-01 through EV-12 with actual local source-bound evidence. |

Keep scheduling and waiting in the existing host lifecycle; no retained Lua
closure/VM, per-event detached task, loader, network permission or custom checker.
Consumer writes are explicit independent candidates. Pending child publications
become visible only after successful commit; depth-4 attempts poison the whole
candidate. A failed consumer cannot roll back its publisher or silently retry.

Treat typed pre-execution queues and encoded post-execution queues as distinct
budgeted stages. Enforce OR-based admission and release actual retained ownership
on terminal paths. Do not pause sequence clocks for slow consumers, count skipped
ticks as queue drops, or drain recursive chains during Graceful shutdown.

Use current adopted schema and private-storage authorities; never hand-edit
generated output or auto-rewrite authored Luau. Require fresh init for migration
and remove old API names/readers, including the old protected-update exception.
The current workflow and producer-owned fixtures use the coordinated
ADR-0057 version matrix. Earlier fixtures remain rejection inputs only.

Run focused local macOS checks, controlled-time tests plus a small real timer
smoke case, actual loopback GraphQL profiles and transactional model assertions.
Complete applicable Mock/CLI quality, ownership and dependency checks; no remote,
full-product, cross/distribution, collector/broker or broad load campaign is
required. Use normative fixtures as test oracles. Track implementation, review
findings and independent corrections in Beads, preserving historical identities.

## 45. Tiered validation and weekly qualification

Contract: [ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md).
Architecture Section 53 and Conformance Section 45 define the same boundary.
Use focused checks while editing, `./scripts/validate.sh` for ordinary
fixed-toolchain handoff and `./scripts/validate.sh --profile full` for the
preserved complete product catalog. Routine completion does not require
repeating stable and broad optimized checks or the long campaign.

Both native Tier 1 PR jobs and their strict aggregate remain required. Weekly
CI runs both full profiles plus six finite long fuzz jobs and isolated load/soak,
with failure evidence retained. Additional execution follows actual input,
concurrency/retention and hot-path risks. First qualification retains the full
formal conditions and independent performance review under the runbook.

Verify profile selection, unchanged full inventory, mandatory fixed functional
and optimized safety coverage, expected-profile receipts, missing/failed/skipped/
cancelled commands and source mutation. Measure command durations rather than
assuming a speedup. A workflow inspection does not establish hosted CI success.
Record actual local/hosted scope and evidence in Beads. Implementers record
Fixed; the separate independent review session owns Verified finding closure.

## 46. Craftguild standards adoption

[ADR-0059](adr/0059-craftguild-standards-adoption.md) amends common CLI
presentation, prepared runtime SBOMs, source measurement, make entries,
version ownership and project licensing. It preserves ABI 8, namespace
machine formats and the two MSRV lanes. The catalog retires the two build-tool
regression steps under ADR-0059's test-ownership clarification; all ordinary
product behavior and generated-ownership gates remain.

Validate actual copied-binary offline help/version/SBOM, write/flush failures,
actual selected target/profile/features/compiler identity, embedded esbuild
contributions and asset notices, one owned staged install,
Code roles and canonical ownership reproduction. Keep host qualification and
remote CI separate from local tests. Release version-edit checks use shared
APIs in memory; runtime Git identities use explicit metadata fixtures without
creating commits or signing keys. Real path-Cargo integration remains required. Product release,
version/note application and Git publication remain deferred to the user.
