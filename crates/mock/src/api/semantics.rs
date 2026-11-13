//! Rules shared across model operations.

use super::{FailureClass, RuleSpec};

pub(crate) const RULES: &[RuleSpec] = &[
    RuleSpec {
        id: "call.receiver",
        description: "Call operations with colon syntax so the model is the receiver. The VM validates arity; options, input, and patch values must be tables.",
        failure: FailureClass::Capability,
        example: "transaction.caught",
    },
    RuleSpec {
        id: "scope.write",
        description: "find and count are read-only. Other operations require a mutation root. Nested mutation calls are read-only, and relation handles do not expose model connect or disconnect operations.",
        failure: FailureClass::Capability,
        example: "transaction.candidate",
    },
    RuleSpec {
        id: "query.closed",
        description: "An omitted, nil, or ctx.null where clause is unconditional. Otherwise use exactly one closed leaf, all-group, or any-group shape.",
        failure: FailureClass::Query,
        example: "query.predicates",
    },
    RuleSpec {
        id: "query.groups",
        description: "An empty all-group is true and an empty any-group is false. The root has depth one, every group is a node, and all branches validate before scanning even when the result is already empty.",
        failure: FailureClass::Query,
        example: "query.predicates",
    },
    RuleSpec {
        id: "query.slots",
        description: "A field is an exact stored scalar-slot name. Lists, DTOs, objects, arrays, JSON, inverse or virtual fields, and dotted paths are not searchable; names are case-sensitive.",
        failure: FailureClass::Query,
        example: "query.predicates",
    },
    RuleSpec {
        id: "query.operand",
        description: "Operands must match the stored scalar type, finite-number and exact-integer rules, enum membership, value constraints, and input budgets. Query operands do not use GraphQL argument coercion.",
        failure: FailureClass::Query,
        example: "query.predicates",
    },
    RuleSpec {
        id: "query.null",
        description: "Null or omitted record values do not match ordinary operators, including ne. Use isNull with a boolean operand; nil and ctx.null are invalid leaf operands.",
        failure: FailureClass::Query,
        example: "query.null",
    },
    RuleSpec {
        id: "query.contains",
        description: "contains applies only to String or STRING-represented scalars and performs case-sensitive substring matching.",
        failure: FailureClass::Query,
        example: "query.predicates",
    },
    RuleSpec {
        id: "query.order",
        description: "Apply filter, sort, offset, then limit. Sort keys have left-to-right priority, ties retain registration order, null sorts last in both directions, text uses UTF-8 byte order, and numeric precision is preserved.",
        failure: FailureClass::Query,
        example: "query.page",
    },
    RuleSpec {
        id: "query.options",
        description: "orderBy is a closed array of unique field/direction entries. Omitted or empty order preserves registration order. Offset defaults to zero, limit defaults to all matches, and both must be exact integers from 0 through 2^53-1.",
        failure: FailureClass::Query,
        example: "query.page",
    },
    RuleSpec {
        id: "value.null",
        description: "nil omits a field and ctx.null assigns explicit null. Updates replace top-level fields. Use dot-called ctx.jsonObject(table) and ctx.jsonArray(table) to preserve JSON container shape.",
        failure: FailureClass::Storage,
        example: "update.patch",
    },
    RuleSpec {
        id: "value.number",
        description: "Luau numbers must be finite f64 values. Integer representations must round-trip exactly, and GraphQL Int remains signed 32-bit.",
        failure: FailureClass::Storage,
        example: "query.predicates",
    },
    RuleSpec {
        id: "relation.noop",
        description: "Connecting an existing edge or disconnecting an absent edge is a validated no-op. Source and target selections validate first; no sources returns an empty list and no targets returns detached source copies.",
        failure: FailureClass::Relation,
        example: "connect.owner",
    },
    RuleSpec {
        id: "relation.shape",
        description: "Many-valued sides add one distinct edge and preserve remaining order on removal. A null list becomes a one-item list, removing the last edge yields an empty list, and required singular sides cannot be disconnected.",
        failure: FailureClass::Relation,
        example: "disconnect.owner",
    },
    RuleSpec {
        id: "relation.validation",
        description: "Relation edits validate owner storage shape, capacity, and constraints together with target existence, inverse cardinality, and uniqueness in the final candidate state. Inverse and implicit edits update the same canonical relation state.",
        failure: FailureClass::Relation,
        example: "connect.owner",
    },
    RuleSpec {
        id: "transaction.snapshot",
        description: "Queries and nested resolvers read the request snapshot; mutation roots read the latest candidate state shared by all models in that request.",
        failure: FailureClass::Storage,
        example: "transaction.candidate",
    },
    RuleSpec {
        id: "transaction.rollback",
        description: "Host, conversion, and budget failures remain sticky through pcall. Invalid returns, cancellation, and deadlines also prevent commit and roll back every model, relation, order, and index change.",
        failure: FailureClass::Storage,
        example: "transaction.caught",
    },
    RuleSpec {
        id: "resource.shared",
        description: "Value, host-call, and work costs share bounded call and scope budgets, including scans, sorts, comparisons, misses, and no-ops. Rust processing checks cancellation and deadlines; exhaustion returns no partial result. VM memory is not process RSS.",
        failure: FailureClass::Resource,
        example: "transaction.caught",
    },
    RuleSpec {
        id: "sandbox.globals",
        description: "Only documented globals and bounded standard functions are installed. require, filesystem, network, process, debug, dynamic loading, and coroutine capabilities are absent. pcall has a re-entry limit, and context/model tables are readonly.",
        failure: FailureClass::Capability,
        example: "transaction.caught",
    },
    RuleSpec {
        id: "reference.readiness",
        description: "Reference output describes the schema and API with readiness_checked=false. It does not prove seed consistency, module type checking, compilation, or successful service startup. User and Post examples are illustrative only.",
        failure: FailureClass::Output,
        example: "find.records",
    },
    RuleSpec {
        id: "unsupported.api",
        description: "The model API has no callback predicates, aggregate DSL, joins, upsert, or bulk input operations. Legacy directives, APIs, and formats are rejected. Migrate by initializing a new directory and moving user-owned scripts and seeds explicitly.",
        failure: FailureClass::Capability,
        example: "find.records",
    },
];
