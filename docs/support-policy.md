# Support, MSRV, and deprecation policy

Chigiri 0.1.0 requires Rust 1.95 and Edition 2024. By the 2026-11-07 human
instruction and [ADR-0059 amendment](adr/0059-craftguild-standards-adoption.md#rust-195-unification-amendment-2026-11-07),
all first-party libraries, CLI, Mock and newly generated SDKs declare minimum
Rust 1.95. Builds, generation formatting, local checks and both native Tier 1
CI hosts use exactly 1.95.0. The former 1.85 compatibility and duplicate stable
lanes are retired. CI and full profiles retain their distinct functional scopes;
nightly fuzzing remains separate instrumentation.

Earlier ADR-0029 and ADR-0048 compiler splits describe historical decisions.
Older release/design/validation records retain the compiler and source scope
they describe; they do not override the current compiler policy.
Retained old SDK archives keep their actual producer/compiler facts. Current
SDKs are regenerated through the normal generator after the template and fixed
formatter change; old ownership markers are not hand-edited. Future MSRV raises
require an explicit decision and release boundary, never a silent patch change.

The first production release has not occurred. By explicit user direction, this migration does not guarantee
backward compatibility with any earlier unreleased API or generated ABI.
Current generated contracts use ABI 8. Protected `rust update` requires
complete ownership admitted by the current minimum/tooling contract. Earlier
Rust 1.85 SDK output requires preserved fresh `rust codegen` generation after
explicit retirement of the old owned tree; the former ABI 7 update statement
does not waive the current Rust 1.95 ownership requirement.
Generated/runtime mismatches fail compilation with a regeneration diagnostic.

Third-party rate providers depend directly on the exact release of the std-only `chigiri_rate` SPI. Official
process-local providers use the same exact release through `chigiri_rate_memory`. Mixing Git revisions or release
versions across SPI, adapter, policy composition, and façade is unsupported.

After the first Stable release, removal or incompatible alteration of a Stable public API requires a semver-major
release. When safe, a deprecation remains for at least one minor release and its replacement and removal target are
documented. Security fixes may shorten that period when keeping the API creates material risk. Candidate and
Experimental surfaces may change as documented in their release notes.
