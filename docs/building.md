# Building Chigiri

Chigiri separates native developer builds from raw distribution candidates.
The Make interface never installs Rust targets, Cargo subcommands, Docker
images, Apple SDKs, LLVM tools, or Microsoft SDK material. Prepare those inputs
explicitly, then run the read-only preflight before a multi-target build.

## Native builds

The shared native build runs on qualified engine hosts: macOS arm64 and Linux
x86_64. Intel macOS remains a separate build-only target from those hosts. Builds need
GNU Make, a POSIX shell, Rust 1.95, the native linker, Git, Python 3.11 or newer,
curl and Node.js/npm (CI pins Node.js 24.15.0). Prepare the pinned engine and
locked Cargo/npm dependencies explicitly. Builds regenerate GraphiQL in isolation
from the offline npm cache to establish its exact embedded-input SBOM; its
security audit checks current advisory metadata. No toolchain is installed:

```sh
rustup toolchain install 1.95.0 --profile minimal
make setup
cargo +1.95.0 fetch --locked
bash scripts/python-with-toml.sh scripts/graphiql_build.py --prepare-dependencies
make          # target/<host-triple>/release/chigiri
make build    # target/<host-triple>/release/chigiri
make debug    # target/<host-triple>/debug/chigiri
```

These commands use the lockfile and the sole `chigiri` binary target.
They pass an explicit Cargo target directory and never write `target/dist/`.
`CARGO_TARGET_DIR` accepts a relative or absolute output directory. An explicit
`TARGET` selects its triple directory, which is also present for native defaults; for example
`make TARGET=aarch64-apple-darwin` produces
`target/aarch64-apple-darwin/release/chigiri`. Other supported triples delegate
to the existing named distribution build. Preflight rejects unrelated environment overrides and any resolved Cargo
configuration that sets `build.target` before the build command runs. The
explicit target directory and empty encoded Rust/Rustdoc flags prevent
lower-priority Cargo configuration from changing the documented native output.
A native Linux binary may use a glibc newer than 2.17 and is therefore not the
Linux distribution candidate.

The CLI-assets workflow selects the existing `distribution` variant for Linux,
explicitly prepares the pinned cross/image inputs below, checks the exact receipt
output's ELF/glibc requirements, and executes it in that image before common
Release packaging. macOS arm64 retains its native build and artifact acceptance.
The installer downloads those packaged bytes; it does not build a native fallback.

Engine v0.6.1 resolves the declared context, prepares its exact SBOM, compiles in
an ignored candidate cache, and publishes the binary with
`.craftguild/chigiri/receipt.json`. That receipt retains sealed inputs and output
hashes; its per-run SBOM records build identity and actual runtime components
without a source inventory. These are local consistency records, not signatures
or release attestations. The thin entry and full engine are pinned by
`standards/engine.json`; explicit `make setup` verifies and caches that fixed
macOS ARM64 or Linux x86_64 GNU engine without a sibling checkout or global tool.
`make prime` reads matching offline rules. Ordinary operations do not download
or update the engine. The compatibility shell selects Python 3.11+ and retains
the product's bounded validation prerequisites, not a host/archive loader.
`make install PREFIX="$HOME/.local"` copies an existing matching release binary;
`BINDIR`, `DESTDIR`, `TARGET` and `CARGO_TARGET_DIR` are supported. A missing,
modified or mismatched output fails with a build instruction. Install does not
invoke Cargo, npm or any compiler and never acquires a missing entry/engine.
Repeat an explicit `TARGET` or `PROFILE` used for the build during install.
`make acceptance` builds and checks a copied
offline binary and installation under a private temporary staging directory.

## Release preparation

`make release` previews and applies the shared Conventional Commit version and
release notes locally. It is a deliberate release preparation command, not a
build alias. It does not commit, tag, push, upload or publish. Preview without
product-file writes using `make release RELEASE_ARGS=--json`.
The default `RELEASE_ARGS=--apply` performs local preparation. The `chigiri`
group owns the workspace version, all 22 member versions and the root lockfile.
The private GraphiQL build recipe, tooling and schema-specific SDK versions are
independent. See [ADR-0059](adr/0059-craftguild-standards-adoption.md) for scope,
signed-history requirements and the source-only registry-publication boundary.
Below 1.0.0 breaking/feat selects MINOR and fix selects PATCH; from 1.0.0
breaking selects MAJOR. Use `--major` for an intentional major request.
The declared CLI download installer uses the shared generator and
`make installer INSTALLER_ARGS=--check`. Actual clean-HEAD delivery packaging and
publication remain separate owner operations; generated installer settings do not
claim that CLI assets are already publicly available.

