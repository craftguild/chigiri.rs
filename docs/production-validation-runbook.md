# Production validation runbook

Use the [validation guide](validation.md) and
[ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md) for ordinary
`ci`, weekly `full` and change-specific execution. This runbook defines formal
qualification conditions when selected. The first campaign retains all of
them; ordinary changes and releases do not repeat the long campaign by default.
Weekly CI runs full functional validation, six long fuzz targets and load/soak;
comparative benchmarks remain on controlled hardware with human acceptance.

This runbook explains how a person reproduces the thresholds fixed in ADR-0013 Section 2.9 on a production candidate and stores machine-readable evidence in `target/production-evidence/`. Do not change concurrency, duration, request counts, or tolerances to fit the results.

## Prerequisites for formal long-running measurements

- Commit the source revision and use a clean worktree.
- Install fixed Rust 1.95.0, nightly, `cargo-fuzz`, Python 3, and `ps`.
- Use Rust 1.95.0 for product builds and checks. Select nightly explicitly for
  the separate fuzz instrumentation and its installation.

```sh
rustup toolchain install nightly
cargo +nightly install cargo-fuzz --locked
./scripts/run-release-fuzz.sh --check-toolchain
```

The explicit nightly selection separates fuzz instrumentation from the fixed
production compiler; it does not change the product or SDK minimum.

- Select an immutable performance compiler version in the `1.95.0` form and install it beforehand, for example with `rustup toolchain install 1.95.0`. The scripts isolate committed HEAD in a detached worktree and name that version for both building and evidence collection.
- Both validation profiles use fixed Rust 1.95.0 under ADR-0059. Keep full functional validation separate from performance comparison. Never record a mutable compiler alias in performance evidence.
- Unset compiler overrides including `RUSTC`, `CARGO_BUILD_RUSTC`, rustc wrappers, `RUSTFLAGS`, and `RUSTC_BOOTSTRAP`. Before building, the formal runner rejects nonempty overrides for Cargo home/configuration, incremental compilation, profiles, targets, and native compiler flags, then uses Cargo and rustc resolved by `rustup which`. It also stops rather than implicitly applying `config` or `config.toml` from the default Cargo home or a parent directory outside the repository.
- For the first release, measure baseline and candidate consecutively with the same revision, binary SHA-256, hardware, OS, and toolchain.
- For comparisons with a previous release, retain enough Git history to verify the baseline revision.
- Stop other high-load processes. Include the result JSON, every log, `workload-binary/chigiri_books`, and `benchmark-baseline-binary/production-benchmark` in the release artifact.

Only current format-version 5 evidence is accepted for formal qualification.
Older `benchmark-baseline-accepted.json`, `load.json` and `soak.json` are
historical material. The old soak mutation payload never reached its resolver,
and old evidence lacks sufficient source/binary/toolchain attribution.
Do not add a compatibility reader for old evidence.

## Required manual work

The initial campaign requires performance, load, soak, and long-running fuzz
measurements from the clean candidate commit. Prior deferral is not successful
qualification evidence. The production release owner must run all of the
following from that candidate.

ADR-0016 through ADR-0025 changed the crate graph to the `chigiri` facade and `chigiri_runtime`, and introduced generated ABI 5, projection dispatch, structured doctor/scaffold output, the standalone `chigiri_rate` SPI, `chigiri_rate_memory`, automatic baseline composition, Stage A/B provider deadlines, and borrowed operation audit identity. These changes affected SPI/storage linkage and layout and the public event hot path. Benchmark, load, soak, and fuzz results taken against ABI 4, the old preset, the old rate crate graph, or the old request/audit hot path are therefore invalid as formal evidence for this candidate. The release owner must:

<!-- Acceptance trace: formal-evidence-invalidation-and-human-rerun -->

regenerate all evidence from the new clean HEAD rather than copying old values into the accepted baseline. The release owner owns collection; the performance reviewer owns evidence review. Until both finish, Gates P-D and P-E remain Not Passed and the candidate must not be described as production-ready.

