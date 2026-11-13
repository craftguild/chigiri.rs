# ADR-0027: Lightweight product validation and generator build identity

- Status: Accepted
- Implementation: Implemented
- Validation: macOS local / MSRV and stable / Reviews A and B complete. See
  validation scope and untested environments.
- Date: 2026-09-06
- Decision owners: Pythia maintainers
- Depends on: ADR-0026

2026-09-07 amendment: under
[ADR-0028](0028-semantic-generated-layout-and-handwritten-line-budget.md), the
400-line check applies only to handwritten files. Generated output uses a
semantic layout, explicit ownership, and existing quality and resource checks.

2026-10-17 implemented amendment:
[ADR-0053](0053-trait-only-resolvers-and-application-boundaries.md) changes the
Rust resolver contract to ABI 7 and removes projection/scaffold surfaces.
Lightweight validation, existing provenance formats, and the separation of
product work from publishing/platform qualification remain unchanged. The new
local implementation assignment adds no full-product campaign requirement.

## 1. Context and responsibility

Product validation needs an ordinary Cargo-built CLI and explicit Git or Path
dependencies. Publisher-only SSH, package, fresh-clone and release-identity
gates should not control the development loop. The previous workflow coupled
clean HEAD, release artifacts, package checks, SBOM and provenance to production
readiness. Separate these publisher concerns while preserving product checks.

This ADR replaces portions of ADR-0008, ADR-0013, ADR-0024, ADR-0025, and old
handoff/release records that make publisher gates current implementation
completion requirements, require generator and runtime to be the exact same
release, or prohibit external Path dependencies. Historical Passed evidence is
not altered, and historical goal prompts are not current requirements. This
does not remove ADR-0026 filesystem/process protection, GraphQL security, or the
product need for performance validation.

## 2. Trust the ordinary acquisition and build environment

- The CLI can be built and used through
  `cargo build --locked -p pythia_codegen --bin cargo-pythia`. It requires no
  `cargo publish`, package script, clean HEAD, tag, remote configuration, or
  special environment variable.
- Acquiring a distributed binary is a user or CI responsibility. Future manual
  acquisition from CI artifacts or a registry remains possible, but the product
  CLI gains no downloader, custom signing or authentication, or Git-fetch
  dispatcher.
- Cargo, Git, SSH configuration, agents, credential helpers, rustup, wrappers,
  and caches are trusted execution-environment inputs. SSH is not mandatory. No
  custom known-hosts injection, `-F /dev/null`, empty Cargo home, or forced Git
  CLI transport is required.
- The product never silently falls back from the Git or Path source selected in
  the manifest. It does not claim to isolate or authenticate ordinary Git/Cargo
  configuration.
- Build information and diagnostics do not copy raw environment lists,
  home/user names, absolute source or build paths, remote secrets, or raw
  Git/SSH/Cargo errors. Required rustc facts pass through bounded parsing and the
  common redactor.
- Source trust is distinct from input validation. SDL, TOML, markers, snapshots,
  and output destinations remain closed and bounded.

## 3. GeneratorInfo

Define the closed `pythia/generator-info-v1` build-identity representation below.
Its JSON representation is closed at every level and requires keys even for
nullable fields. Unknown fields, missing fields, and inconsistencies are rejected.

```json
{
  "schema_version": "pythia/generator-info-v1",
  "version": "0.1.0",
  "revision": "unknown",
  "git_commit": null,
  "dirty": null,
  "build": {
    "rustc": "unknown",
    "host": "aarch64-apple-darwin",
    "target": "aarch64-apple-darwin",
    "profile": "debug"
  }
}
```

| Git observation | `git_commit` | `dirty` | `revision` |
| --- | --- | --- | --- |
| HEAD and status available, clean | complete nonzero lowercase 40-character hash | false | `{hash}` |
| HEAD and status available, dirty | same hash | true | `{hash}-dirty` |
| no Git, or either HEAD/status unknown | null | null | `unknown` |

Dirty includes staged, unstaged, untracked, and submodule changes but excludes
ignored build and cache output. A failed status observation never implies clean.
The CLI identity never comes from a different repository or working directory.
Validation rechecks consistency among revision, commit, and dirty fields, while
keeping strict Git revision configuration distinct from unknown producer state.

Version is the generator package semver. Build contains the rustc banner, host,
target, and profile that built the CLI. It contains no arbitrary timestamp,
absolute path, machine ID, user or host name, or environment dump. Strings are
bounded. A failed rustc probe yields `unknown`, not raw stderr. Only the current
implementation's Git object format is supported; SHA-256 repositories are not
added here.

