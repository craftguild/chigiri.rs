# ADR-0058: Tiered validation and weekly robustness qualification

Status: Accepted / Implementation in progress

Date: 2026-11-01

## 1. Decision and precedence

Use focused checks during editing, fixed-toolchain `ci` validation for ordinary
changes and releases, and `full` validation plus long robustness runs weekly
and for relevant high-risk changes. The first qualification still executes
the complete functional and formal performance/robustness campaign.

This changes execution frequency and duplicated coverage, not correctness,
security, ownership, resource bounds, or the pass criteria of retained checks.
It supersedes the universal full-suite frequency in ADR-0027 Section 5 and
the daily/per-release long-run frequency in ADR-0013 Sections 2.5 and 2.9.
Historical validation and review records retain their original scope.
Platform support tiers, MSRV and generated ABI are unchanged.

## 2. Product validation profiles

`./scripts/validate.sh [--profile ci|full]` defaults to `ci`. Unknown profiles
and arguments fail before a run starts. The same catalog is used locally and
by both native Tier 1 CI runners: Linux x86_64 and macOS arm64.

Both profiles retain source snapshots, finite command deadlines, exact
command/result correlation, failure/cancellation handling and source-unchanged
checks. A successful `ci` run establishes only its own profile.
Source snapshots exclude `.beads` local tracking/database state just as they
exclude build caches and editor state. A review update cannot change product
validation, and an absent Beads directory does not prevent it from running.
In a Git checkout, inventory tracked and untracked unignored source paths;
tracked fixed corpus remains included even when an ignore pattern matches.
Ignored local fuzz discoveries and build output are not source fixtures. A
source archive without Git uses a no-follow tree walk with the same local-state
exclusions. `fuzz/fixed-corpus.txt` explicitly owns the fixed input names;
Rust replay and long-fuzz copying use that same inventory. It must match the
authored corpus paths and rejects missing, duplicate or unregistered inputs.
Local ignored discoveries cannot enter fixed replay and are left untouched.

The `ci` profile uses Rust 1.85.0 for the SDK and Rust 1.95.0 for Mock/CLI.
It includes the fixed-toolchain functional suites and doctests, formatting,
Clippy, rustdoc, generated exact/surface checks, a representative standalone
consumer's five quality commands, fixed fuzz regressions, short HTTP workload,
dependency/asset checks and the existing contract and checker regressions.
The optimized Mock re-entry and runtime-VM safety regressions remain mandatory.

The `full` profile preserves the complete pre-cutover command catalog. These
checks run only in `full`, in addition to the ordinary profile:

- all stable-toolchain product, generated, consumer and fixed-corpus repeats;
- host packaging and relocated-binary acceptance;
- all nine broad Mock release suites, in addition to the two optimized safety
  suites that remain in `ci`;
- complete generated-crate quality and the full-schema fixture consumer checks on Rust 1.85.0;
- large-consumer conformance, including its MSRV/stable execution.

Cheap regressions for these runners remain in `ci`. Selection is an explicit
catalog classification, not an automatic guess based on changed paths. An
unclassified new command stays mandatory in both profiles. The short workload
uses Rust 1.95.0 in `ci`; `full` preserves its existing stable-based check.

Product completion evidence advances to `chigiri/product-validation-v2` with
`schema_version`, `profile`, `source_sha256` and the exact `commands` results.
The expected profile is supplied by the caller and checked independently of
the record. Missing, extra, failed, skipped, cancelled, duplicate, reordered or
mismatched-profile results cannot satisfy a profile. Older evidence remains
historical and is not accepted as current profile-bound completion.

Required PR jobs retain the `Product validation required` aggregate. Both
native jobs must succeed and report `validation=complete` and `profile=ci`.
The reusable workflow accepts an explicit validated profile for weekly/manual
full runs. Branch protection settings and external release configuration are
outside this change.

## 3. Weekly and additional qualification

Every Monday at 00:17 JST (Sunday 15:17 UTC), run the following on one captured
default-branch commit. Manual dispatch supports additional qualification:

- native `full` validation on Linux x86_64 and macOS arm64;
- all six fuzz targets on separate Linux jobs, each for 1,800 seconds with the
  existing sanitizer, input-size, per-input timeout and RSS limits;
- the existing formal load/soak runner on a separate Linux job using Rust
  1.95.0: 64 clients/10,000 load requests and a 32-client/900-second soak.

Replace the old daily fuzz schedule rather than adding a duplicate campaign.
Install cargo-fuzz with the nightly toolchain explicitly. Long jobs have
finite outer deadlines including build time. Always retain logs, source and
toolchain identities, measurement artifacts and failure inputs for 30 days;
important regressions are additionally preserved in the committed corpus.
Fuzz-generated corpus files and crash artifacts do not modify the main checkout
used for other qualification. Performance workloads run without competing jobs
on their runner. Preserve all existing workload/data/RSS/drain assertions.

These shared-runner jobs do not produce a formal comparative benchmark.
Benchmark comparisons keep the existing controlled hardware, fixed compiler,
binary/source identity, raw-metric verification and human baseline acceptance.

Additional long fuzz is required for relevant parser/decoder/input-boundary
changes. Concurrency, queue, shutdown and retention changes require relevant
long soak/stress coverage. Hot-path changes require a controlled benchmark.
The implementation/review handoff records the applicable risk and executed
scope; automated tests never consult Beads or review decisions as oracles.
First qualification and major releases run the complete campaign. Ordinary
releases require `ci` on the candidate and the additional checks required by
their actual changes, rather than repeating every long test unconditionally.

Weekly failure is not retrospective PR success or failure. Investigate it and
retain an issue and reproduction evidence. A crash, hang, OOM, corrupted result
or ownership/resource failure that is present in a release candidate must be
resolved before releasing it. Infrastructure failure or cancellation provides
no successful evidence. Older weekly results identify only their actual source.

## 4. First campaign and unchanged trust boundaries

The initial campaign tracked by `lab01-0g1` retains the existing formal
31-sample/100-iteration benchmark, load/soak and six 30-minute fuzz conditions.
It requires a clean candidate commit, actual raw evidence, identifiable human
baseline acceptance and independent performance review. No short-mode result
or documentation change alone closes that task. Source changes require fresh
candidate-bound evidence.

Implementers append `Fixed` evidence; independent reviewers alone verify and
close findings. Neither product validation nor release evidence consumes
review statuses. Source snapshots describe transient command runs, with ordered
results retained for that run; no persistent business lifecycle or Header-Detail
model is introduced. Product/runtime state machines and their ownership history
are not altered by selecting a validation profile.

## 5. Acceptance

- Profile and required-job regressions reject invalid selection, missing or
  failed work, profile substitution, cancellation and source mutation.
- `full` retains the pre-cutover catalog, and `ci` retains all fixed functional
  and optimized safety checks specified above.
- Workflow checks prove both native PR jobs, the single weekly schedule, all
  six finite fuzz jobs, formal load/soak and unconditional failure-artifact
  retention. Inspection is not a hosted-run success claim.
- Normative architecture, handoff, conformance, support policy, runbook and
  README agree about the profiles and formal qualification boundary.
- Actual local/hosted execution and unexecuted environments are reported
  separately, with source identity and command timings for future budgeting.