```sh
./scripts/check-fuzz-regressions.sh
CHIGIRI_FUZZ_SECONDS=1800 ./scripts/run-release-fuzz.sh
./scripts/run-release-load-soak.sh 1.95.0
./scripts/check-large-consumer-conformance.sh
```

Run ordinary fixed-toolchain, generated-drift, representative consumer, and
short-harness checks with `./scripts/validate.sh`. Use
`./scripts/validate.sh --profile full` for the complete Rust 1.95.0 functional catalog.
Those checks allow dirty path-based development and are distinct from the
clean-worktree and fixed-compiler requirements above. ADR-0027 removed the old
readiness dispatcher and package/fresh-clone distribution gate.

Before formal measurement, verify exactly one baseline generation:

```sh
./scripts/run-release-benchmarks.sh verify-initial \
  1.95.0 target/production-evidence/benchmark-baseline-accepted.json

./scripts/run-release-benchmarks.sh verify-previous \
  1.95.0 \
  path/to/previous-release/benchmark-baseline-accepted.json \
  path/to/previous-release/benchmark-baseline-binary/production-benchmark
```

Then run the long fuzz, load, and soak commands above, followed by `compare-initial` or `compare-previous` below. Previous-release inputs must come from revision-specific immutable storage, not a fixed output path at the repository root. After the ADR-0026/0028 migration to ABI 6 and responsibility-based layout, do not reuse performance evidence from older candidates as proof for the current one.

`run-release-fuzz.sh` runs the parser, SDL composition, HTTP decoding, JSON conversion, persisted-ID, and configuration-parser targets for 30 minutes each under nightly with sanitizers. Any finding, crash, hang, or OOM fails the gate.

`run-release-load-soak.sh` uses the release binary of the application-owned books server. Load uses 64 concurrent clients and 10,000 requests, requires zero transport/GraphQL/internal errors and exact expected query data, and permits at most 64 MiB of steady-state memory growth. Soak uses 32 concurrent clients for 15 minutes at a 9:1 Query-to-Mutation ratio, using `replaceBook`, which does not increase record count. Load requires an exact match for every field of seeded books. Soak accepts only the initial or known post-replacement values and rejects empty arrays and unknown values. Both require expected data, zero GraphQL errors, and zero application and endpoint in-flight work at shutdown.

Workload evidence format 6 requires one valid RSS observation after warmup and
immediately before load, plus at least one observation entirely during load.
The periodic collection target is 500 ms; scheduling jitter, a minimum total
sample count beyond these two observations, and percentage coverage are not
acceptance conditions. The pre-load settling delay remains 0.5 seconds for the
short harness and one second for formal workloads. A worker takes one additional
synchronous observation after its first actual request, before it returns, so a
fast load does not depend on a timer firing. The two-second short workload and
all Query/Mutation, error, drain and shutdown assertions remain unchanged.

Each observation stores resident bytes and its monotonic read start/end times,
relative to the sampler origin. Workload start/end use that same origin. The
verifier requires the baseline read to finish before load starts and at least
one whole read interval within the workload. Reads crossing a phase boundary
or occurring after load cannot satisfy the during-load minimum. Every read must
succeed. Both native hosts use `ps` RSS in KiB converted to bytes; failed reads
also emit bounded diagnostics after owned-server cleanup.

The verifier recomputes the single baseline value, the peak of all observed RSS
values, growth at most 64 MiB, fixed workload conditions, counters, histogram,
readiness and server exit. Lower-frequency observations do not prove that no
peak occurred between them. Benchmark and accepted-baseline evidence retain
format 5; historical workload records keep their original format and policy and
cannot qualify the new format-6 gate. A percentile in the over-five-second bucket
uses `upperBoundMs: null` and `greaterThanMs: 5000`, never an invented upper bound.

