# ADR-0050: Portable host boundary and tiered CLI builds

The build-entry and native-output decisions below are superseded by
[ADR-0059](0059-craftguild-standards-adoption.md) for standards v0.5.0.
Historical paths and results retain their original scope.

Status: Accepted

Date: 2026-09-24

Implementation: Complete; see `docs/portable-host-and-builds-validation.md`

## 1. Context

ADR-0048 produced one `chigiri` binary with Rust and Mock namespaces while
preserving the existing Linux/macOS CLI host boundary. The repository still
builds that binary through direct Cargo commands. It has no Make entry point,
cross-build configuration, release artifact layout, or explicit platform
qualification tiers.

The current codegen and Mock implementations also contain host-specific path,
filesystem, locking, signal, pipe, and process-tree code. Those details are
distributed across product modules. Extending the build matrix without first
establishing one host boundary would duplicate platform policy and make it easy
for different workflows to weaken path or publication guarantees.

Accept a shared host-support boundary and a tiered build contract together.
The product logic above that boundary remains platform-neutral. macOS arm64
and Linux x86_64 are fully validated Tier 1 platforms. macOS x86_64 and Windows
x86_64 are build-only Tier 2 targets: their release binaries must build and link,
but runtime qualification is not a completion condition. Security design
constraints remain unchanged; compilation alone does not verify runtime safety.

The 2026-09-24 tier revision narrows native qualification to the two environments
prioritized for v1. It replaces the earlier three-target Tier 1 matrix and
Windows test-executable linking requirement. Intel macOS moves to Tier 2;
Linux running on Intel Mac hardware qualifies only as Linux, not macOS.

This decision is independent of ADR-0049. Mock authoring feedback and unfinished
inventory do not consume this ADR number, change its platform tiers, or become
a prerequisite for the host-boundary work. ADR-0049's withdrawn shared-module
proposal adds no host-boundary requirement.

## 2. Supersession and preserved contracts

At implementation cutover, this ADR takes precedence only in these areas:

| Earlier contract | Narrow change |
| --- | --- |
| [ADR-0026](0026-generated-contract-crate-and-manifest-cli.md) and [ADR-0027](0027-lightweight-validation-and-generator-build-identity.md) | Move host path, filesystem, process, and cancellation primitives behind the shared boundary. Preserve generated ownership, exact checking, transactions, build identity, MSRV, and fixed-rustfmt behavior. |
| [ADR-0029](0029-experimental-luau-mock-cli.md), [ADR-0032](0032-mock-project-layout-and-portable-export.md), and later Mock ADRs | Move project filesystem, locking, process ownership, writer transport, and signal primitives behind the shared boundary. Preserve project ownership, recovery, portability, VM isolation, shutdown budgets, and request behavior. |
| [ADR-0048](0048-chigiri-brand-and-single-binary-cli.md) | Replace the Linux/macOS-only distribution boundary with the tier model in Section 5 and add the Make interface in Section 4. Preserve one executable, both namespaces, command grammar, version/help behavior, MSRV separation, and namespace-specific output and exit contracts. |
| [ADR-0049](0049-mock-authoring-modules-and-feedback.md) | No behavioral supersession. Its diagnostic and inventory workflows must preserve verified project reads and consume the common host boundary when both changes are present; no shared-module work is required. |

Do not change GraphQL semantics, generated Rust ABI 6, Mock formats, project
formats, runtime logs, reference formats, command names, or package version
solely for this transition. No compatibility reader or migration is required
because the user-visible data formats remain unchanged.

Coordination with the later [ADR-0051](0051-mock-contract-validation-without-completion-state.md):
that independent decision removes completion tracking and defines its own
Mock-format cutover. If implemented, its retained check/reference workflows
still use this host boundary, but removed unfinished-inventory workflows must
not be restored to satisfy this ADR. No platform or build requirement changes.

## 3. Shared host boundary

### 3.1 Package and dependency direction

