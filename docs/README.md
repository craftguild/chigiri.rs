# chigiri.rs documentation

Start with the root [README](../README.md) for a source build and a small Mock
project. This index separates user guides from design authorities and recorded
decisions. Runtime and build requirements are documented separately.

## Guides

- [Mock quick start](guides/mock-quick-start.md): initialize, implement, check
  and serve a Todo API.
- [Mock project workflow](mock-project-workflow.md): editing, references,
  schema adoption, recovery, storage, export and timeleap.
- [Rust integration](rust-integration.md): generated traits, domain bindings,
  context lifetimes and migration.
- [Subscription walkthroughs](subscriptions-walkthrough.md): Rust and Mock
  streaming recipes, delivery bounds and response metadata.
- [Building and installation](building.md): native output paths, cross-target
  prerequisites, receipts, install and deliberate release preparation.
- [Validation](validation.md): ordinary, full, weekly and formal checks.
- [Distributed tracing](distributed-tracing.md): setup, linked event spans,
  suppression, masking and bounded metadata.
- [Support and MSRV](support-policy.md): platform, compiler and compatibility
  boundaries.

## Design reading order

Before changing implementation, read these in order:

1. [Implementation rules](implementation-rules.md)
2. [Architecture](architecture.md)
3. [Implementation handoff](implementation-handoff.md)
4. [Specification conformance](spec-conformance.md)
5. The relevant [Architecture Decision Records](adr/)

The [design index](design/README.md) describes these authorities. The root
[AGENTS.md](../AGENTS.md) also applies. Run
`./scripts/check-implementation-rules.sh` before work, after a subtask and
formatting, and before handoff. ADR-0059 applies tokei Code to implementation
only, at most 400 lines per file; tests are excluded by role. Generated output
requires complete verified ownership and exact isolated regeneration.

## Current boundaries

- One `chigiri` executable provides `rust` and `mock` namespaces. ADR-0048
  supersedes the former product names and binaries.
- Generated Rust uses ABI 8 under ADR-0054, manifest-only commands, dedicated
  owned crates and complete resolver traits. ADR-0053 defines the ordinary
  application boundary retained by ABI 8. Applications enter the framework
  through `chigiri::Endpoint`, whose façade owns the transport pipeline.
  Applications own HTTP listeners, routing, connections and shutdown.
- All first-party libraries, generated SDKs, Mock and CLI require Rust 1.95;
  normal builds, generation and CI use exactly 1.95.0 under the ADR-0059 amendment.
  Normal/Mock dependency isolation remains. ADR-0050 defines the portable host
  boundary and platform tiers; ADR-0058 separates ordinary checks from full
  and weekly robustness campaigns.
- The current Mock contract includes adopted SDL snapshots (ADR-0052), typed
  event sources and consumers (ADR-0057), and request-atomic behavior
  (ADR-0039). The [workflow version table](mock-project-workflow.md#supported-versions)
  names the current closed formats; old examples and historical ADR versions
  do not define additional supported readers.
- [ADR-0059](adr/0059-craftguild-standards-adoption.md) adopts pinned Craftguild
  rules v0.1.0 with engine v0.6.1 for CLI presentation, SBOM, Code measurement, build/install and release
  preparation. Its implementation remains in progress until the current
  product gates and standalone-source checks finish.

## Decisions and evidence

The [ADR directory](adr/) retains stable numbers, decision statuses and
supersession relationships. Normative contracts and explicit checked-in test
fixtures are the test oracles. The required sibling-generator assertion trace
and frozen migration fixtures remain public because current gates consume them.

Historical implementation prompts, evaluation notes and execution records are
preserved byte-for-byte with hashes in the private source archive. They are
outside the public build and test inventory. Historical Passed statements in
design documents retain their original scope; they do not prove the current
source passed. Run the current gates for new evidence. No private review state
or Beads record is a product-test oracle.

## Contributor workflow

Use [AGENTS.md](../AGENTS.md) and the [review workflow](review-workflow.md).
Track tasks and reviews in Beads. Implementers record Fixed; independent
reviewers record Verified and close findings. Do not create Markdown review
ledgers or use recorded verdicts as test inputs.

First-party prose is English under
[ADR-0043](adr/0043-english-first-repository-language.md), with its explicit
literal-name and Unicode fixture exceptions. Explicit `make setup` prepares the
engine fixed in [standards/engine.json](../standards/engine.json). `make prime`
renders its matching offline specifications; the
[local Skill](../.agents/skills/craftguild-standardize/SKILL.md) explains topics.
