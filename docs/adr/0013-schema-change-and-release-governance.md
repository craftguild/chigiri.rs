# ADR-0013: Schema change, testkit, and release gates for production readiness

> 2026-09-06 amendment: [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md)
> supersedes the publisher/package/fresh-clone portions that define completion
> criteria. Existing execution semantics, test facts, and historical Passed
> records remain unchanged. ADR-0026 defines the revised generation boundary.

- Status: Accepted
- Implementation: Planned
- Date: 2026-08-27
- Last updated: 2026-08-28
- Decision owners: Pythia maintainers
- Depends on: ADR-0008 through ADR-0012
- Revisits: ADR-0002, ADR-0004, ADR-0006, ADR-0007

## 1. Context

Production readiness requires more than the existence of features such as rate
limiting and observability. The project must distribute crates, generators, and
generated ABIs reproducibly without breaking compatibility, and must
continuously verify untrusted-input and cancellation behavior.

The repository already checks generated drift, GraphQL conformance, large
consumers, fresh clones, and independent review. It does not yet unify semantic
schema diffs, registered-operation impact, reusable endpoint/bridge/provider
testkits, and the security release process.

## 2. Decision

### 2.1 Two-level compatibility check

A schema rollout evaluates two distinct concerns:

1. schema-wide semantic diff
2. registered operation impact

The schema-wide diff compares the old and new validated semantic descriptors.
SDL file order, descriptions/comments, equivalent built-in spelling, and
generated shard boundaries are not API changes.

Changes use a closed classification:

| Classification | Examples | Default gate |
| --- | --- | --- |
| Breaking | remove a type, field, argument, input, or enum value; make an input required; make an output non-null from nullable; change named/list shape | fail |
| Dangerous | add an enum value or union possible type; change a default; add deprecation; make an output nullable from non-null | review required |
| Non-breaking | add an optional field or argument; change a description | pass |
| Operational | change cost, cache, or policy metadata | policy review |

The complete type and directive rules are a machine-readable test table keyed
by GraphQL specification version.
If the tool does not understand the snapshot version, it fails closed instead
of classifying the change as non-breaking.

When an application configures ADR-0010's `OperationCatalog`, registered-operation
impact evaluates every operation in that catalog by parsing, validating, and
selecting it against the new schema.

- validation success
- operation kind/name/identity integrity
- selected-field and variable/input-type compatibility
- cost and policy-classification changes
- result-shape change metadata

A schema-wide Breaking change does not automatically pass merely because no
registered operation is affected. The two inventories answer different
questions. Conversely, a schema-wide Non-breaking change may still fail an
operational gate if operation cost or quota classification crosses a configured
threshold. An application without a catalog receives only the schema-wide diff,
with operation impact marked `Not evaluated`. Pythia neither discovers nor
invents client documents used by the application.

### 2.2 CLI artifacts

The CLI adds schema-governance commands separate from resolver generation:

```text
cargo pythia schema snapshot --schema-dir graphql --output schema.snapshot.json
cargo pythia schema check --schema-dir graphql --against schema.snapshot.json
```

A snapshot is canonical JSON containing the format version, Pythia version,
GraphQL specification version, schema digest, and semantic API descriptor. It
excludes absolute host paths, full source text, generated Rust, and application
configuration secrets.

`check` emits a human-readable summary and a machine-readable JSON report with
fixed exit codes:

- 0: policy passed
- 1: compatibility/policy failure
- 2: command/config/input/tool failure

The baseline is never downloaded implicitly from a remote URL; CI supplies it
through an explicit file or provider. Operation-impact analysis consumes the
application-built `OperationCatalog` rather than imposing a client document or
manifest format. Rust tests can pass that catalog to `pythia_testkit`'s
validation API.

### 2.3 Immutable compiled schema rollout

Pythia does not include runtime SDL hot reload in the production baseline that
uses compiled generated Rust traits and descriptors.

- One `ExecutableSchema` has an immutable schema digest.
- Schema changes involve generate, compile, test and binary deployment.
- During rolling deployment, the old and new binaries will each publish their own digest with event/readiness metadata.
- The application-owned server uses graceful shutdown/drain to retire old
  instances.
- An application may atomically swap multiple schema instances, but Pythia does
  not provide an automatic file watcher/reload contract.

The application routes its readiness endpoint. Pythia provides read-only
`SchemaInfo` containing version, ABI, schema digest, and optional operation
catalog revision.

### 2.4 Reusable testkit