Add a publishable `chigiri_host` support crate with Edition 2024, MSRV 1.85,
and `#![forbid(unsafe_code)]`. Its package metadata, included sources, and
versioned dependency declarations must permit future registry publication
because the publishable `chigiri_codegen` crate depends on it. Actual registry
publication of either crate is a separate release task, not an implementation
step or completion condition of this ADR. No registry credentials or uploads
are required. It is not re-exported from the `chigiri` facade and is not an
application-facing runtime API.

The allowed dependency direction is:

```text
chigiri_codegen ----\
                     -> chigiri_host -> vetted OS abstraction dependencies
chigiri_mock -------/
```

`chigiri_host` must not depend on `chigiri_cli`, `chigiri_mock`, Luau, GraphQL
runtime crates, HTTP bridges, Tokio, or generated artifacts. Codegen may use it
as both a normal dependency and a build dependency. Normal framework/runtime
crates do not acquire host, CLI, or Mock dependencies.

Target selection and direct host APIs belong only in sealed backend modules in
`chigiri_host`. Product modules above that boundary must not import
`std::os::unix`, `std::os::windows`, `rustix`, Windows bindings, or target-OS
`cfg` branches. Extend the implementation-rules checker with a narrow source
inventory that enforces this placement while permitting platform-specific
backend tests.

Prefer one implementation through safe cross-platform dependencies when it
preserves the contract. Where kernels require different operations, expose the
same facade and error classes from sealed Unix and Windows backends. Unsafe
code may remain inside reviewed third-party abstractions; first-party Chigiri
code remains unsafe-free.

### 3.2 Path model

Expose a validated `PortableRelativePath` for product-owned logical paths.
It uses `/` separators on every host and rejects empty paths, empty components,
`.` and `..`, backslashes, colons, control characters, absolute roots, drive or
authority prefixes, and the existing component/depth/byte limit violations.
Generated inventories, Mock logical paths, diagnostics, and bundle entries use
this type rather than host-native path strings.

Validation is retained at storage boundaries: generated marker and tree maps,
Mock management and transaction inventories, unfinished reports, reference
module entries, bundle source names, and diagnostic labels carry the validated
type. Conversion back to text is limited to explicit wire or display output.

Native user paths enter only through an explicit ambient open operation. That
operation resolves the native root, opens every component without following a
link or reparse point, and returns a capability-style `Directory`. Consumers
must not concatenate native strings, canonicalize an untrusted path, reopen a
verified logical path through ambient APIs, or infer identity from display
text.

`Directory` does not expose its physical path or implement a native path
conversion. External-tool destinations remain an opaque `ResolvedPath`; host
methods place a verified directory or destination into a process argument and
derive portable labels or relative references without returning native path
authority to Codegen or Mock. The boundary checker rejects reintroducing a
public physical-path extractor or `AsRef<Path>` conversion on these types.
Host-owned external-tool configuration uses a random mode-0700 temporary
directory, retains the file identity through process argument construction,
and removes only that identity through the shared claim-and-verify primitive.
Product code does not allocate, reopen, or unlink a predictable native
temporary path.

The Windows backend additionally rejects device namespaces, alternate data
streams, reserved device components, trailing spaces or dots, symlinks,
junctions, and other reparse points. Rejection happens before mutation and does
not echo an untrusted physical path in a diagnostic. Platform-independent
portable-name rules may be stricter than a host filesystem when that is needed
to keep projects and bundles relocatable.

### 3.3 Filesystem and locking facade

The shared facade owns these primitive responsibilities:

- capability-relative directory traversal and verified parent access;
- bounded file reads and tree capture;
- opaque file/directory identity and before/after change detection;
- exclusive file and directory creation;
- no-replace rename and transactional replace primitives;
- shared and exclusive non-blocking locks;
- containment checks that do not follow aliases; and
- stable closed error classes for missing, existing, busy, changed, invalid,
  limited, unsupported, cancelled, and I/O failures.

Codegen retains ownership of generated-tree transaction policy. Mock retains
ownership of project/update/export policy. Both compose those policies from the
same host primitives instead of reimplementing path walking, metadata checks,
or locking.

