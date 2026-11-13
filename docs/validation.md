# Development and qualification checks

The [ADR-0059 Rust amendment](adr/0059-craftguild-standards-adoption.md#rust-195-unification-amendment-2026-11-07)
supersedes the former MSRV/stable compiler matrix: both current profiles run
Rust 1.95.0 only. Their functional and optimized-safety scopes remain distinct.
Large-consumer evidence format 5 records one 1.95.0 positive result and all six
negative cases for repo-owned inputs; earlier formats remain historical.

[ADR-0058](adr/0058-tiered-validation-and-weekly-robustness.md) defines the
frequency of ordinary, weekly and formal checks. Profiles reduce duplicated
execution, not correctness, security, ownership or resource assertions.

## During editing

Run focused tests for the changed responsibility. Run
`./scripts/check-implementation-rules.sh` before work, then its `--product-only`
mode after each subtask and formatting. Run `make check` at integration and
handoff to perform shared assessment once. Apply formatting, Clippy, rustdoc and
affected consumer/exact checks before requesting review.

The first catalog boundary retains the product-to-shared ownership converter.
It checks complete product groups and their exact canonical records, including
generator revisions. The final `--boundaries-only` check repeats the product
boundaries without repeating conversion or shared assessment.

## Ordinary product validation

```sh
./scripts/validate.sh
# Equivalent explicit selection:
./scripts/validate.sh --profile ci
```

This uses Rust 1.95.0 for the SDK, framework, Mock and CLI. It retains
functional and safety suites, doctests, generated exact/surface checks,
representative standalone consumer quality, fixed fuzz regressions, optimized
Mock safety regressions and the short HTTP workload. Linux x86_64 and macOS
arm64 both run this profile on pushes and pull requests.

CI explicitly prepares locked dependencies for the root workspace and each
independently locked consumer, compile fixture and fuzz manifest before offline
validation. Preparation includes the two additional consumers used by the
optional full profile; it does not run that profile or update any lockfile.
CI runs the standalone ordinary catalog first, then `make check`. The shared
adapter verifies its complete, ordered, current-source receipt instead of
repeating the catalog. This preserves each command deadline and the two-hour
CI job limit. Standards v0.6.1 has no implicit aggregate command deadline.
The CI catalog runs its existing workspace test command immediately after the
debug artifact build, before the slower Mock and CLI suites, to report native
runtime failures sooner. Every required command still runs once with its
original arguments and deadline, and receipts follow the current catalog order.

On the WarpBuild Linux runner, Cargo dev/test builds keep line-table debug
information for filename/line backtraces and disable incremental caches to
reduce disk use in the ordinary Rust catalog.
The workflow logs those three settings and storage before and after validation.
Local, macOS and release profile settings retain their normal values. Product
commands and ordinary Make entries do not remove host tools.

Ordinary tests and checks do not create Git commits or signing keys and do not
require GPG. Runtime Git source/revision validation uses explicit repo-owned
Cargo metadata-v1 fixtures; it does not fetch a test repository or manufacture
history. Real path-Cargo consumer/doctor integration remains in the catalog.
This is the permanent [ADR-0059 amendment](adr/0059-craftguild-standards-adoption.md#commit-free-validation-amendment-2026-11-09),
not a skipped suite or a requirement to commit local work before checking it.

Mock process tests require successful exit, a closed listener and the original
five-second shutdown bound. Exit success requires both the VM worker join and
writer stop to succeed. The best-effort join notice may be replaced by an
explicit positive notice-loss report under the existing bounded logging
contract; a missing notice without attributed loss still fails the test.

The Mock library unit/example/benchmark aggregate runs independent cases on one
test thread to bound concurrent VM and filesystem fixture pressure. Every case
and its internal concurrency checks remain required within the 30-minute
command deadline. This uses the serial harness policy formerly applied to the
full stable-toolchain repeat with the current fixed Rust 1.95.0 compiler.

All 16 CLI integration targets run in one locked, all-features Cargo invocation,
with the same explicit target list and no test-name filter. This avoids rebuilding
the CLI between targets when their fixtures switch prepared build contexts. The
command retains a 30-minute deadline; VM target commands remain separate. Both
profiles use this grouping. Their completion records must match the current plan;
older per-target command records cannot satisfy it. The pinned v0.6.1 engine
retains explicit command watchdogs without an implicit aggregate deadline. Current
timings and incomplete attempts live in Beads rather than changing the required
product scope.

The two Cargo-config cache cases run their original assertions in dedicated Rust
test children without ambient target/build-directory overrides. This keeps their
fixture configuration authoritative without changing the parent process's cache
selection or mutating process-global environment during concurrent tests.

Broad Mock release suites, complete generated-crate
quality, full-schema/large consumers, host packaging and relocated-binary acceptance
remain in the full profile. Selection does not guess the affected scope from
changed paths. Newly registered checks stay mandatory unless explicitly deferred.

## Full functional validation

```sh
./scripts/validate.sh --profile full
```

This preserves the full functional catalog on Rust 1.95.0, including
broad optimized tests and all standalone generated
and large consumers. It requires the tools used by those scripts.

Results live under `target/product-validation/run-*/`. Completion uses
`chigiri/product-validation-v2` and records the profile, source digest and exact
ordered command results. Missing/failed work, cancellation, source changes or
a mismatched profile cannot produce successful completion. Individual logs
and timings identify failed work and support future time budgets.
In Git checkouts, source inventory includes tracked and untracked unignored
inputs, including the explicitly tracked corpus. Ignored local fuzz discoveries,
build output and Beads database state are excluded. A source archive uses a
no-follow tree inventory. Existing local discoveries are preserved.
`fuzz/fixed-corpus.txt` selects the fixed inputs for both Rust replay and long
fuzzing. Authored corpus paths must match that inventory; register new regression
inputs there in the same change. Unregistered local discoveries are not replayed.

Full functional completion is separate from performance/long-run qualification.
Ordinary-profile, old-source and short-workload results identify only the scope
actually executed.

## Weekly CI

`Weekly and requested full qualification` runs Monday at 00:17 JST against
one captured default-branch commit and also supports manual dispatch. It runs
full functional checks on both Tier 1 platforms, all six Linux fuzz targets
for 30 minutes each, and formal Linux load/15-minute soak in a separate job.
It replaces the former daily fuzz schedule.

Fuzz uses copied corpus inputs under `target/production-evidence/fuzz/`.
Each invocation retains source, compiler and cargo-fuzz identities, logs,
discovered corpus and crash artifacts in a unique run directory. Discoveries
cannot mutate the source corpus. Required regression inputs are minimized
and committed through normal review. CI retains evidence for 30 days even on
failure.

Shared-runner load/soak verifies responses, resource growth and drain behavior;
it is not a controlled comparative benchmark. Formal benchmark baselines retain
their separate hardware, compiler, source/binary and human-acceptance rules.

## Additional checks and releases

- Parser, decoder and input-boundary changes require relevant long fuzz runs.
- Concurrency, queue, shutdown and retention changes require relevant long
  soak/stress checks.
- Hot-path changes require a controlled comparative benchmark.
- First qualification and major releases require the complete campaign.

Ordinary releases require the candidate's `ci` results and the additional checks
required by its changes. Weekly results identify their actual source and do not
establish unexecuted candidate checks. Record risk and actual execution in the
implementation/review handoff. Review decisions are not automated test oracles.

Investigate weekly failures in Beads and preserve unsuccessful artifacts. A
crash, hang, OOM, data inconsistency or ownership/resource defect still present
in the candidate must be resolved before release. Failed infrastructure and
cancelled jobs provide no passing evidence.

Use the [formal runbook](production-validation-runbook.md) for initial full
qualification, controlled benchmarks, raw verification, retained binaries and
human baseline acceptance. Publication and signing remain separate work.