An independent `pythia_testkit` provides reusable support rather than
example-only helpers:

- runtime-neutral GraphQL request/response assertions
- common lifecycle conformance suites for neutral, Hyper, and Axum bridges
- ADR-0010 operation store provider conformance
- ADR-0011 rate-limit provider conformance with fake clock
- ADR-0012 observer/redaction/cardinality assertions
- old/new engine structural differential comparator
- deterministic pending/drop/cancellation probes

ADR-0007's differential comparator moves from an example to the public testkit.
It uses an explicit leaf-only allowlist and does not weaken existing
single-consumption and unused-entry rejection. The testkit is a development
dependency, never a production dependency, and is not exposed as a prelude.

### 2.5 Security and robustness verification

In addition to ordinary tests, the release branch includes:

- coverage-guided fuzz targets for the parser, SDL composition, HTTP decoding,
  JSON conversion, persisted IDs, and configuration parser;
- adversarial tests for recursion, allocation, diagnostics, bodies, caches,
  queues, and key cardinality;
- endpoint timeout/cancellation/drop tests and an application-example graceful
  drain integration test;
- contract tests for malformed provider and observer implementations; and
- load/soak tests with memory and concurrency-limit observation.

Load and soak workloads create requests with a standard JSON encoder, decode
GraphQL responses, and verify expected Query and Mutation data shapes rather
than checking HTTP status alone. An HTTP 200 response containing `errors` fails.
Evidence records attempts and successes separately for Query and Mutation and
counts each GraphQL error code. Soak Mutations use a fixture whose state growth
remains bounded while resolvers execute. Load Queries require exact ID, title,
and author matches for seeded books; soak Queries require either the same
initial value or a known value after a bounded Mutation. Empty arrays, missing
fields, and unknown values do not count as success. A fixed-bucket histogram or
bounded reservoir stores latency distribution, while total requests and total
and categorized errors use independent counters. A percentile above the largest
bucket is not converted to a guessed upper bound: its upper bound is null and
`greaterThanMs` records the largest bucket boundary.

The RSS sampler fails closed. Evidence records sampler errors, raw samples,
sample count, expected count, elapsed time, time coverage, and the baseline
interval. It requires zero errors, at least ten samples, and at least 80 percent
coverage. Initial RSS is the median of at least five samples from a steady
interval after warm-up. The verifier recomputes baseline median, peak, and
increase from raw samples.

ADR-0058 now controls execution frequency: weekly and relevant high-risk
qualification run long fuzzing; ordinary releases use the candidate's normal
profile and change-specific additional checks. The first qualification and
major releases retain the complete campaign. When long qualification runs, it uses the
fixed corpus and a sanitizer build for at least 30 minutes per target. Discovered
crash, hang, and out-of-memory inputs are stripped of secrets and committed to
the regression corpus. Sanitizers unavailable on the Rust/MSRV stable toolchain
remain a separate gate with the applicable nightly toolchain recorded; they are
not forced into the stable job.

Benchmarks track the following rather than deciding solely from absolute
throughput, which varies greatly by environment:

- parse/validate/cache hit latency
- small/large operation execution overhead
- Resolver count/schema size compile time and generated bytes
- idle/request peak memory
- observer/rate/persisted functional overhead of disabled/enabled

The operation profile includes real catalog-digest lookup and safelisting. The
rate profile includes Stages A and B with a real provider. The observer profile
uses the production ID, sampling, and bounded chain. A compound
`facade-composition-enabled` profile measures the single `Endpoint` component
graph. JSON records each profile's exact include/exclude set. Measurement order
alternates forward and reverse for each sample. The compound profile may add at
most 50 percent overhead relative to the disabled profile.

A profile with an observer queue drains the `ServerBuilt` event before
measurement and fully drains the queue outside each timed sample. Each successful
request must produce 29 lifecycle events, zero error events, and zero drops.
Evidence records capacity, expected and accepted event counts, and separate
lifecycle/error drop counts. A count mismatch or any drop prevents evidence
generation.

Separate from pass/fail profiles, evidence records medians for
`diagnostic-noop-observer`, `diagnostic-queue-only`,
`diagnostic-random-request-id`, `diagnostic-telemetry-clone`, and
`diagnostic-full-observer`. A telemetry profile using a fixed request ID excludes
request-ID derivation and sampling; comparison with the seeded keyed-hash profile
isolates the contribution of ID generation and telemetry cloning. Diagnostic
values do not directly add fixed pass/fail thresholds, but profile definitions,
finite positive medians, and queue-delivery evidence are verified as closed
contracts. Benchmark RSS records attempts, samples, errors, and coverage. If any
required attempt fails, no evidence is produced.