## Build configuration checks

The resolved `build.target` check uses `RUSTC_BOOTSTRAP=1` only to access the
pinned Cargo 1.95 read-only config command. When updating the pinned Rust/Cargo
version, rerun the real Cargo-config regression and confirm that unset and set
`build.target` values retain their current admission behavior.

Named distribution builds and `verify-dist` additionally require Python 3.9
or later for executable inspection and bounded smoke orchestration.

## Distribution targets and build hosts

The distribution inventory consists of exactly these raw binaries:

```text
target/dist/aarch64-apple-darwin/chigiri
target/dist/x86_64-apple-darwin/chigiri
target/dist/x86_64-unknown-linux-gnu/chigiri
target/dist/x86_64-pc-windows-msvc/chigiri.exe
```

| Make target | Supported build hosts | Runtime tier |
| --- | --- | --- |
| `macos-arm64` | macOS arm64/x86_64 | Tier 1 |
| `macos-x86_64` | macOS arm64/x86_64 | Tier 2 build-only |
| `linux-x86_64` | macOS arm64/x86_64 or Linux x86_64 | Tier 1 |
| `windows-x86_64` | macOS arm64/x86_64 or Linux x86_64 | Tier 2 build-only |
| `cross` | macOS arm64/x86_64 | All four candidates |

Each named target checks only its own prerequisites, uses the shared engine to
build and verify its recorded output under the selected target directory, then
validates the exact bytes retained by the product publication process, and
only then atomically exchanges its own distribution slot. Publication uses
no-follow directory descriptors and a native safe `rustix` exchange helper. It
builds that repository helper in its own cache; it never invokes the product
compiler or writes a parallel build receipt. Product SDK/compiler argv adapters
retain the pinned compatibility container and isolated Windows cache policy.
It keeps the previous candidate available for
rollback until identity, ancestry, and directory-sync checks pass. A failed
build, structural check, source substitution, or publication check preserves
the previous candidate. `make cross` preflights all four environments before
its first build or distribution mutation.

Linux-to-macOS builds, Make on Windows, Tier 2 runtime qualification, archives,
installers, signing, notarization, checksums, SBOM publication, and registry or
release-channel uploads are outside this interface.

## Apple prerequisites

Select an Xcode or Command Line Tools installation with a usable macOS SDK and
linker, then install both Rust targets for Rust 1.95:

```sh
rustup target add --toolchain 1.95.0 \
  aarch64-apple-darwin x86_64-apple-darwin
```

The preflight records the SDK path and version plus the exact clang driver and
linker paths selected by `xcrun`; the build binds that SDK and target-specific
Cargo linker. It rejects inherited `DEVELOPER_DIR`, `SDKROOT`, `TOOLCHAINS`, deployment
target, linker, and Rust flag overrides rather than silently selecting another
build configuration.

## Linux glibc 2.17 prerequisites

Linux distribution builds always use cross 0.2.5 and the digest-pinned
manylinux2014 x86_64 image in `Cross.toml`, including on native Linux.
The target's explicit environment passthrough forwards only the common engine's
invocation-local preparation selector and command name. The build script verifies
the same sealed source/SBOM record inside the container; it does not regenerate or
substitute metadata there. Prepare the tools and image explicitly:

```sh
cargo +1.95.0 install cross --version 0.2.5 --locked
rustup target add --toolchain 1.95.0 x86_64-unknown-linux-gnu
docker pull \
  quay.io/pypa/manylinux2014_x86_64@sha256:2fa5f99f660f115f547d96b4edcadfc19f77a6de8807734bf0fc4924f81be01a
```

Docker must be running, the invoking user must be able to use it, and the
pinned image must already be cached and executable. The named build rejects an
ELF candidate whose required glibc symbols exceed 2.17.