A backend must not silently replace an atomic, no-follow, identity, or locking
guarantee with a weaker check-then-act operation. If the host or filesystem
cannot supply the required capability, return `Unsupported` before mutation.
This fail-closed result is an acceptable Tier 2 outcome and is not permission
to weaken Tier 1 behavior.

Identity-checked deletion first atomically claims the named entry by moving it
without replacement into an unpredictably named mode-0700 sibling directory.
The backend verifies and unlinks only the private claimed identity. If the
identity changed, it restores the claim without replacement. When the original
name is already occupied, an atomic exchange restores the claim and moves the
concurrent replacement to a random `.chigiri-recovered-*` sibling. If recovery
itself is unavailable, the private claim remains intact rather than deleting
either entry. A separate check followed by unlinking the original public name
does not satisfy this contract.

### 3.4 Processes, signals, and isolated output

The host facade also owns bounded child execution and process-tree lifetime:
stdin/output limits, deadlines, cancellation checks, child-tree termination,
leader waiting, descendant disappearance, and stable failure classification.
Unix process groups and Windows Job Objects remain backend details. Codegen
build-information capture and normal command execution use the same facade on
their respective native build hosts.

Expose a platform-neutral interrupt source for the CLI and server. Preserve
Unix TERM/INT behavior and provide the corresponding Windows console interrupt
behavior where available. Help and version must not install handlers or start
host machinery.

Replace Mock's parent-side Unix nonblocking pipe dependency with one bounded
writer transport used on every host. A dedicated pump thread may perform
blocking frame and acknowledgment I/O, but request/VM execution communicates
with it only through capacity-bounded channels. Closing or terminating the
owned child must unblock the pump within the existing shutdown deadline. Keep
the same-executable hidden writer, frame limits, dropped-output accounting, and
BrokenPipe behavior.

Child and writer ownership may not detach cleanup into an unjoined background
thread. Drop synchronously takes final wait ownership, terminates the group or
job when required, waits for the leader, and joins any writer pump before the
owner is released. Drop reuses an already established shutdown deadline rather
than starting a new interval. If cleanup cannot finish by that deadline, the
process fails closed by aborting instead of blocking indefinitely or releasing
live cleanup work.

## 4. Make and artifact contract

### 4.1 Native builds and distribution candidates

Add a repository-root `Makefile`. Its first target is `release`, so `make` and
`make release` are identical. Commands are non-interactive and use Rust 1.95
with `--locked -p chigiri_cli --bin chigiri`.

| Target | Contract |
| --- | --- |
| `release` | Build the native release binary at `target/release/chigiri`; never write to `target/dist/`. |
| `debug` | Build the native debug binary at `target/debug/chigiri`; never write to `target/dist/` or overwrite a release artifact. |
| `macos-arm64` | Build and structurally verify the `aarch64-apple-darwin` release candidate, then place it in its documented distribution slot. |
| `macos-x86_64` | Build and structurally verify the Tier 2 `x86_64-apple-darwin` release candidate, then place it in its documented distribution slot. |
| `linux-x86_64` | Build the `x86_64-unknown-linux-gnu` release through the pinned glibc 2.17 build environment, verify its architecture and symbol floor, then place it in its distribution slot. |
| `windows-x86_64` | Cross-build and structurally verify the Tier 2 `x86_64-pc-windows-msvc` release candidate, then place it in its distribution slot. |
| `cross` | On a supported macOS build host, preflight all four targets before building any, build all four candidates, then run local `verify-dist`. |
| `check-build-tools` | Read-only preflight of the complete four-target build environment on a supported macOS host; do not acquire missing tools or SDKs. |
| `verify-dist` | Read-only verification of the four-file inventory, architecture, executable format, Linux symbol floor, and applicable native Tier 1 smoke checks as defined in Section 4.3. |