Each formal benchmark campaign records a baseline, hardware/toolchain, tolerances, and regression
justification. Formal performance evidence is built from a clean isolated HEAD
and records revision, dirty state, measurement-binary SHA-256, and complete
Cargo/Rustc information for the explicit build toolchain in JSON. The toolchain
is a resolved immutable version such as `1.95.0`, not a moving alias such as
`stable`, and the same version is explicit during build, measurement, and
verification. The formal runner fixes actual Cargo and Rustc paths with
`rustup which`. It rejects ambient compiler overrides such as `RUSTC`, wrappers,
and `RUSTFLAGS`, as well as Cargo home/configuration, incremental, profile,
target, and native-compiler flag overrides. A global Cargo configuration in the
default Cargo home or an ancestor outside the repository also fails closed.
Ordinary compatibility testing on the current stable toolchain remains a
separate gate from performance comparison.

Evidence verification requires exact agreement with values recomputed from the
same binary. For an initial release, baseline and candidate use the same
revision, binary digest, hardware, and toolchain. Baseline approval records an
identifiable human approver, UTC approval time, and a concrete explanation that
the measurement conditions and values were reviewed; generic role names or
template reasons are rejected. The verifier does not trust a self-declared
`passed` field. It recomputes load/soak fixed conditions, counters, histogram
percentiles, memory, RSS, readiness, exits, all benchmark medians, overhead, and
regressions from raw fields, including finiteness and non-negativity. Profile
include/exclude sets must match exactly, not merely by profile name. Baseline,
workload, and benchmark top-level and nested objects are closed contracts that
reject unknown or missing fields.

All evidence includes a valid Unix timestamp, OS version, architecture,
positive logical CPU count, hardware descriptor, and actual command arguments.
The verifier rederives mode, toolchain, sample/iteration or parallelism/count,
and duration from the command. GraphQL responses must be top-level JSON objects;
arrays, null, scalars, and malformed `errors` shapes are classified as
`malformed-response`, not harness exceptions. Integer fields reject JSON floats
and booleans. Source identity includes the source revision's Git commit Unix
time and must match history exactly. Measurement time must follow the commit and
must not be more than five minutes in the future. Baseline approval records both
a canonical UTC string without fractional seconds
(`YYYY-MM-DDTHH:MM:SS+00:00`) and the same second as
`acceptedAtUnixSeconds`, within 24 hours after measurement.

Baseline comparison mode is closed to two values:

- `initial-candidate`: baseline and candidate revision, binary SHA-256, and
  toolchain match exactly.
- `previous-release`: revision differences are allowed, but the input is the
  immutable previous-release `production-benchmark` binary matching the SHA-256
  in the accepted baseline. The candidate is built with the same immutable
  toolchain recorded by the baseline, and hardware matches. An update to the
  `stable` alias after baseline creation cannot change the comparison version.

The evidence directory retains the load/soak binary and initial baseline
benchmark binary so evidence remains verifiable after the isolated worktree is
removed. Commands record runtime paths, but verification uses binary basename
and source SHA-256 as the authority, permitting artifact relocation. A stored
baseline binary is mandatory for previous-release comparison. After comparing
against the previous release, each release separately records and receives human
approval for the current revision's baseline, storing accepted JSON and its
matching binary together at an immutable revision-specific path. The next
release compares against that stored pair rather than a mutable fixed root path.
Before measurement, the runner rejects comparison input and new baseline output
that resolve to the same path, symlink target, or hardlink inode.

Adding observer-queue delivery and diagnostic profiles to the closed contract
raises the performance-evidence format to version 5. Because this repository has
not shipped a production release and the user explicitly declined backward
compatibility, no compatibility shim accepts format 4 or earlier, even for
diagnostic material.

### 2.6 Distribution and supply-chain gate

A production candidate provides:

- one release note containing the workspace crate version, CLI version, and
  generated-ABI compatibility table;
- a verifiable tarball from an immutable registry artifact such as crates.io, a
  signed Git release, or the same source revision as a local candidate;
- a fresh consumer that builds and tests `--locked` from the package tarball;
- a changelog, migration guide, MSRV policy, and deprecation period;
- `SECURITY.md`, a vulnerability-reporting channel, supported versions, and an
  advisory process;
