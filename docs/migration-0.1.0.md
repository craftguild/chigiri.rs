# Migration to Chigiri 0.1.0

This guide migrates pre-release development sources to a dedicated generated
crate, ABI 8, and the manifest-only CLI. There is no execution-compatibility
layer for old generated ABIs, modules, CLI commands, configuration, presets,
duplicate builders, or rate shims. The
trait-only resolver validation record
records the historical ABI 7 cutover; the
subscription validation record
records the current ABI 8 implementation, local acceptance and unexecuted qualification.

## Generated crate and CLI

1. Prepare the pinned engine and locked prerequisites from [Building Chigiri](building.md), then build the unified CLI with `make debug` and add `target/<host-triple>/debug` to `PATH`. Generation also requires the fixed Rust 1.95.0 formatter. Use `chigiri rust --version --format json` to inspect build information.
2. Preserve every pre-cutover manifest and generated tree. Run `chigiri rust init` with a new `chigiri.toml` path, then edit that new manifest. Specify `schema_version = "chigiri/config-v2"`, schema directory, package, output, and runtime. Relative paths are resolved from the manifest directory. For ordinary path development, set the runtime `root` to the Lab01 root. Git sources must use a real URL and a complete lowercase 40-character commit, and the application's Chigiri dependency must use the same source.
3. Run `codegen` into a fresh dedicated output separate from every old generated tree. `update` rejects unrecognized or mixed ownership markers. A verified ABI 7 standalone crate with the retained marker format can instead use protected `update` as described below. ABI 6 and older trees require a fresh destination. In a workspace, place the SDK outside the member directory and list it in the parent workspace's `exclude`. The entire SDK is generated output; do not mix in Cargo.lock, caches, or handwritten consumers.
4. Add an ordinary SDK path dependency to the application's Cargo manifest and replace `mod generated` with a dependency reference. Set `[consumer]` values for `cargo_manifest`, `package`, and `generated_dependency` to match the application. `generated_dependency` is the Cargo dependency alias. The generated crate must not depend on the application or domain crate.
5. Move the old `[bindings].domain-enums` setting to `[bindings].domain_enums` and list the SDL enum names. Implement each bound enum's domain-related type and required `*_from_graphql` / `*_to_graphql` conversions on the application-local `Types`. Remove the former approach of implementing generated traits directly on external domain types. The [three-crate example](../crates/codegen/tests/fixtures/sibling-enum/consumer.rs) demonstrates the bidirectional connection.
6. Implement every root/object resolver trait as `ObjectResolver<C, T, E>`. Pass one explicit error mapper to `Resolvers::builder(map_app_error)` and register each group in SDL order. Use SDK crate-root traits and types. Projection builders, setters, `ResolverFuture`, and `rust scaffold` are removed; ordinary trait methods may appear in any order.

```sh
chigiri rust --manifest chigiri.toml codegen
chigiri rust --manifest chigiri.toml check
chigiri rust --manifest chigiri.toml --format json doctor
```

`codegen` is only for output that does not yet exist. For later SDL changes,
`update` creates and compiles a new candidate before replacing the installed
output. `check` performs a read-only exact comparison with the current
renderer. `doctor` additionally checks the real consumer with `cargo check
--locked`. A difference in CLI build information alone does not restamp output.
The retired schema snapshot/diff facility has no Chigiri replacement; use
generated ownership/exactness checks and application-specific schema review.
For unknown files or remaining recovery material, follow the
[recovery guide](generated-crate-recovery.md).

## ABI 7 standalone crates to ABI 8

Preserve the current owned tree and run `chigiri rust update` using its existing
manifest. The writer verifies the old ownership inventory and compiles the new
SDK independently of the still-unmigrated application. Modified or unknown
output is protected; inspect it and follow the recovery guide instead of
editing ownership hashes. `rust codegen` continues to require a fresh output.
ABI 7 is accepted only as verified ownership provenance for replacement, not
as an execution adapter or retained renderer. Exact checks require ABI 8;
ABI 6 and earlier ownership markers are not accepted by this update reader.

Preserve complete ordinary resolver traits and their error parameter. Use one
`Error + Send + Sync + 'static` type for all groups. Replace projection chains
with complete ordinary methods and register an API-owned synchronous mapper.
For `FieldError`, pass `std::convert::identity::<chigiri::FieldError>` explicitly;
for `Infallible`, use `|error: Infallible| match error {}`. Keep domain errors free
of Chigiri dependencies. Normal `?` uses normal Rust `From` conversions.

