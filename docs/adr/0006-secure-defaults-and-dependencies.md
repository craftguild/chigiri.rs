# ADR-0006: Use secure defaults and crate boundaries to minimize dependencies

Status: Accepted
Date: 2026-08-26
Last updated: 2026-08-27

## Context

GraphQL endpoints are vulnerable to resource exhaustion through document depth, fragment expansion, aliases, field concurrency, and large variables. The crypto-asset domain also requires avoiding accidental disclosure of schema or internal details and keeping direct dependencies small.

This phase focuses on dependency and feature minimization; cargo-vet, SBOM generation, and vendoring are not yet required.

## Decision

- Core and generator have no external normal dependencies.
- Isolate JSON in the HTTP endpoint's `serde_json`; Pythia owns no network listener.
- Keep Hyper, Axum, and Tokio in opt-in bridges and application examples with only required features.
- The production preset uses bounded limits and disables introspection and schema suggestions while masking internal errors.
- Version 1 always masks internal errors and does not expose a no-op error-policy API that appears configurable.
- Never infer development/production mode implicitly from environment variables.
- Apply document, parser, and validation limits to direct execution APIs as well as HTTP.
- Codegen discovery defaults to 256 KiB per source, 16 MiB aggregate, 1,024 sources, directory depth 64, and 65,536 total directory entries. It follows no symlinks and performs bounded reads.
- Before rendering, codegen limits GraphQL names to 255 bytes, iteratively counts constant-value nodes, and computes a conservative size estimate. It limits intermediate output to 100,000 items, final output to 65,536 files, unformatted and final bytes to 64 MiB each, and each rustfmt batch to 512 items or 2 MiB.
- Handwritten `.rs` and `.sh` files in framework, adapters, CLI, examples, tests, generated-source tooling, and repository scripts remain at or below 400 physical lines; the repository checker runs first in CI.
- First-party comments and rustdoc are written in English under ADR-0043. Security boundaries, limits, concurrency, and GraphQL invariants document both the mechanism and rationale.

Architecture is authoritative for security-limit defaults; [implementation rules](../implementation-rules.md) are authoritative for file limits and prose requirements.

## Error policy

- Parse, validation, and coercion errors use prescribed client-safe messages and codes.
- Only errors explicitly created with `FieldError::public` expose application details.
- Unknown/internal errors use fixed messages; original errors are available only to middleware observers.
- The production preset suppresses validation suggestions.

## Consequences

- A newly created framework endpoint is never unbounded by default.
- Large legitimate queries may require explicit limit tuning.
- A std-only core requires more implementation but is easier to audit.
- The 400-line rule increases module count while keeping review units and responsibility boundaries small.
- Users who do not select Hyper/Axum bridges do not depend on them or Tokio.
- Lockfile policy, deny rules, vetting, SBOMs, and vendoring may be added later but are not version 1 completion requirements.

## Revisit triggers

- Production traffic shows default limits are materially too strict or too loose.
- A persisted-operation allowlist is introduced.
- An organization-wide dependency governance policy is adopted.