Native release builds are for local use. Their target triple does not establish
the distribution runtime baseline. In particular, a native Linux build may
depend on a newer glibc and must not replace the compatibility-built artifact.
Native commands pass the physical repository `target` directory explicitly.
Preflight rejects environment overrides and uses the pinned Cargo version for
read-only resolved-config inspection, rejecting a configured `build.target`
before the build command runs. The build command's explicit target directory
and empty encoded Rust/Rustdoc flags take precedence over lower-priority Cargo
configuration. This preserves the documented native layout and prevents native
commands from selecting a distribution target or directory implicitly.
The read-only resolved-config query uses `RUSTC_BOOTSTRAP=1` with the pinned
Cargo 1.95 unstable config command. Revalidate that admission step whenever the
pinned Rust/Cargo version changes; this maintenance check is not a current
completion blocker.
Only the four named distribution targets write their respective slots, even
when a distribution target matches the build host. A failed build or artifact
check must not replace an existing distribution candidate.

Publication reopens every source and destination ancestor without following
links, verifies the exact source bytes inside the publication process, and
copies them into a random mode-0700 staging directory. An existing candidate
is exchanged atomically with the retained stage by a native `rustix`-backed
helper in `chigiri_host`, so the old identity remains
available until post-exchange identity, ancestry, and directory-sync checks
pass. A failed check exchanges it back; an absent slot uses a no-replace rename
and restores absence on failure. Source and stage substitutions therefore do
not destroy the previous candidate, and the irreversible commit has no later
fallible validation step.

The complete distribution-candidate inventory is exactly:

```text
target/dist/aarch64-apple-darwin/chigiri
target/dist/x86_64-apple-darwin/chigiri
target/dist/x86_64-unknown-linux-gnu/chigiri
target/dist/x86_64-pc-windows-msvc/chigiri.exe
```

An individual named target checks only its own candidate and does not require
the other three files to exist. `cross` and `verify-dist` require the complete
inventory. Placement under `dist` is not evidence of native Tier 1 acceptance
or authorization to publish; Section 5 defines those qualification gates.

### 4.2 Supported build hosts and preflight

Build-host support is distinct from the platforms on which the resulting
binary is intended to run. This ADR supports the following Make invocations:

| Make targets | Supported build hosts | Required build environment |
| --- | --- | --- |
| `release`, `debug` | macOS arm64/x86_64 and Linux x86_64 | Native Rust 1.95 toolchain and native linker/toolchain prerequisites. |
| `macos-arm64`, `macos-x86_64` | macOS arm64/x86_64 | Cargo, the selected rustup target, and a selected Xcode/Command Line Tools installation with a usable macOS SDK and Apple linker. |
| `linux-x86_64` | macOS arm64/x86_64 and Linux x86_64 | Pinned `cross`, the installed target, a running Docker daemon, and the digest-pinned Linux build image; non-x86_64 hosts also need its execution support. |
| `windows-x86_64` | macOS arm64/x86_64 and Linux x86_64 | Pinned `cargo-xwin`, the installed MSVC target, LLVM/Clang/linker tools, the explicitly acquired pinned Windows SDK and CRT, and usable host network isolation (`sandbox-exec` on macOS or an unprivileged user/network namespace on Linux). |
| `cross`, `check-build-tools` | macOS arm64/x86_64 only | All four target environments above. |
| `verify-dist` | macOS arm64/x86_64 and Linux x86_64 | Read-only artifact inspection tools and the prerequisites for applicable native Tier 1 smoke checks. |

This matrix describes admitted build hosts, not additional qualification gates.
Required execution evidence is collected on the two Tier 1 hosts. An Intel
macOS machine is not required: build the macOS x86_64 candidate with the Apple
SDK/linker on macOS arm64. Make invocations on Intel macOS remain unqualified
unless separately executed and recorded.

Linux-to-macOS builds and running Make on Windows are not promised by this
contract. Unsupported build hosts fail before starting a build or changing
`dist`. This does not change the build-only artifact tiers. Native Linux
validation may build the Linux candidate through its named target or consume
the same candidate produced by the supported macOS build host.