- license/notice material and dependency license/advisory checks;
- an SBOM and source-revision/build-provenance artifact;
- a bidirectional source-set comparison between source revision and candidate;
- exact agreement between the lockfile and Cargo-generated manifest from an
  isolated clean-HEAD package;
- exact semantic agreement of SBOM components, versions, and dependency edges
  with isolated clean-HEAD metadata;
- an exact, duplicate-free checksum/provenance subject set matching actual
  SHA-256 digests;
- negative regressions rejecting manifest, lockfile, and SBOM tampering; and
- negative regressions that inspect a package tarball before extraction for path
  traversal, duplicate paths, paths outside ordinary files, symlinks, hardlinks,
  devices, FIFOs, and other non-regular members.

This implementation goal does not authorize publishing to an external registry,
pushing Git commits, or creating releases or tags. Gates P-D and P-E are
therefore reproduced with a local `cargo package` artifact, checksum, SBOM, and
provenance. External distribution requires separate explicit release authority.
Testing a local candidate does not weaken the future contract requiring an
immutable published artifact to match its source.

The project preserves zero normal dependencies for `pythia_core`, opt-in
bridges with only required features, the 400-line checker, and the English prose
rule. SBOM and advisory tools are not runtime dependencies.

### 2.7 Stability levels

Each feature has one of these stability levels:

| Level | Meaning |
| --- | --- |
| Experimental | API/semantics may change; not recommended for production |
| Candidate | public contract fixed; conformance and fresh-consumer checks complete; operational evidence accumulating |
| Stable | semver, deprecation, and security support plus the release gates in this ADR |

Describing the entire repository as production-ready requires Stable core Query
and Mutation execution, resolver code generation, the neutral HTTP endpoint, and
at least one opt-in bridge. The combined façade components from ADR-0009 through
ADR-0012 must pass the integrated gates. A standalone server that owns listeners,
routing, connections, and shutdown is neither a Stable requirement nor part of
Pythia's distribution. Experimental features such as Federation or Subscription
do not prevent the core from becoming Stable, but each feature's level must be
stated explicitly.

### 2.8 Accepted schema/testkit contract

Schema snapshot/diff models and their CLI implementation live in the build-time
`pythia_codegen` crate. It exposes `SchemaSnapshot`, `SchemaChange`,
`SchemaChangeKind`, and `SchemaDiffReport`. Unknown formats or kinds are parse
errors with exit code 2; understood changes that violate policy use exit code 1.
Runtime `SchemaInfo` lives in `pythia` and exposes only workspace version,
generated ABI, schema digest, and optional catalog revision as read-only data.

Production crates do not depend on `pythia_testkit`. Its endpoint, bridge, and
provider conformance entry points return fixture factories and async test
futures so third-party implementations can poll them on their own runtime. The
deterministic pending/drop probe spawns no task; it observes future and stream
drop guards directly.

### 2.9 Pre-result release thresholds

WP-P0 fixes production-candidate thresholds before results are observed:

- fuzz: run every parser, SDL-composition, HTTP-decoding, JSON-conversion,
  persisted-ID, and configuration-parser target for 30 minutes against its fixed
  corpus; allow zero crashes, hangs, out-of-memory failures, or sanitizer
  findings.
- load: use the application-owned server example's release build with 64
  concurrent clients, 10,000 requests, and small Queries no larger than 1 MiB;
  allow zero transport, GraphQL, or internal errors; verify expected data for
  every Query; and allow memory to grow by no more than 64 MiB from the steady
  post-warm-up median.
- soak: use the application-owned server example's release build with 32
  concurrent clients for 15 minutes at a 9:1 Query-to-Mutation ratio; allow no
  unintended 5xx response, timeout, or GraphQL error; reach both Query and
  bounded-Mutation resolvers; and finish with zero in-flight work in both the
  application server and Pythia endpoint.
- benchmark: allow at most 15 percent regression from the previously accepted
  baseline median on identical hardware/toolchain for parse, validate,
  cache-hit, and execute; at most 5 percent disabled-policy overhead; at most 25
  percent overhead for each enabled operation/rate/observer profile; and at most
  50 percent compound façade overhead. For the first release, baseline and
  candidate come from the same clean candidate revision and binary digest.
- large consumer: check the 70-object/750-field fixture from a clean target under
  MSRV and stable within 180 seconds, with at most 64 MiB of generated source and
  at most 400 lines per generated file.
