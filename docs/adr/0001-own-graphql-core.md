# ADR-0001: Implement the GraphQL core in-house

Status: Accepted
Date: 2026-08-26

## Context

The target product operates in the crypto-asset domain, so both runtime and build-tool dependency graphs should remain small. Pythia code generation, compile-time resolver completeness, and a purpose-built middleware and error model are also core framework values.

A GraphQL server has two layers:

1. a transport that converts between HTTP methods, headers, and bodies and GraphQL requests and responses; and
2. a GraphQL core/executor that parses and validates documents, coerces values, executes selections, dispatches resolvers, and propagates nulls and errors.

Apollo's Rust implementation, `apollo-compiler`, provides SDL and operation parsers and validators plus an [executor API](https://docs.rs/apollo-compiler/latest/apollo_compiler/resolvers/index.html) based on the `AsyncObjectValue::resolve_field` callback. Its ordinary field execution is currently sequential and has a source-level parallelization TODO. It also lacks stable insertion points for `FieldError` extensions and the required three-stage middleware pipeline.

## Decision drivers

- Keep external GraphQL crates out of runtime and build dependencies.
- Control concurrent I/O for Query fields and serial semantics for Mutation roots.
- Make Request, Operation, and Field middleware and error extensions first-class executor concepts.
- Explicitly target OneOf, introspection, and validation from the September 2025 specification.
- Keep the async runtime and HTTP stack outside the core.

## Considered options

### `async-graphql` dynamic schema

This provides a mature executor, HTTP integration, subscriptions, limits, and extensions. It would, however, require duplicate representations for Pythia's generated contracts and a dynamic-schema adapter and would substantially broaden core and runtime dependencies.

### `apollo-compiler` parser, validator, and executor

This offers Apollo's semantic schema and validation with fewer dependencies than `async-graphql`, but the current executor constraints do not satisfy Pythia's middleware, error, and concurrency requirements.

### `apollo-compiler` parser and validator with a custom executor

This lowers the cost of tracking parser and validation semantics but leaves part of the most complex trust boundary in a dependency and requires following upstream semantic-AST API changes.

### Fully custom GraphQL core

This has the highest implementation and verification cost, but allows dependency boundaries, diagnostics, execution, middleware, and security limits to share one coherent model.

## Decision

Implement these facilities in `pythia_core`:

- source maps, lexer, and SDL/executable parsers;
- schema composition and type-system validation;
- executable-document validation;
- variable, argument, and input coercion;
- Query/Mutation execution and value completion;
- introspection;
- Request, Operation, and Field middleware; and
- bounded document caches and execution limits.

[GraphQL September 2025](https://spec.graphql.org/September2025/) is normative. Version 1 supports Query and Mutation execution; Subscription, Federation, `@defer`, and `@stream` are out of scope.

The core is std-only and uses `#![forbid(unsafe_code)]`. JSON, HTTP, and runtime integration are isolated in adapter crates.

## Consequences

### Positive

- The core is independent of external GraphQL implementations' supply-chain and API churn.
- Middleware, error masking, limits, and concurrency can share one execution pipeline.
- Generated schema descriptors and executor internals can be optimized together.

### Negative

- Pythia maintainers own correctness of normative GraphQL algorithms and future specification updates.
- Parser, coercion, or null-propagation defects become interoperability and security issues.
- Version 1 is large enough that conformance work must take priority over feature growth.

## Guardrails

- Link every applicable row in `docs/spec-conformance.md` to a concrete test.
- Use specification examples as positive and negative fixtures.
- Bound parser recursion and diagnostic counts explicitly and never panic on invalid input.
- Test Query concurrency and Mutation seriality with controllable futures.
- Verify in CI that the core has zero normal dependencies.
- Do not mark a new GraphQL feature Supported until its conformance table, ADR, and tests exist.

## Revisit triggers

- Serious conformance or security defects recur in the custom core.
- An upstream implementation meets the dependency, middleware, and concurrency requirements.
- Subscription or Federation becomes mandatory and its in-house implementation cost is no longer acceptable.