### 3.1 Capture at build time and refresh through ordinary rebuilds

Version information is embedded at build time, not read from Git or rustc after
the CLI is moved and launched. Regression tests prove that an ordinary rebuild
reflects changes to HEAD, index, unstaged files, untracked files, and submodules.
Refreshing does not require `cargo clean`, touching a file, or a publisher-only
build wrapper. Conversely, metadata-only changes do not cold-rebuild unrelated
runtime and dependency crates. Git status uses a safe NUL-delimited porcelain
form or equivalent and never mistakes a non-UTF-8 filename for clean state.

The build script is limited to information capture. Moving automatic SDL code
generation into build.rs remains prohibited. Ordinary library generation uses
the same codegen build information, and a manifest or environment value cannot
inject a false generator identity. Fixture injection and reuse of a recorded
identity during check are separate from the normal generate/update stamp path.

### 3.2 Public display in the CLI and generated crate

`cargo-pythia --version` displays version, revision, dirty/unknown status, rustc,
host, target, and profile in human-readable form.
`cargo-pythia --format json --version` writes one `GeneratorInfo` JSON object to
stdout. Help and version succeed without a manifest, Git, or network and report
the information embedded in the binary.

The generated crate exposes
`pub fn generator() -> &'static pythia::GeneratorInfo` at its root without
requiring an endpoint or backend instance. The JSON marker records the same
fields. Std-only representation and validation for `GeneratorInfo` and build
types live in core and are re-exported from the façade. JSON/Serde conversion
lives in codegen and tool layers, so metadata adds neither Serde nor codegen to
core. Structured data is available through public fields or same-named accessors,
not only a debug string. The tool JSON adapter limits one record to 4 KiB and
reports shape, omission, or inconsistency as `SF4021` without values. Recorded
values are escaped as Rust literals, and absolute paths such as `::std` prevent
schema names from shadowing namespaces.

`GeneratorInfo` is diagnostic provenance, not a signature or proof of
authenticity, runtime version, or SDL content. The same `hash-dirty` may describe
different uncommitted contents. `unknown` honestly represents a missing
observation and does not prohibit generation. Neither dirty/unknown status nor a
checker-version difference becomes a release gate under `--deny-warnings`.

## 4. Preserve canonical generation and records in exact checks

Generate and update record the executing CLI's build information. Check follows
this order:

1. Capture the actual generated tree once with bounds.
2. Validate the current marker schema, `GeneratorInfo`, Rust/JSON record
   consistency, ownership, and inventory.
3. With the **current** manifest, SDL, runtime configuration, and renderer,
   generate a candidate that reuses only the captured `GeneratorInfo`.
4. Compare file sets and all bytes, including Cargo.toml, Rust, and the marker.

When generation and checking CLI information differs, an informational notice
shows both. Check succeeds if different CLIs produce identical output and fails
for actual drift in renderer, inputs, dependencies, types, or API. It neither
removes record fields with a regex nor trusts the marker unconditionally and
ignores other differences. An unknown schema, tampered revision/dirty relation,
Rust/JSON mismatch, or added/deleted file is nonzero. An attacker who writes
consistent false information into every file can defeat authenticity detection;
this limitation is documented without adding authentication.

Check never restamps on success or failure and runs neither network nor Cargo.
Using the pinned formatter is allowed under ADR-0026. A clean CLI, different
build platform, or different profile can check an SDK committed after generation
without requiring a record-only commit. Doctor also validates captured output
without rewriting the record.

## 5. Required validation and removed processes

2026-11-01 amendment: [ADR-0058](0058-tiered-validation-and-weekly-robustness.md)
supersedes the universal full-suite frequency in this section. Shared local/CI
`scripts/validate.sh` defaults to the fixed-toolchain `ci` profile. Iteration
uses focused checks; weekly and explicitly requested `--profile full` runs
retain this section's complete catalog, including stable and broad release
coverage. Long fuzz/soak runs weekly and for relevant high-risk changes. First
qualification retains the complete formal campaign. Correctness, resource,
ownership and command/source evidence requirements remain unchanged.

1. Format check; automatic rewriting occurs only during an explicit format step.
2. Implementation rules, Clippy, rustdoc, generated exact/layout checks,
   handwritten 400-line checks, dependency policy, and diff checks.
3. Full product tests: workspace/all features, examples, standalone generated
   crate, real and large consumers, compiler-negative cases, endpoint/bridge/
   observability/rate/audit, fixed fuzz regressions, and process/filesystem tests.