Use ordinary Cargo and the selected Apple SDK/linker for both macOS builds;
installed rustup targets alone are insufficient preflight evidence. Use a
checked-in `Cross.toml` with a digest-pinned CentOS-compatible image for every
`linux-x86_64` build, including on native Linux. The candidate must require no
glibc symbol newer than 2.17. Pin the `cargo-xwin`, Windows SDK, and CRT
configuration for the MSVC artifact. Record tool, SDK, linker, and image
identities in the implementation validation record.

The Makefile never installs rustup targets, Cargo subcommands, Homebrew tools,
Docker images, SDKs, or licenses implicitly. A missing prerequisite fails with
an actionable message before a partial multi-target build. Preflight includes
the selected SDKs and cached images, not only executable presence. Initial
acquisition is an explicit user setup step outside Make and may use the network
through the installed tools. Individual build targets check only their required
prerequisites; native release/debug must not require cross-build tools. Builds
must not silently switch to another SDK, image, host target, or linker when a
specified prerequisite is absent.

Preflight also rejects environment overrides that could replace the pinned
Cross configuration, container engine, Docker endpoint, Apple SDK/linker,
cargo-xwin backend, download source, or xwin cache identity. Prepared xwin
caches are version-, architecture-, and variant-bound and cannot acquire
validity by relabeling an older cache. The preparation command accepts only an
empty cache with symlink-free ancestry. One descriptor-retained process holds a
nonblocking cache lock through version admission, acquisition, normalization,
and provenance recording; cargo-xwin operates relative to that retained working
directory rather than a reopenable public pathname. Normalization converts
cargo-xwin's confined regular-file aliases into hard links, removes its
confined directory aliases through retained parent descriptors, and rejects
absolute, escaping, cyclic, absent, or special-file link targets. It then
requires real headers and libraries in every selected include/library role and
records bounded SHA-256 entries for every directory and regular file in the
entire cache root. Reads use no-follow descriptors and compare identity, size,
and timestamps before and after the bounded read. Preflight rejects changed or
unrecorded root content, missing inventory entries, symlinks, special files,
and changed or symlinked ancestry.

Preparation and builds explicitly select the `clang-cl` cargo-xwin backend.
A distribution build copies the verified `xwin` subtree into a random private,
read-only view, verifies it immediately before and after cargo-xwin, and lets
cargo-xwin write only root-level per-build scratch in that disposable view.
The view derives deterministic lowercase hard-link aliases for mixed-case
Microsoft library names. Its manifest covers both copied entries and derived
aliases, while the sealed source cache remains unchanged. This makes the pinned
toolchain usable when the repository volume is case-sensitive.
Normal Cargo dependency fetching may occur before that step. The actual
cargo-xwin build runs with Cargo offline and with host-enforced network denial,
so a missing or changed `DONE` record cannot enter an SDK acquisition path.

### 4.3 Local artifact verification versus native qualification

`verify-dist` does not rebuild artifacts, start remote jobs, or run the full
product validator. It inspects all four candidates for exact inventory,
architecture, executable format, and the Linux glibc symbol ceiling, then runs
bounded root/Rust/Mock help and version smoke checks only on a Tier 1 candidate
matching the current native OS and architecture. A failed applicable check
fails the command. Record foreign Tier 1 runtime checks as not executed and
Tier 2 runtime checks as not executed because the tier is build-only, even on
a matching native host. These are not passes. Rosetta, Wine, and CPU emulation
are not required or automatically enabled and cannot stand in for native
validation.

Successful local verification means that these structural and applicable smoke
checks passed. It does not establish Tier 1 support. Native full validators on
both Tier 1 targets, native execution of the macOS arm64 distribution candidate,
and execution of the Linux candidate in the pinned glibc 2.17 environment remain
separate mandatory gates under Section 5 and HOST-09/HOST-10. Keep their
source/artifact identities and results separate from local `verify-dist`
output. A macOS arm64 `make cross` can therefore succeed while native Linux
still awaits qualification. Tier 2 artifacts require structural verification,
not native execution, to satisfy this ADR.

Artifacts are raw binaries. Archives, checksums, installers, registry uploads,
signing, notarization, SBOM publication, and release-channel promotion are not
part of this ADR.