- compile/package: complete every MSRV and stable gate for a fresh consumer from
  the release tarball within 20 minutes each.

Production review records hardware, OS, revision, dirty state, binary SHA-256,
complete toolchain, commands, sample count, median, and peak RSS. Exceeding an
absolute time on different CI hardware never passes automatically; it requires
remeasurement on identical hardware or an owned finding.

## 3. Integrated production gates

### Gate P-A: Developer contract

- ADR-0008 DX-01 through DX-06
- WP-P2a neutral endpoint/body/deadline SPI and opt-in bridge public contract
- resolver generation/checking, resolver completeness, and a vertical slice
  through the neutral endpoint and Hyper/Axum bridges
- server resolver code generation usable independently of client tooling and an
  operation directory

### Gate P-B: Admission and operation governance

- every ADR-0009 LIFE item
- every ADR-0010 OP item
- every ADR-0011 BUDGET/RATE item
- adversarial simple-browser, unknown-operation, and high-cost-operation cases

### Gate P-C: Diagnostics and privacy

- every ADR-0012 OBS item
- error-ID correlation, secret redaction, bounded cardinality, and exporter
  backpressure

### Gate P-D: Change and distribution

- schema diff and registered operation impact
- testkit/provider/endpoint/bridge conformance
- MSRV, stable, fuzz, load, and soak results
- package artifact fresh consumer, SBOM, security/release documentation

### Gate P-E: Independent production review

- protocol/conformance review
- security/resource governance review
- developer experience/API compatibility review
- zero unresolved Critical, High, or Medium findings; Low findings are accepted
  only with an owner, deadline, and documented residual risk
- every gate reproducible from both the final commit and local packaged
  candidate artifact

## 4. Consumer strategy

Application integration separately checks:

- large schema codegen/compile/Clippy/rustdoc
- a real operation catalog and schema-impact report built through an application
  adapter
- authentication, DataLoader, rate, and cookie behavior connected as application
  policy
- differential/canary migration
- a large sample of compile-time and runtime overhead

Application-specific APIs do not define the framework gate. The repository
also retains a synthetic large consumer, the books quick start, and an
adversarial corpus, and verifies Candidate status with a second independently
owned consumer when possible.

## 5. Alternatives considered

### Declare production readiness after functional tests alone

Functional tests cannot evaluate schema rollout, distribution, security
response, or performance regression.

### Require a runtime schema registry and hot reload

This complicates compile-time resolver completeness and binary-artifact
integrity without being necessary for the first production baseline.

### Leave the comparator in an example

Other consumers could not reuse the same strict migration authority, so it moves
to the testkit.

## 6. Consequences

### Positive

- The production-ready determination is a reproducible gate.
- Schema-wide compatibility can be evaluated separately from actual operation impact.
- Consumers can reuse framework, provider, endpoint, and bridge quality tests.
- Rolling deployment remains safe without weakening generated, compiled contracts.

### Negative

- Release, artifact, CI, and security maintenance costs increase.
- Control planes such as remote registry and hot reload are not included in the initial Stable range.
- The exception to the schema change would require continuous owner/client inventory management.

## 7. Acceptance matrix

| ID | Expected |
| --- | --- |
| GOV-01 | Notational differences between semantically identical SDL produce an empty diff; API differences use the closed classification. |
| GOV-02 | With a catalog configured, every operation is revalidated against the new schema and impacted operations are reported by exact ID. |
| GOV-03 | An unknown diff kind never falls back to pass. |
| GOV-04 | Schema-instance and readiness events report matching version, ABI, schema digest, and optional catalog revision. |
| TEST-01 | The neutral endpoint and Hyper/Axum bridges pass the same public testkit conformance suite, and bridges do not own the server lifecycle. |
| TEST-02 | Third-party persisted-operation and rate-provider implementations can run the contract suite. |
| TEST-03 | ADR-0007 negative and allowlist coverage remains complete after moving the comparator. |
| TEST-04 | The fuzz regression corpus produces zero panics, hangs, or unbounded allocations. |
| REL-01 | A fresh consumer from the package artifact passes fmt, Clippy, tests, and rustdoc on MSRV and stable. |
| REL-02 | A release includes the compatibility table, migration guide, security material, HEAD-matching package and SBOM, provenance, and safe tar-extraction checks. |
| REL-03 | Independent review has zero unresolved Critical, High, or Medium findings. |

Only a release that satisfies Gates P-A through P-E may describe Pythia core as
production-ready in the README or release notes.
