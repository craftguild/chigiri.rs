# ADR-0005: Use associated types for domain binding and explicit null/undefined mapping

Status: Accepted
Date: 2026-08-26
Last updated: 2026-08-29

## Context

The generator must map GraphQL objects, inputs, enums, interfaces/unions, and custom scalars to Rust. GraphQL inputs in particular must distinguish omission, explicit null, and values after default application.

## Decision

### Output objects

Do not generate output DTOs. Applications bind domain types through `SchemaTypes` associated types, and object resolvers receive `&T::Object` parents.

### Inputs and enums

Input objects and enums are generated owned Rust types. A nullable input field or argument without a default uses `Input<T> { Undefined, Null, Value(T) }`. Nullable positions with defaults become `Option<T>` after coercion; non-null positions become `T`.

Generate OneOf input objects as Rust enums.

If a singular input-object edge belongs to a strongly connected component through direct or mutual recursion, box every singular field and OneOf payload within that component using `std::boxed::Box`. Although every valid cycle contains a nullable edge, consistent boxing of non-null edges too makes output independent of source order or feedback-edge selection. List edges already use `std::vec::Vec` and need no additional `Box`.

### Custom scalars

Applications bind custom scalars through `SchemaTypes` associated types and implement input parsing and output serialization with the framework's `Scalar` trait. `Scalar::serialize` may return any JSON-representable `Value`, including recursively nested nulls, primitives, objects, and lists. Built-in result coercion is unchanged. ADR-0015 depth, list-item, request-node, and HTTP-byte limits apply to custom containers.

### Interfaces and unions

Generate a generic enum whose variants are the possible concrete objects. It carries the runtime type and domain value together, avoiding string-based `__typename` registration.

## Considered options

### Generate every output DTO

This enables default property resolution but increases conversion, eager loading, and cloning between duplicate domain representations.

### JSON-like dynamic values

This simplifies generation but loses typed resolver signatures and compile-time schema-drift detection.

### Use `Option<T>` for every nullable input

This is concise but collapses omitted and explicit-null values and violates GraphQL semantics.

### Represent interfaces/unions as `(typename, Value)`

This is flexible at runtime but turns invalid type names and value/type mismatches into runtime errors.

## Consequences

- Applications can use existing domain aggregates or handles as GraphQL objects.
- Schema changes produce compiler errors in associated types, arguments/inputs, and resolver returns.
- Nullable inputs without defaults are slightly more verbose because they use `Input<T>`.
- Associated domain values must be owned and `'static`, or use `Arc` or an equivalent; borrowed values cannot be bound directly.
- Adding an interface/union possible type changes exhaustive matches on the generated enum.

## Compatibility rules

- Apply `T! -> T` and `T -> Option<T>` recursively in output positions.
- Fully qualify standard/prelude types and traits in generated signatures, including `std::option::Option`, `std::vec::Vec`, `std::string::String`, and `std::marker::Send`, so legal GraphQL names cannot shadow them.
- Deterministically escape `self`, `Self`, `super`, `crate`, and standalone `_`, which cannot be raw identifiers, with a reserved prefix and diagnose post-escape collisions.
- List items have no presence state, so nullable items use `Option<T>`.
- Apply input defaults before resolver invocation.
- ID is a canonical string newtype, Int is `i32`, and Float is finite `f64`.