For reproducibility, preflight rejects inbound `CROSS_*` overrides and remote
or overridden Docker endpoints. It requires the repository `Cross.toml`, the
exact cross version and image digest, and a local Unix Docker socket. The build
sets its Cross configuration and container engine explicitly.

## Windows MSVC prerequisites

The Windows artifact uses cargo-xwin 0.23.1 with its `desktop` variant, Visual
Studio generation 17, SDK 10.0.26100, CRT 14.44.17.14, x86_64 libraries, LLVM
tools, and the Rust 1.95 MSVC target. Install the non-Microsoft tools first:

```sh
cargo +1.95.0 install cargo-xwin --version 0.23.1 --locked
rustup target add --toolchain 1.95.0 x86_64-pc-windows-msvc
rustup component add --toolchain 1.95.0 llvm-tools
```

`clang`, `llvm-lib`, and `lld-link` must all be installed and available on
`PATH`. A Homebrew LLVM installation provides them on macOS; add its `bin`
directory to `PATH` if it is keg-only. The build also requires usable network
isolation: macOS uses `sandbox-exec`, while Linux uses `unshare` with an
unprivileged user and network namespace. Preflight executes the selected guard,
not just its command name. cargo-xwin uses Microsoft SDK and CRT material.
Review the [Microsoft license](https://go.microsoft.com/fwlink/?LinkId=2086102)
before acquisition. Chigiri does not accept that license for you. If you accept
it, populate the pinned cache explicitly outside Make:

```sh
CHIGIRI_ACCEPT_MICROSOFT_LICENSE=yes ./scripts/prepare-windows-sdk.sh
```

Set `CHIGIRI_XWIN_CACHE_DIR` for both preparation and builds to use a cache
outside the version-, architecture-, and variant-bound default under
`target/tool-cache/`. Preparation accepts only an empty cache with symlink-free
ancestry. One preparation process retains the cache descriptors and a
nonblocking lock while cargo-xwin runs relative to that retained directory.
cargo-xwin creates internal aliases on Unix hosts; preparation safely
materializes confined regular-file aliases as hard links and removes confined
directory aliases through retained parent descriptors. It then records the
pinned identity plus directory and SHA-256 file entries for the complete cache
root. Absolute, escaping, cyclic, absent, and special-file link targets fail
preparation without a provenance marker. Bounded no-follow reads compare file
identity, size, and timestamps before and after reading.

Preflight rejects every remaining or newly added root entry that differs from
the inventory, including symlinks and special files, plus missing real headers,
libraries, and pin mismatches. It also rejects every inherited `XWIN_*`
override, including alternate compiler and download-source selection.
Preparation and builds explicitly use the `clang-cl` backend. Each distribution
build stages a random private read-only copy of the verified `xwin` subtree,
checks it immediately before and after cargo-xwin, sets Cargo offline for that
step, and denies network access with the preflighted host guard. The staged view
adds deterministic lowercase hard-link aliases for mixed-case Microsoft library
names so linking also works when the repository volume is case-sensitive. The
view manifest covers those aliases; the sealed source cache remains unchanged.
Ordinary locked Cargo dependency fetching occurs before isolation; Microsoft
SDK/CRT acquisition cannot occur inside the named build. An old or incomplete
cache cannot become valid by relabeling and is never repaired or downloaded by
Make.

## Preflight, build, and local verification

On a prepared macOS build host:

```sh
make check-build-tools
make cross
```

Individual named targets may be run on the supported hosts in the table above.
`make verify-dist` is read-only and requires the exact four-file inventory. It
checks executable Mach-O, ELF, and PE structure and architecture, rejects
objects and libraries, checks the Linux glibc symbol ceiling, and runs bounded
root/Rust/Mock help and version checks only for a Tier 1 candidate matching the
current native OS and architecture. Smoke processes have bounded output,
deadlines, process-group cleanup, and exact Chigiri help/version markers.

Foreign Tier 1 runtime checks are reported as `Not executed` with a reason. All
Tier 2 runtime checks are `Not executed` because those targets are build-only,
even on a matching host. A successful `verify-dist` is local artifact evidence,
not full native qualification. Tier 1 support separately requires the complete
shared product validator on native macOS arm64 and native Linux x86_64, native
execution of the macOS candidate, and execution of the Linux candidate inside
the pinned glibc 2.17 environment.