## 5. Platform tiers

### 5.1 Tier 1

Tier 1 consists of:

- `aarch64-apple-darwin`; and
- `x86_64-unknown-linux-gnu` with glibc 2.17 compatibility.

Each Tier 1 target requires a native full product validator on final source,
not only a cross-build or artifact-header check. Required CI therefore has
native Linux x86_64 and macOS arm64 validation jobs. The existing
required-job aggregation succeeds only when both complete the common
validator. Record runner image, architecture, toolchains, source identity, and
result separately.

Tier 1 acceptance includes real Rust init/codegen/check/update/doctor/scaffold,
Mock init/update/check/reference/serve/restart/export, HTTP value assertions,
filesystem adversarial cases, lock contention, transaction recovery,
BrokenPipe, cancellation, child-tree cleanup, and shutdown deadlines. The Linux
release artifact additionally runs in the pinned glibc 2.17 environment and is
inspected for newer symbol requirements.

### 5.2 Tier 2

Tier 2 consists of:

- `x86_64-apple-darwin`; and
- `x86_64-pc-windows-msvc`.

These are build-only targets, not claims of runtime qualification.

Required Tier 2 evidence is:

- the macOS x86_64 release binary builds and links using the selected Apple
  SDK/linker, normally on the macOS arm64 build host;
- the Windows x86_64 release binary builds and links through the pinned cross
  tool and SDK/CRT configuration; and
- the artifacts have the correct x86_64 Mach-O and PE formats respectively,
  filenames, and distribution slots within the closed four-file inventory.

Test-executable compilation/linking, native execution, help/version smoke tests,
native CI runners, Rosetta, Wine, and full product validation are not Tier 2
completion requirements. Optional focused results must distinguish executed
cases from unexecuted coverage; environment availability does not make them
mandatory. Tier 2 results do not participate in the Tier 1 required-job
aggregate.

The host design must still reject unsupported capabilities through the stable
closed error boundary, without unsafe fallback or partial mutation. Review
this constraint without representing build/link success as proof that Tier 2
runtime behavior has been tested.

Promotion of either target to Tier 1 requires a later ADR with native
complete-test evidence and explicit filesystem, process, signal, and
distribution support criteria.

## 6. Work packages and acceptance

Implement this decision in bounded packages:

1. HOST-WP0: record current platform imports, build commands, test coverage,
   artifact inventory, dependency/MSRV constraints, and focused reproductions.
2. HOST-WP1: add `chigiri_host`, portable paths, filesystem capabilities,
   locks, process ownership, interruption, and shared contract tests; migrate
   codegen without changing its user-visible behavior.
3. HOST-WP2: migrate Mock project operations, process ownership, server
   interruption, and writer transport; retain all current format and runtime
   behavior.
4. HOST-WP3: add the supported Make/Cross/cargo-xwin build-host matrix, separate
   native and distribution outputs, prerequisite preflight, local artifact
   verification, and developer documentation.
5. HOST-WP4: run focused regression gates, native Tier 1 full validation,
   Tier 2 build/link gates, two independent reviews, fixes and re-review, and a
   final-source validation record.

Acceptance status is maintained in the implementation validation record; this
table defines the required evidence rather than claiming current execution.