For a subscription root, implement every generated `subscribe_<field>` source
initializer and `resolve_<field>_event` event method with the SDL's exact owned
output shape and the same application error type. A source owns its
`Send + 'static` stream; it cannot borrow setup context or arguments. A root
type used in a nested position additionally requires its ordinary single-value
methods. Do not replace these roles with aliases or output DTOs. The
[executable walkthrough](subscriptions-walkthrough.md#typed-rust-resolver-assembly)
shows the complete generated-contract consumer and application-owned bridges.

Run `rust check` for exact output and `rust doctor` for consumer compilation,
then implement any required methods or groups exposed by an SDL change. No
application-source merge or replacement trait scaffold is provided. See
[Rust integration](rust-integration.md) for error disclosure and context recipes.

## Mock project and portable bundle cutover

The closed [ADR-0054 identity table](adr/0054-graphql-subscriptions-and-bounded-stream-delivery.md#96-closed-identity-and-migration-table)
defines project v8 to v9, portable bundle v9 to v10, host API v8 to v9,
capabilities v2 to v3, full/topic reference v11/v7 to v12/v8, runtime log v3
to v4, and update-state/schema-snapshot v9 to v10. Check v3, directive
vocabulary v3 and the listed normalization, formatting, schema-sync,
transaction and recovery identities are unchanged.

Only explicit `chigiri mock update --config-dir <project>` may read an otherwise
valid, ownership-verified v8 project for this one-step protected migration.
It preserves user-owned resolvers, hooks, storage, seeds and adopted public
SDL while generating the complete v9/v10 candidate and new subscription
modules through the existing transaction. Signature conflicts do not authorize
overwriting user edits. Other commands reject old or mixed identities. There
is no portable v9 compatibility loader or in-place bundle conversion.

After update, run `check`, inspect the generated reference and produce a new
portable export. Older or unrecognized projects use a separate fresh
destination; preserve the original files and move application-owned content
deliberately. See the [Mock workflow](mock-project-workflow.md#fresh-destination-migration)
for exact commands and recovery boundaries. Local `mock codegen` continues to
use adopted public SDL; only explicit public `update` adopts upstream changes.

## Endpoint and providers

- Applications enter through the `chigiri` facade and the selected Hyper or Axum bridge. Build a single `chigiri::Endpoint::builder(resolvers)` so the safe defaults for structural limits, caching, byte limits, deadlines, CSRF, masking, and visibility are applied.
- Declare application-specific rate identities with `chigiri::rate::transport(...)`, `operation(...)`, or typed injection. Available policies include token bucket, fixed window, sliding-window counter, and a queue-free leaky-bucket meter. The independent provider SPI is `chigiri_rate`; the official in-memory provider and clock are in `chigiri_rate_memory`. Use `ManualClock` and `new()` / `with_config(config)` / `with_clock(config, clock)`.
- The application owns listeners, routing, connections, signals and native server shutdown. Official managed Axum/Hyper services automatically own GraphQL supervision and recovery; pass the same immutable endpoint to direct and native integrations. Readiness exposes `SchemaInfo`, rate coverage, and in-flight counts, then reevaluates operation impact.
- A built schema transfers only schema, resolver, scalar, and catalog identity. Move plugins, samplers, observers, ID factories, and similar state into one endpoint-builder configuration or typed `inject` entry.
- In prefork deployments, the parent may edit the builder, but each worker must call `build()` and create its own request-ID factory after the fork. Do not share a built endpoint or the official random ID factory across processes.
- `EndpointBuildError::InvalidConfiguration` no longer requires string parsing. Consume its redacted `ConfigurationDiagnostic` through `slot`, `origin`, and `violation`.

For managed subscriptions under ADR-0055, replace ordinary
`streaming_service` service/driver/guard tuples, per-upgrade drivers and final
process-drain calls with `chigiri_axum::service(endpoint.clone())?` or
`chigiri_hyper::managed_service(endpoint.clone())?`. Construct the service inside
the existing supported runtime and handle construction errors. Existing finite
handlers remain valid. No generated ABI or Mock/CLI identity changes accompany
this lifecycle migration.

Use `endpoint.shutdown_on(application_signal, ShutdownOptions::graceful())`
with the native server's shutdown method when coordinated SDK-first drain is
wanted. The 30-second default/typed override bounds only SDK waiting; Immediate
requests cancellation, and no hook means no coordinated notification. Inspect
the optional sticky outcome when needed, without another SDK await. Initial
closed infrastructure failure is a safe finite 500, using an existing charged
failure or an allocation-free bodyless response; committed abnormal failure
uses protocol/body/connection termination. See the
[maintained native application](../examples/subscriptions/README.md) and
[sizing/workspace walkthrough](subscriptions-walkthrough.md).

The [books example](../examples/books/README.md) and Axum adoption are executable references. Routine product verification is centralized in `./scripts/validate.sh`; formal performance, load, and long-running fuzz work uses the independent gates in the [production validation runbook](production-validation-runbook.md). Published packages, SSH access, and commits are not prerequisites for routine verification.