CI coverage remains on MSRV 1.85 and stable. The same suite is not rerun under
another aggregate name. Because the generated crate is standalone, workspace
test and Clippy alone do not validate it. Validation reuses ordinary caches and
must not modify checkout, manifests, lockfiles, or the generated crate. A dirty
working tree alone does not prevent product validation.

Dependency policy uses `scripts/dependency_policy.py` to inspect one locked,
all-features Cargo graph. Explicit Path/Git dependencies for non-published crates
use `allow-wildcard-paths`; registry wildcards remain prohibited, and advisory
and source policy remain. Only license evaluation excludes a non-published local
SDK validated through the shared registry and marker/inventory. A same-name and
same-version crate from another source is rejected, and both declared Cargo
metadata and the resolved graph must show the SDK's sole dependency as the real
workspace-root façade. Runtime and third-party dependencies therefore remain
checked. The exclusion does not expand to unregistered crates, handwritten
private crates, or third-party crates; the other three policies still inspect
the SDK itself. This is repository inspection policy, not a mechanism to decide
or declare arbitrary user SDK licensing. Configuration follows
[cargo-deny bans](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html)
and license scope follows
[cargo-deny licenses](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html).

Only publisher-specific scripts, workflows, helpers, environment variables,
and tests solely for those contracts are removed. Starting from the research
baseline, close the call graph and retain a removal/relocation/retention
inventory for:

- `scripts/build-release-artifacts.sh`
- `scripts/check-release-artifacts.sh`
- `scripts/check-package-fresh-consumer.sh`
- publisher-only portions of release artifact verification, SBOM, and provenance
- package, clean, and fresh-clone mandatory paths in
  `scripts/check-production-readiness.sh`

The old readiness dispatcher is not retained as a compatibility alias. Product
aggregate and performance scripts receive separate responsibilities. Shared
checks remain, including fail-closed large-consumer evidence generation and
validation, input/diagnostic/resource protection, zero normal dependencies for
core, and generated-output quality. A filename containing “release” is not by
itself a reason for deletion.

Formal performance, load/soak, and long fuzzing are separate product gates, not
publisher features. Short harness and fixed regressions stay in ordinary
validation; long measurements, procedures, and unexecuted status remain
separate. Short HTTP validation uses a dedicated `short-soak` mode with two
workers, two seconds, and a 0.5-second RSS baseline and records the ordinary
Cargo environment and dirty source. Binary digest, source/command correlation,
all counters, RSS, and drain checks remain. Formal verification distinguishes
this mode and rejects short mode and dirty source. A short success never marks
performance, load, or long fuzz gates Passed. Clean source, pinned compiler, and
other formal-comparison conditions remain.

Required CI evaluation never treats failure, skip, cancellation, or an absent
mandatory job as success. Normal product validation runs for forks and without
secrets; absence of a remote deploy key cannot bypass it with overall success.
This task does not modify branch protection or remote configuration. Required
job names are recorded in the handoff.

## 6. Operations not required now and future TODOs

Completing this product implementation does not require commit or push, an SSH
fetch, package publication, an empty Cargo home, duplicate cold builds, a GitHub
protected environment, signing, tags, or registry publication. A real Path
consumer check is mandatory. Git dependency syntax and identity are tested with
local fixtures. A real network fetch may be recorded separately when possible;
an unexecuted fetch is not Passed.

Only an explicit future intent to publish should design these TODOs:

- `cargo publish` targets, dependency order, authentication, approval, and
  pre/post-publication tamper protection.
- Clean distribution builds, CI artifact storage and retrieval, checksums,
  signatures, attestations, reproducible builds, and provenance verification.
- Optional Git tags, release notes, and registry/version policy.
- If needed, fewer formatter dependencies in the generated CLI and additional
  host support.

TODOs do not remain in the product as hidden completion conditions or
unimplemented fallback paths. Passed means that this ADR's product contract and
targeted local/CI validation succeeded; it does not mean the product was
published, input authenticity was verified, or unexecuted performance gates
succeeded.

## 7. Acceptance

Meet SG-INFO and SG-VERIFY in
[spec-conformance Section 15](../spec-conformance.md#15-sibling-generator-conformance).
The implementation record includes the removal inventory, cache/rebuild
observations, a post-generation commit-equivalent check, version behavior without
Git, and a distinction among executed checks, environment-dependent checks, and
future TODOs. Source changes and documentation status remain synchronized.