| ID | Required evidence |
| --- | --- |
| HOST-01 | Product code above `chigiri_host` contains no target-OS branches or direct Unix/Windows host imports; the checker enforces the boundary. |
| HOST-02 | Portable logical paths produce identical admission and normalized `/` identities on Tier 1; absolute, escaping, aliasing, special, over-limit, and platform-ambiguous inputs fail before mutation. |
| HOST-03 | Directory and snapshot identities detect replacement and concurrent change; bounded reads, tree capture, containment, and source diagnostics preserve current contracts. |
| HOST-04 | Exclusive creation, no-replace publication, shared/exclusive locks, update transactions, rollback, recovery, and generated/project ownership retain their positive and adversarial behavior. |
| HOST-05 | Bounded child input/output, timeouts, cancellation, tree termination, final wait ownership, and descendant absence pass on every Tier 1 host without zombies or detached work. |
| HOST-06 | The common Mock writer transport preserves frame bounds, backpressure, dropped-output accounting, BrokenPipe success for Mock terminal output, Rust output-failure behavior, and bounded shutdown. |
| HOST-07 | `make`, `make release`, and `make debug` use Rust 1.95, the lockfile, the sole binary target, the native profile, and the documented Cargo output locations. Existing `dist` bytes remain unchanged, including when native Linux uses a newer glibc; cross-build prerequisites are not required for native builds. |
| HOST-08 | Named distribution targets use the declared build-host/tool matrix and verify candidates before replacing their slots. Linux always uses the pinned compatibility environment. macOS `make cross` preflights every target first and produces the closed four-file inventory. Unsupported hosts and missing tools, SDKs, or images fail before building or changing `dist`, without implicit acquisition. |
| HOST-09 | macOS arm64 and Linux x86_64 pass the complete final-source validator natively; required CI and aggregation cannot omit either Tier 1 target. No macOS x86_64 native runner is required. |
| HOST-10 | The Linux release runs with glibc 2.17 and declares no newer glibc symbol requirement. The macOS arm64 release has the expected architecture and runs on its native validator. |
| HOST-11 | macOS x86_64 and Windows x86_64 release binaries build and link with the specified tools, have the correct Mach-O/PE architectures, and occupy the expected slots. Test-executable linking and runtime execution are not required. Review confirms unchanged fail-closed design constraints; optional runtime results remain distinct from build-only evidence. |
| HOST-12 | Rust 1.85 codegen/host MSRV, Rust 1.95 CLI/Mock, stable repeats, dependency direction, std-only core, first-party unsafe prohibition, formatting, Clippy, rustdoc, generated exactness, and implementation rules pass. Host package contents and versioned dependency metadata permit future publication; no registry upload or credentials are needed to complete the work. |
| HOST-13 | README and build documentation distinguish build hosts, runtime tiers, native versus distribution outputs, setup/acquisition, and local versus native qualification. `verify-dist` checks all artifact headers and the Linux symbol floor, runs only applicable native Tier 1 smoke checks, reports foreign Tier 1 and all Tier 2 runtime checks as unexecuted with reasons, and neither rebuilds nor substitutes for HOST-09/HOST-10. Microsoft license implications and unexecuted environments remain explicit, with no publication or signing claim. |
| HOST-14 | Architecture/security and regression/workflow reviews close every finding, and the final validation record maps all HOST IDs to source and command evidence. |

The full validator is logically required for Tier 1 because this transition
replaces shared filesystem, process, and writer boundaries. Do not run it merely
as repeated reassurance after each small change; use focused tests during work
and one final-source run on each native Tier 1 target. Neither Tier 2 target
requires a complete validator or a native execution environment.

## 7. Consequences and non-goals

One host facade reduces duplicated security logic and makes platform support an
explicit qualification decision. It adds a publishable support crate and a
strict dependency boundary. Platform backends still exist where operating
systems differ, but those differences cannot leak into codegen, Mock, or CLI
business logic.

Tier 1 becomes stronger than the previous local-macOS-only evidence because
macOS arm64 and Linux x86_64 receive native complete validation. Tier 2 makes
Intel macOS and Windows buildability visible without presenting compilation as a
support promise. A successful `make cross` proves the defined build and artifact
checks; it does not by itself prove Tier 1 runtime acceptance or promote
either Tier 2 target.

Do not add application-facing filesystem APIs, runtime target portability
claims, shell execution, implicit tool installation, automatic downloads,
package management, Windows installers, release upload automation, universal
macOS binaries, Linux musl artifacts, Windows arm64 artifacts, or new CLI
commands. Do not weaken an existing security guarantee to make a Tier 2 command
appear supported. Revisit the ADR when adding a target, changing a tier, or
altering the host boundary's public semantics.

Publishing this ADR does not add the Makefile, create artifacts, change current
platform support, or satisfy any HOST acceptance item.
