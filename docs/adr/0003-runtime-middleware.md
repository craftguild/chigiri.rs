# ADR-0003: Use a runtime-neutral three-stage middleware pipeline

Status: Accepted
Date: 2026-08-26

## Context

Authentication, authorization, tracing, metrics, error masking, and policy enforcement should be separate from resolver bodies. Pythia needs extensibility comparable to Apollo's plugin lifecycle without making the core depend on Tower, Hyper, or Tokio.

## Decision

Define a custom `Plugin<C>` and consuming `Next` in the core with three phases:

| Phase | Invocation point | Primary uses |
| --- | --- | --- |
| Request | after context creation, before parse/cache | request logging, rate/policy checks, request short-circuiting |
| Operation | after validation, selection, and variable coercion | operation authorization, complexity policy, operation tracing |
| Field | after argument coercion, before resolver | field authorization, metrics, result/error transformation |

Plugins enter in registration order and exit in reverse order. `Next` is consuming, non-`Clone`, and callable once. Every phase may short-circuit.

Application context `C` is created per request and shared inside the core by immutable reference. Plugins must be `Send + Sync` because Field phases can execute concurrently.

## Considered options

### Tower `Layer` / `Service`

This integrates well with the Rust ecosystem but fits field-execution lifetimes and values poorly and would leak runtime dependencies and types into the core API.

### Event observer only

This is sufficient for tracing but cannot authorize by short-circuiting, retry, or transform results and errors.

### Request middleware only

This is simple but pushes field-coordinate authorization, field metrics, and operation complexity policy back into resolvers.

### Hooks for every compiler phase

This maximizes extensibility but turns validator and AST internals into stable public API and makes specification evolution difficult.

## Consequences

- GraphQL middleware remains reusable across server adapters.
- Field plugins run concurrently, so authors must choose their own synchronization strategy.
- Parsers and validators are not replaceable. Request middleware may short-circuit before parsing; Operation middleware controls only validated operations.
- Custom executable-directive metadata is available in Operation and Field contexts so plugins can implement directive semantics.
- Panics remain application failures rather than being caught. GraphQL errors use `Result`.

## Ordering contract

For plugins A then B, successful execution is:

```text
A enter -> B enter -> next -> B exit -> A exit
```

If a plugin returns a response or value without invoking `next`, inner plugins and the terminal resolver do not run. Outer plugins still perform their normal exit processing.

## Revisit triggers

- Many plugins require per-request listener or state factories.
- Subscription event lifecycles are added.
- Interoperation with ecosystem middleware requires a Tower adapter.
