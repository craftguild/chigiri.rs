# ADR-0059: Integrate standards v0.5.0 with normative baseline 0.1.0

Status: Accepted; v0.5.0 integrated in the worktree, qualification tracked in Beads

Date: 2026-11-05

Updated: 2026-11-09. Earlier v0.2.0 implementation and qualification retain their
historical scope; they do not establish v0.5.0 completion.

## Decision and precedence

Use the Craftguild normative v0.1.0 baseline with execution engine v0.5.0 at
the immutable public HTTPS revision
`69ef0ba960e4d61bff3f363c07f8d49c4d53d77e`. This replaces the former v0.2.0
bundle target. Follow published standards
[ADR 0006](https://github.com/craftguild/standards.rs/blob/69ef0ba960e4d61bff3f363c07f8d49c4d53d77e/docs/adr/0006-initializer-cli-and-controlled-engine-upgrades.md).
Use a fixed engine reference, thin entry and offline embedded rules from a
standalone source checkout; do not copy execution archives or a full rules tree.
The common implementation-size contract replaces physical-line, test-inclusive
and 350-line instructions in ADR-0026 through ADR-0028 and active guides:
tokei 15.0.0 Code, at most 400 implementation lines, excluding tests, test
helpers, fixtures, design documents and vendored code by explicit role.
Mixed Rust tests are separated syntactically. Unmeasured input fails.
The shared engine separates parsed conditional fields, initializers, variants,
arms and statements in temporary, line-preserving source. It retains potentially
production feature branches, including their implementation attributes;
unsupported contexts remain failures. Compiled GraphiQL dependency
payloads are retained as verified vendored output while authored glue is measured
at source. Their ownership, exact regeneration and licenses still pass the gate.
Canonical generated Rust still requires complete ownership, hashes and exact
regeneration; a name, directory or header cannot grant an exception.

The user explicitly resumed local integration on 2026-11-07, without staging,
committing, tagging, pushing, running an actual release or changing versions
and release notes. Those operations remain for a later joint session. The
2026-11-09 amendment below permanently removes commit creation and OpenPGP
prerequisites from ordinary validation. Separately authorized repository commits
retain their human-selected signing and full-revision verification policy.

## Rust 1.95 unification amendment (2026-11-07)

The latest human decision replaces the former Rust 1.85 library/SDK and
1.95 CLI/Mock split, and the intervening 1.86 proposal. All first-party
packages, generated SDK requirements, normal builds, canonical formatting,
CI and local acceptance use Rust 1.95.0 with declared minimum Rust 1.95.
The immutable standards release and its own compatibility statement stay fixed.
Normal and Mock dependency isolation, default workspace membership and the
ci/full functional scopes remain; duplicate older/stable compiler lanes end.
Nightly fuzz instrumentation is a separate campaign, not a product build.

The formatter and generated Cargo template are real generation inputs. Preserve
complete old SDK trees and their recorded producers before this change. Since
strict current ownership admission rejects the former minimum, explicitly move
validated old outputs into the private archive and use the normal create-only
`rust codegen` command to produce current SDKs. Do not hand-edit manifests,
markers or producer identities, add a legacy reader, or restamp a check result.
Verify complete ownership and isolated exact regeneration after the cutover;
retained historical records continue to describe their actual compiler.
This is technical build provenance, not a business lifecycle or line-item
model; no persistence schema or domain transition history changes.

## Preserved product boundaries

The 2026-11-08 test-language amendment moves the product assertions added by this
adoption into an opt-in Rust integration test in the CLI crate. The existing
script verifies the common receipt and invokes Cargo with that receipt's fixed
toolchain, target, profile and immutable preparation. A separate test build
directory prevents Cargo from replacing the delivered CLI. `make acceptance`
explicitly runs the ignored artifact test; default-suite skips are not acceptance
evidence. Rust owns the copied offline CLI scenarios, closed GeneratorInfo, exact
SBOM, output-error, no-write and installed-byte assertions. The common receipt
and test command results remain the evidence; the former bespoke Python
cli-acceptance report is retired. Existing infrastructure tests and unchanged
legacy scenarios retain their own scope. No runtime command, helper crate,
dependency or general test framework is added.

Retain generated ABI 8, all GraphQL and Mock formats, transaction/recovery and
resource guarantees, the single closed CLI tree, hidden writer protocol,
the uniform Rust 1.95 build and SDK minimum, and host capabilities.
GeneratorInfo JSON and the generated producer record retain their existing
semantics. Common presentation uses the actual CLI build identity separately;
human help/version gain the common banner/affiliation and an embedded `sbom`
command. Namespace machine output is unchanged. Common terminal paths remain
offline and precede configuration, VM and process initialization. Their writes
and flushes fail visibly; Mock operation-specific BrokenPipe semantics remain.

Common build preparation must precede Cargo compilation. The build script
checks exact sealed inputs, target, features, profile and compiler without
recursively invoking Cargo. Schema 2 prepares the selected debug or release
identity directly; the repository never rewrites a seal or its SBOM properties.
The shared generator selects the actual normal
runtime graph; embedded GraphiQL contributions require their own actual-byte
evidence. Luaur is the selected Rust implementation, without an added external
Luau executable or native Luau payload. Compiler/platform system libraries and
unexecuted foreign runtime qualification remain distinct evidence scopes. Development/build-only Rust dependencies do not
become runtime components merely because Cargo resolved them.

Expose `make`/`make build`, `install`, `test`, `check`, `acceptance`, and
`release`. Native optimized compilation moves from `make release` to `make
build`; `debug`, named distribution targets and their safe publication retain
their separate responsibilities. Support explicit TARGET/CARGO_TARGET_DIR and
PREFIX/BINDIR/DESTDIR without implicit cross-tool acquisition. Installation
copies a built artifact only. The named Linux distribution retains the pinned
glibc environment and cannot be replaced by an ordinary native build.

The workspace version remains 0.1.0. One `chigiri` release group owns all
workspace package versions and the workspace lockfile, using `v` tags and
Conventional Commits through the shared release implementation. Internal Path
dependencies omit redundant version constraints so the shared preparation can
advance the workspace authority consistently. This is a source-distribution
contract; future registry publication must explicitly supply publishable
inter-package versions. The standalone host packaging gate remains required.
The private GraphiQL npm manifest is a build recipe, not a separately shipped
frontend. Its existing 0.1.0 version, build-tool versions, schema-specific SDK
package versions and pinned third-party asset versions are independent of the
product release authority. They are not incremented by `make release`. No release algorithm is copied into this repository.
Below 1.0.0 the common release policy uses breaking/feat → MINOR and fix → PATCH;
from 1.0.0 breaking changes select MAJOR. An explicit major request uses `--major`.
Prepare common installer declarations/generation checks where applicable; actual
release and clean-HEAD delivery packaging are separate authorized operations.

The complete-graph dependency wrapper checks wildcard requirements before
running cargo-deny. It retains the prior private Path/Git and development-edge
exceptions and additionally admits only local edges between actual workspace
members with matching package identities. Registry wildcards, unknown paths and
same-name external sources remain errors, including transitive dependencies.
The stock [cargo-deny public-crate wildcard rule](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html)
cannot express this source-only workspace arrangement, so its duplicate
wildcard diagnostic is disabled; no node is removed from advisory, bans or
source checks. The wrapper remains the required product gate. MIT-0 is allowed
only for the pinned `borrow-or-share` 0.2.4 build dependency of shared JSON Schema
validation. Existing exact MPL exceptions remain unchanged. Repository-owned
example SDK manifests explicitly select the root dual license through canonical
generation; arbitrary caller/fixture license text remains caller-owned.

## Quality, documentation and provenance

Retain ADR-0058 ordinary/full selection and product contracts, with the
build-matrix and bookkeeping retirements described below.
Common checks add gates; they do not turn absent, failed, cancelled or skipped
product work into a pass. Reused evidence must match the current complete
source digest, requested profile and exact ordered successful commands.
Hosted Tier 1, foreign/Tier 2 runtime, formal performance, long fuzz/soak and
publication evidence remain distinct from local inspection or focused tests.

Preserve license exceptions and third-party notices. License project-owned
source under MIT OR Apache-2.0. Retain original history and private evaluation
records outside the public inventory; required contracts and compatibility
fixtures remain available without sibling repositories or private services.
The actual excluded product checkout and its environment are not inspected.

Build preparation and release plans are transient technical operations. Git
and immutable source-bound plans/receipts provide provenance; this change adds
no persistent business lifecycle, transition table or Header-Detail model.
Existing application state and its atomic ownership rules are unchanged.

## Derived ownership and release consistency

Original closed Rust markers and the GraphiQL asset inventory remain the sole
product ownership authorities. Canonical shared-format validation fixtures under
`tests/fixtures/generated-ownership/` bind those authorities, schema/config
inputs and complete output hashes. Rust records retain the recorded full Git
producer; the frontend record uses its actual authoritative inventory digest.
They do not duplicate mutable Cargo version/build input inventories or restamp
existing producer observations. Normal release-only Cargo version edits do not
require an unrelated fixture rewrite.

The converter's read-only `--check` detects drift; explicit `--verify` performs
isolated canonical replay before refreshing these derived data records. Every
shared assessment independently checks inputs, complete outputs and isolated
exact reproduction. Conversion alone grants no size exemption or authenticity.
All runtime and generator implementation remains measured. Beads owns current
execution evidence; these static fixtures contain no pass/review status.

## Public documentation inventory

Original evaluation records, retired implementation prompts, research and
obsolete workflow examples are preserved byte-for-byte with SHA-256 entries in
the ignored private archive. Public guides, normative contracts, numbered ADRs,
current assertion traces and required fixed compatibility fixtures remain.
Links to retired records become historical prose rather than broken public
hyperlinks. The ordinary usability boundary gate no longer treats a retired
production prompt as an authority; it retains its checks against architecture,
implementation handoff and the governing public ADRs. No current acceptance
contract is removed or replaced by a review verdict.

## Pinned v0.5.0 entry and standard outputs

Preserve the current index and product work. Explicitly preserve and retire the
legacy pin, copied common files, archives and custom host/archive loaders before
running the matching `craftguild-standards init` from this repository root.
It creates reference schema 3 in `standards/engine.json`, thin
`standards/launcher.py` and `standards/make/common.mk`, the local Skill and common
AGENTS block, and verifies/caches `craftguild-standards-engine` under
`.craftguild/engine/`. Init preserves product configuration, recipes and Cargo/npm
inputs; finish their integration and align common-crate full revisions manually.

Make/CI and direct launcher operations use the same pinned engine. A standalone
checkout restores only that fixed cache with explicit `make setup`; no global
initializer or full engine is needed. `make prime` reads the engine's offline
embedded rules. Ordinary operations never acquire/update the engine, resolve
Latest or fall back to a sibling checkout. Copy-only install requires the verified
cache and performs no compiler, dependency or tool acquisition. Qualified engine
hosts remain macOS ARM64 and Linux x86_64 GNU; no Windows build host is added.

The common schema 2 resolver owns artifact/toolchain/target/profile/features and
output selection. Enable `rust_output_layout = "standard"`, remove Rust output
overrides and update all native/distribution callers to
`<target-dir>/<target-triple>/<profile>/<file>`, including native builds, without
an old-path fallback. Named distribution packaging remains separate
from native builds. Explicit cross-target selection and named distribution
aliases retain their SDK, pinned Linux compatibility image and sealed Windows
cache/network constraints through bounded prerequisite/compiler argv adapters.
The engine owns runtime graph preparation, GraphiQL evidence, seals, compilation,
output verification and success receipts. Build scripts only verify preparation.
Make and compatibility scripts forward to this implementation; the former
profile editor and custom binary receipt engine are removed.

The product's retained-descriptor distribution publisher provides specialized
no-follow ancestry/identity and exchange guarantees. Distribution packaging
consumes a verified common output and receipt, preserves those stronger product
guarantees, and does not reconstruct the graph or invoke the product compiler.
Its publisher helper is a repository tool with a separate cache. Format, glibc
ceiling and bounded smoke checks remain mandatory; foreign compilation is not
native runtime qualification.

The published engine also includes the conditional Rust AST separator formerly
implemented here. Remove the entire repository Rust standards adapter; invoke
the same published executable for structural assessment, quality, build, install
and release. The common tokei convention includes production-capable cfg
attribute Code lines. Test-only fields and large test modules remain excluded,
without blanking production attributes. Standards owns those regression tests.

The following catalog details describe the v0.2.0 implementation, to be
requalified against the final v0.5.0 source rather than reused as a new pass.
The ADR-0058 product catalog intentionally interleaves debug/release profiles
preparation with Cargo tests. It cannot recursively acquire the common operation
lock. The entry therefore completes the product validator before the common
check/test operation, or reuses its receipt only after verifying the current
complete source digest, explicit ci profile and exact 97 successful ordered
commands. A missing, failed, cancelled, stale or partial record runs the validator
again and never satisfies a gate. The language adapters verify that same receipt;
the full profile contains 148 commands after retiring the three build-regression
steps under the test-ownership decision below. Ordinary selection remains intact. The shared
operation then records the receipt check and, for check, bounded conversion of
canonical ownership into ignored cache before global assessment and remaining
gates. Nested structural assessment uses explicit --root and requires no operation
lock. No source verdict or Beads state is a test oracle. Ordinary validation
does not create commits or invoke signing.

Source inventories, hashes and aggregate digests stay in local seals and receipts.
New consumer SBOM metadata contains build identity and features only; actual
shipped GraphiQL/font/license evidence remains. Technical run histories and
immutable receipts retain provenance without changing business state or adding
Header-Detail records. Full product validation must finish on the final source;
the earlier cancelled partial run is not completion evidence.

### Build-test ownership clarification

Standards owns regression tests for its shared build, install, receipt, entry,
release and measurement behavior, using neutral minimal fixtures. Chigiri removes
its build-interface, SDK-preflight, entry, common-size and repetitive SDK
Git/cache build regression suites;
these are not transplanted into standards. Actual product builds and distribution
SDK/network/cache guards still run. Product/domain, ABI, generated ownership,
repository-language and compiler quality checks remain. Acceptance keeps offline
CLI behavior and one staged installation of the actual built binary, without a
cloned receipt tree or build-tool failure matrix. This amends the earlier blanket
retention of build-test gates; it does not weaken the runtime product contracts.

Writer transport acceptance allows up to 30 seconds for the initial child
readiness acknowledgment. A fresh macOS link took 5.17 seconds to start in local
qualification; subsequent starts took about 6 milliseconds. This setup budget
is separate from the existing two-second backpressure and shutdown assertions,
which remain unchanged. The worker protocol and runtime policy do not change.

The Git/cache build matrix retires acceptance IDs SG-INFO-02 and SG-INFO-03;
their IDs and historical evidence remain stable. This changes test ownership
and execution frequency, not SDK clean/dirty/unknown capture or caching behavior.
The 39 remaining SG IDs, public metadata/schema and generated-record tests,
actual build identity and offline product acceptance stay active. The
2026-11-09 amendment retires the isolated OpenPGP helper and CI GPG prerequisites;
runtime source/revision assertions remain active as explicit metadata fixtures.

Historical completion-state validators are retired. Product checks use normative
contracts, explicit fixtures and actual execution receipts; they do not require
old Implemented/Passed wording or treat recorded review verdicts as current
product evidence. ADR status/history and truthful qualification limits remain.

Before late contract and test-inventory commands, the validator calls the shared
engine to build the CLI with Rust 1.95 and debug identity. This restores the
correct context after release lanes without editing a prepared record.
The shared setup operation verifies the pinned entry/engine protocol; protocol
is an internal engine command, not a published launcher operation. Retiring one
bookkeeping step and adding this real shared build leaves ci97/full148 commands.

## Local integration completion

The local completion criteria for this integration are:

| Criterion | Completion evidence |
| --- | --- |
| I1 — Fixed identity | v0.5.0 reference, managed entry/Skill/common block and common-crate full revision agree. |
| I2 — Connected operations | Make/CI, actual artifacts, bounded SDK hooks and complete product configuration use the fixed engine; old bundle authorities/loaders are retired. |
| I3 — Standalone workflow | Isolated intended public source supports explicit setup, offline prime and actual build without siblings, private archives or a global engine. |
| I4 — Actual artifacts | CLI release/debug outputs use standard paths; exact SBOM, offline CLI/GeneratorInfo and temporary copy-only install succeed; retain specialized distribution protection. |
| I5 — Product quality | Final ordinary `make check`, actual builds and applicable acceptance succeed, preserving the uniform Rust 1.95 build/SDK minimum, ABI, ownership and isolated exact regeneration. |
| I6 — Maintained delivery | Documentation, licenses, dependency boundaries, implementation Code measurement and declared installer/release settings match. |
| I7 — Current evidence | Record final source/index basis, pin, commands, profiles, hosts and deferred qualification; required missing/failing local gates block completion. |
| I8 — Execution boundary | Local completion leaves real release, clean-HEAD packaging, staging, commits and publication for separate authorization. |

Init/prime, configuration inspection and historical v0.2.0 passes do not complete
integration. Use existing product checks and one final-source pass; add tests only
for a concrete uncovered product connection risk. Common build failure matrices
remain in standards with neutral fixtures. A source export may include intended
uncommitted work and must report its Git identity honestly. Hosted/foreign runtime,
extended performance and unavailable SDK qualification remain explicitly scoped,
not implied by local completion. The whole adoption goal pauses on an actual
signing/verification failure during separately authorized repository work,
without unsigned or alternate-key fallback. Ordinary validation performs no
signing operations.

## Entry-equivalence correction (2026-11-08)

Make, CI and direct launcher test/check use the same configured language adapter.
Its test action validates the current complete-source ci report and executes the
normal catalog if that report is absent, invalid or stale. Failures propagate;
no marker is fabricated. Format/lint gates still require that exact successful
catalog. The product wrapper does not run a separate prerequisite pipeline.

The v0.5 engine holds its operation lock across operation hooks. This catalog
interleaves common debug/release preparation, so test/check do not declare an
outer operation with nested builds. They use the ordinary common quality entry;
each actual build still uses the pinned resolver, lock, seal and receipt.
Canonical non-executable ownership fixtures under
`tests/fixtures/generated-ownership` make the common pre-gate assessment usable
in a fresh checkout. They normalize the existing complete product inventory;
shared hash checking and isolated reproduction remain mandatory. Refresh only
through the checked converter after legitimate input changes. This is validation
data, not a second generator or an exemption for implementation code.

The existing private npm quality tool declarations now live at the repository
root, so normal Node resolution finds TypeScript identically for Make, direct
launcher and CI. Explicit `npm ci` prepares them. Runtime dependency selection
and generated GraphiQL payload bytes do not change. The unused common font
renderer vendor and its inventory/patch obligations are retired; the product
banner attribution and active StyLua/full_moon sources remain.

The CLI's same 16 integration targets now share one locked, all-features Cargo
invocation, without case filters or added ignores. Separate Cargo invocations
repeatedly rebuilt the CLI after fixture build-context changes and made the
ordinary adapter exceed the fixed engine's one-hour limit. Grouping removes
that repeated preparation without adding another validator, changing engine
timeouts or reducing product assertions. The command remains bounded to 30
minutes; both profiles use the same explicit target list. Exact ordered-plan
verification rejects earlier per-target completion records. The earlier catalog
counts above are historical; the executable plan owns current command selection.

## Commit-free validation amendment (2026-11-09)

The user permanently removes the requirement to create Git commits merely to
run tests or pass ordinary checks. This supersedes this ADR's earlier signed
temporary-repository requirement, not the signing policy for separately
authorized repository commits. Ordinary `make test`, `make check` and their
product catalog must not create commits or keys, invoke signing, or require GPG,
a clean worktree or release authorization.

The doctor consumes Cargo metadata-v1. Its Rust runtime-identity tests now use
explicit repo-owned metadata graphs and synthetic full revisions, parsed by
the production metadata reader and checked by the production identity verifier.
They retain matching HTTPS/SSH source-ID cases and reject missing sources,
different repositories, requested/resolved revision mismatches and duplicate
runtime identities in the selected consumer closure. These inputs claim neither
real Git history nor an executed fetch. Cargo's Git transport and signature
machinery are not the product's unit-test authority.

Real path-Cargo doctor/consumer integration, SG-CLI-05 and all other active
acceptance IDs remain. Remove the obsolete signing helper, its source role and
CI's dedicated GPG prerequisites. Do not ignore tests, accept empty runs, replace
checks with successful stubs, or change runtime source validation, generated ABI,
resource bounds, standards/toolchain pins or actual producer provenance. Existing
historical signing results retain their original scope. Requalify current source
through the normal gates; this amendment does not turn an unrun gate into a pass.

## Duplicate-free validation amendment (2026-11-09)

Keep the existing automatic test policy, ci/full classification, CLI scenarios,
assertions and watchdogs. Do not transfer CLI acceptance to manual-only work.
Cargo test and all-target Clippy already compile the CLI/Mock target scope;
retire the separate `cli-mock-build-1.95.0` pre-build without removing test,
example, bench, doctest or optimized-safety execution.

Build includes preparation, so omit the separate initial debug prepare. After
release checks, restore debug preparation without recompiling the already
verified debug artifact. Preparation does not replace its success receipt;
the existing receipt and executable must still pass normal verification.

The common `make check` assessment owns canonical shared reproduction and Code
measurement. Product catalog boundaries run the same product ownership, language
and host checks. The first `--product-only` boundary also retains product-to-shared
conversion: shared verification alone does not discover product groups or compare
their generator revisions with the authoritative markers. The final
`--boundaries-only` check repeats boundaries without conversion or shared assessment.
Standalone full validation retains one shared assessment at its first boundary.
Standalone ci validation qualifies the product catalog; `make check` qualifies
the combined scope. Current-source completion reuse and rejection of stale,
failed or incomplete evidence remain unchanged.

The user also retires the shared engine's implicit one-hour command deadline.
Implement that policy in standards source; do not patch a downloaded engine or
invent a public revision. Explicit product command watchdogs, cancellation and
owned-child cleanup remain. Published-pin adoption and current-source
qualification retain their actual scope and recorded results.

## Published v0.6.0 adoption amendment (2026-11-11)

The user authorized the standards authentication release and explicit adoption
for this initial CLI delivery. Upgrade through the published v0.6.0 initializer
to full revision `72cf076962dc8401b2e4d1d84933c8a818ff390c`, then align the
common Rust Git declarations and lockfile with that same revision. This replaces
the v0.5.0 pin in the current identity/completion criteria above; earlier decisions,
results and actual generator-producer records keep their original provenance.
Normative baseline 0.1.0, reference schema 3, configuration schema 2 and both
compatibility protocols remain unchanged. Rust 1.95.0 and all product gates stay
required. Init preserves product recipes, configuration and local task state.

The shared release removes its implicit aggregate command deadline while retaining
explicit watchdogs, cancellation and owned-child cleanup. This does not establish
a diagnosis for an earlier hosted-runner shutdown. Requalify the current product
source, actual artifacts and native CI rather than reuse older-source passes.
Ship the CLI for Linux x86_64 GNU and macOS arm64 through CI-built Release assets,
with target checksums and SBOMs, a generated installer and README instructions.
Verify real download/install/uninstall in owned temporary destinations.

The public CLI installer retains the default curl transport. CLI delivery does
not publish npm/crates registry packages or qualify additional SDK/OS payloads.

The active release request separately authorizes signed commits/tags, pushes and
GitHub publication. Preserve the original local-integration boundary as history;
this amendment does not claim any unexecuted qualification or introduce a business
lifecycle, transition-history table or Header-Detail model.

## 2026-11-11 runner and license amendment

The owner authorized WarpBuild runners for every workflow, including native CLI
delivery and installed-asset acceptance. Linux uses Ubuntu 24.04 x86_64 and
macOS uses macOS 26 arm64 on ephemeral WarpBuild VMs. This replaces earlier
GitHub-hosted-only restrictions; explicit tool provisioning, isolated processes,
product gates and deadlines remain in force. No local service is provisioned by
this decision. First-party code is now MIT OR Apache-2.0, at the recipient's
option; third-party licenses and historical producer observations retain their
original scope.

Linux retains line-tables-only development/test debug information and disabled
incremental compilation. The 150 GB runner volume removes the GitHub-image
cleanup step; it does not remove these build-resource settings. Ripgrep and jq
are explicitly provisioned on both native validation hosts.

## 2026-11-11 renamed standards release

The owner authorized adoption of published `craftguild/standards.rs` v0.6.1 at
`6852b210a9a614920bc5be2187f2705f46db978d`. The actual published initializer re-created the
managed entry after its previous owned bytes were verified and archived.
The engine and common Cargo crates use that same full revision. Product
configuration and instructions remain intact. No old repository-name fallback
is provided. The normative baseline stays 0.1.0; product Rust stays 1.95.


## 2026-11-12 RSS observation amendment

The owner replaces strict RSS sampling cadence and coverage with a phase-based
minimum: one valid post-warmup observation immediately before load and at least
one wholly during load. Collection targets 500 ms without a jitter or coverage
acceptance threshold. A synchronous observation after the first request in one
worker makes the minimum independent of periodic scheduling, including fast
loads. Serialized read intervals and load boundaries share a monotonic origin.
The workload verifier recomputes phase membership and all observed peak growth;
zero read errors and the 64 MiB growth limit remain required. The short workload
stays two seconds with two clients and unchanged operation/drain assertions.

Workload evidence alone advances to format 6. Benchmark/accepted-baseline format
5 and historical observations retain their identities and original scope. The
native private RSS reader and temporary runner timing diagnostic are removed;
ordinary `ps` collection is sufficient for the revised precision requirement.

## 2026-11-13 standards v0.6.2 release preparation

Use published standards v0.6.2 at `be63eefb44a850a01f4cc065c66fff2fe558ee18` through the verified initializer.
The fixed engine reference and common Cargo Git dependencies select that same full revision, with rules baseline 0.1.0 and product Rust 1.95.
This amendment aligns the shared release preparation across CraftGuild products while preserving earlier decisions and historical producer observations.
Declare independent Cargo path-consumer lockfiles in `releases.cargo.additional_lockfiles` so normal release preparation synchronizes their selected local package versions.

Repository-only consumer fixtures select first-party crates by path and rely on their committed lockfiles, which normal release preparation synchronizes.
Their manifests do not repeat a literal product version; published Git/registry dependency examples retain their explicit version policies.