Evidence records the actual command, Unix collection time, OS version, architecture, logical CPU count, and hardware descriptor. Missing or empty values, zero CPUs, invalid times, and command/fixed-condition mismatches are rejected. Even valid JSON with a non-object top level is classified as `malformed-response`. `source.revisionUnixSeconds` must match Git history; collection must occur after that time and may be at most five minutes in the future. Fixed integer fields reject floats and booleans. The runner saves the workload binary in the evidence directory so the same SHA-256 artifact remains independently verifiable after the isolated worktree is removed.

Before long tests, `short-soak` can verify the same structure and reach both Query and Mutation resolvers in two seconds. Since ADR-0027, it tests the current dirty tree in a normal Cargo environment, records the dirty state honestly, and checks binary digest, fixed counters, RSS, and drain behavior. It is not formal performance evidence. The formal load/soak verifier rejects short mode and dirty sources; the prerequisites above remain unchanged.

```sh
./scripts/check-production-performance-harness.sh
```

The benchmark workflow never creates an accepted baseline implicitly. For the first release, measure the same candidate revision and binary digest twice on the same hardware and toolchain. First record an unapproved baseline candidate; then a human reviews conditions and values, supplies a personally identifying reviewer and reason, and explicitly names that file for the second measurement.

For later releases, give `compare-previous` both the accepted JSON stored with the previous release and the immutable `production-benchmark` binary whose SHA-256 matches that JSON. A release without a retained baseline artifact cannot be a comparison source. `record` saves its measurement binary as `target/production-evidence/benchmark-baseline-binary/production-benchmark`; `compare-initial` reuses that exact binary without rebuilding. Keep each release's accepted JSON and binary in revision-specific immutable storage that a later `record` cannot overwrite.

After `record` completes for each release, review the unaccepted baseline's
conditions and values. Then set `REVIEWER_ID` to the actual human approver's
auditable name or personal ID, and `REVIEW_REASON` to that review's specific
rationale of at least 40 characters, covering measurement conditions and the
decision basis. Neither input has a default.

```sh
./scripts/run-release-benchmarks.sh record 1.95.0
```

Complete the human review described above and set both inputs for this approval
before running:

```sh
: "${REVIEWER_ID:?Set REVIEWER_ID to the actual human approver ID}"
: "${REVIEW_REASON:?Set REVIEW_REASON to the specific review rationale}"
python3 ./scripts/accept-benchmark-baseline.py \
  target/production-evidence/benchmark-baseline-unaccepted.json \
  target/production-evidence/benchmark-baseline-accepted.json \
  --reviewer "$REVIEWER_ID" \
  --reason "$REVIEW_REASON"
./scripts/run-release-benchmarks.sh compare-initial \
  1.95.0 \
  target/production-evidence/benchmark-baseline-accepted.json
```

For the next release:

```sh
./scripts/run-release-benchmarks.sh compare-previous \
  1.95.0 \
  path/to/previous-release/benchmark-baseline-accepted.json \
  path/to/previous-release/benchmark-baseline-binary/production-benchmark
./scripts/run-release-benchmarks.sh record 1.95.0
```

Review the new baseline and set both inputs again for this release's approval
before running:

```sh
: "${REVIEWER_ID:?Set REVIEWER_ID to the actual human approver ID}"
: "${REVIEW_REASON:?Set REVIEW_REASON to the specific review rationale}"
python3 ./scripts/accept-benchmark-baseline.py \
  target/production-evidence/benchmark-baseline-unaccepted.json \
  target/production-evidence/benchmark-baseline-accepted.json \
  --reviewer "$REVIEWER_ID" \
  --reason "$REVIEW_REASON"
```

Use the accepted baseline's immutable `source.toolchain.version` for `compare-previous`. Current product comparisons require Rust 1.95.0 for both baseline and candidate. A record made with an older compiler retains its original identity and needs a new 1.95.0 measurement and human acceptance before use in a current comparison; never relabel it. Run the fixed-toolchain functional profiles separately with `validate.sh`. `record` plus human approval creates the current release's baseline. Copy the accepted JSON and `benchmark-baseline-binary/production-benchmark` together into a revision-specific release directory and keep them immutable. The next release must use that stored pair rather than a fixed root-level path. Only after `compare-previous` succeeds should `record` produce the next candidate. Human approval and revision-specific storage are deliberately not automated. Never copy the comparison binary over the next baseline's fixed output or overwrite a stored artifact.

Approval is a human decision, not an automatic gate. `acceptedBy`, automatically assigned `acceptedAt`, and a specific `acceptedReason` must be nonempty. Generic roles such as `release-owner` and reasons copied from the runbook template are rejected. Comparison fails regardless of values when format, revision, dirty state, binary SHA-256, complete Cargo/rustc identity, samples/iterations, hardware descriptor, profile contract, or measurement order differ. The initial comparison requires exact baseline/candidate identity. Later comparisons require the prior release binary digest to match baseline identity exactly, then permit different revisions.

`acceptedAt` is normalized to `YYYY-MM-DDTHH:MM:SS+00:00` without fractional seconds, must identify the same UTC second as `acceptedAtUnixSeconds`, must follow measurement, and must be within 24 hours. If an artifact is relocated, retain the command's original path in the audit record; the verifier identifies the relocated artifact by basename and binary SHA-256. `benchmark.json` records baseline and candidate identities, sample counts, each median, disabled/enabled overhead, and peak RSS.

Profiles are `policy-disabled`; `operation-enabled`, using a real catalog Safelist lookup; `rate-enabled`, using a Stage A/B provider; `observer-enabled`, using production's ID, sampling, and bounded chain; and `facade-composition-enabled`, combining everything in one endpoint component graph. Record inclusion and exclusion in JSON and alternate sample order forward and reverse. Parse, validate, cache-hit, and execute regressions fail above 15%; disabled overhead fails above 5%; individually enabled overhead fails above 25%; facade composition overhead fails above 50%. Medians must be finite and positive, every RSS attempt must succeed, and the verifier recomputes regression, overhead, and pass/fail from raw medians.

Profiles using the observer queue drain it outside timing after every sample. They require 29 lifecycle events per successful request, zero error events, and zero drops. JSON records capacity, expected and accepted counts, and lifecycle/error drop counts. `diagnostic-noop-observer`, `diagnostic-queue-only`, `diagnostic-random-request-id`, `diagnostic-telemetry-clone`, and `diagnostic-full-observer` decompose observer overhead and remain separate from threshold pass/fail profiles. The random request-ID profile includes the startup seed and a per-request keyed hash; compare it with the fixed-ID profile to isolate request-ID derivation cost. Evidence from the old per-request OS-random approach cannot become the formal baseline.

Load/soak and accepted baseline evidence can be checked independently by recomputing identity from the current binary:

```sh
python3 ./scripts/verify-performance-evidence.py load \
  target/production-evidence/load.json \
  --binary target/production-evidence/workload-binary/chigiri_books --toolchain 1.95.0
python3 ./scripts/verify-performance-evidence.py baseline \
  target/production-evidence/benchmark-baseline-accepted.json \
  --binary target/production-evidence/benchmark-baseline-binary/production-benchmark \
  --toolchain 1.95.0 \
  --comparison-mode initial-candidate
```

For a previous-release baseline, also supply `--comparison-mode previous-release` and `--baseline-binary`.

The large-consumer gate checks the 70-object / 750-field fixture on fixed Rust
1.95.0 within 180 seconds per command, with at most 64 MiB of generated output
and the current ABI 8 contract. It retains the responsibility-based semantic
layout introduced by ADR-0028. Evidence format 4 records the single compiler
and all six negative diagnostic cases.

## Failure handling

Never reinterpret an environment-dependent failure as a pass. If remeasurement
on the same hardware does not resolve it, record the command, error, alternative
evidence, owner, deadline, and residual risk in Beads under the
[review workflow](review-workflow.md), and leave Gates P-D and P-E incomplete.
Link any applicable historical production-review record without updating its
frozen status index.
