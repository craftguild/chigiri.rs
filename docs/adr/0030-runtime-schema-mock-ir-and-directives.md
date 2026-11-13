# ADR-0030: Building self-contained TOML/Luau from runtime schema and common Mock IR

- Status: Accepted
- Implementation: Implemented
- Date: 2026-09-08
- Decision owners: Pythia maintainers
- Related: ADR-0004, 0015, 0016, 0026~0029
- Supersedes: Replaces only ADR-0029's Mock-specific requirement for a fixed schema, fixed models, and a generated crate per schema.
- Generated ABI: Keep 6. The regular product and generated SDK retain MSRV 1.85; only Mock uses 1.95.
- Implementation goal: Goal for new agents

Planned successor: [ADR-0051](0051-mock-contract-validation-without-completion-state.md)
removes completion-state admission and metadata at its implementation cutover.
Intentional-error resolvers then become valid implementations without weakening
structural or runtime validation. The contract and results below retain their
historical scope; accepting the successor alone does not change the executable.

## 1. Background and goal

ADR-0029 demonstrated relation snapshots, mutation rollback, VM stopping, and recovery with a fixed blog TOML/Luau configuration, Rust host API, existing GraphQL executor, and volatile store.
However, User and Post, root names, Luau type declarations, argument conversion, and generated Rust resolvers were fixed. Replacing the SDL alone could not produce a mock for another service.

The next objective is for one compiled binary to create and run configurations derived from different SDLs.
Schema-dependent information therefore moves into an IR before AI is introduced, eliminating the need to implement and rebuild Rust for every schema change.
TOML contains editable Luau as well as declarative data. This does not create a
general-purpose VM or a separate expression language.

ADR-0029's Passed result remains historical evidence for the fixed-fixture execution infrastructure; it is not reused as evidence for this ADR.
Gates G-A through G-E are new and require their own evidence.
This ADR permits owned schema inputs and runtime dispatch without weakening the integrity of generated production traits.

## 2. Scope of this decision

| Item | Decision |
| --- | --- |
| SDL | Handle multiple sources, scalar, enum, object, input, list/null, interface, union, custom scalar |
| Operation | Support Query and Mutation. Explicitly reject schemas or operations containing Subscription; defer that capability to another ADR. |
| Specifying meaning | Use standard SDL custom-directive syntax to declare model, ID, relation, CRUD, and scalar constraints explicitly. |
| Relation | Support 1:1, 1:N, M:N, self-referential, and nullable relations; reject contradictory declarations during `init`. |
| Automatic generation | Generate static Luau only for explicitly declared CRUD operations. Do not infer business meaning from names. |
| Non-CRUD | Require an explicit manual resolver. Generate an unimplemented scaffold with expected types and refuse to serve while any scaffold remains unimplemented. |
| Seed | `init` creates an empty seed for every model. The user edits the TOML or creates records through Mutation. |
| Placement | Embed SDL snapshot in TOML and do not require original SDL/Cargo/external Lua to serve |
| Compatibility | Mock v0 compatibility is not required before publication. Old format/fixed fixture alias/fallback will not remain |
| Non-target | AI inference, random seed synthesis, persistence, authentication, hot reload, Subscription, production sandbox |

"Working with SDL" and "automatically inferring arbitrary business operations" are different.
Complex searches, authorization, aggregation, payments, and payload shapes are
expressed explicitly in manual Luau.
The GraphQL specification is not claimed to define Mock or CRUD semantics.
Custom-directive syntax is standard SDL; only the meaning of the small
vocabulary below is Pythia-specific. Do not embed a proprietary mini-language
in descriptions or comments, infer CRUD from field names, or fetch an
`@specifiedBy` URL.

## 3. One schema and two responsibilities

```text
SDL + mock directives
        │ bounded parse / compose / semantic validation
        ▼
owned SemanticSchema + Mock IR
        ├── model / relation / scalar / permission / expected type
        └── generate static ScriptCandidate → type-check → self-contained TOML
                                                │ edit
                                                ▼
serve: revalidate embedded SDL → same IR → inspect every script and seed
        → existing Endpoint/executor → Luau + Rust transaction/store
```
SDL is the sole source of truth for types and Mock semantics. Do not duplicate model or relation definitions in TOML.
IR is validated owned data inside Rust, not a DSL that users edit in a separate file.
TOML carries the schema snapshot, startup configuration, seed, resolver itself, and completed state.
Luau composes authorized host operations; it cannot change types, permissions, storage integrity, or budgets.

IR distinguishes at least the following:

- Source locations and GraphQL coordinates; every type wrapper; enums; input, default, and OneOf semantics; and abstract-type member sets.
- Stored models and non-stored value or payload objects within the process,
  including ID fields, stored slots, and computed fields.
- owner relation, inverse, target, singular/plural/null, required uniqueness, reverse index.
- Root operation type, arguments and return values, CRUD plan or manual plan, read/write capability.
- Custom-scalar JSON representation and constraints, expected Luau types, and conversion policy for GraphQL, store, and VM values.

The type graph is shared by ID reference, and no recursive clone or full relation expansion is performed.
Valid User→Post→User and self-references are accepted. The existing schema validator will reject invalid required cycles of input.

## 4. SDL directive contract

### 4.1 Vocabulary and closed meanings

The following table defines the initial vocabulary. Keep directive names, arguments, locations, and enum values in one checked-in SDL definition shared by the CLI, IR, and tests. Do not implement a second parser.

| Directive | Location / Meaning |
| --- | --- |
| `@pythia_mock_model(key: "id")` | OBJECT. Volatile store model. key is a non-null `ID!` or `String!` with no arguments |
| `@pythia_mock_relation(key: "authorId")` | FIELD_DEFINITION. owner side relation and its storage slot |
| `@pythia_mock_inverse(of: "Post.author", limit: "limit")` | FIELD_DEFINITION. Reverse lookup from owner coordinate. limit argument name is optional |
| `@pythia_mock_crud(action: FIND, model: "Post", id: "id", input: "input", limit: "limit", where: [...])` | FIELD_DEFINITION. Limited to root. Allow only action-specific argument mapping |
| `@pythia_mock_resolver` | FIELD_DEFINITION. computed/root field where users write Luau |
| `@pythia_mock_value(...)` | FIELD_DEFINITION / INPUT_FIELD_DEFINITION. Closed value constraints described below |
| `@pythia_mock_scalar(representation: STRING, ...)` | SCALAR. JSON representation and constraints described below. Required for all custom scalars |

Every directive is non-repeatable. `action` is one of
`FIND`/`CREATE`/`UPDATE`/`DELETE`, and every `where` element has exactly the
shape `{ argument: String!, field: String! }`. Do not apply implicit naming
rules to directive arguments other than model keys.

Supply directive definitions by adding the bundled v1 vocabulary during
composition. If input SDL defines the same name, accept only an equivalent
definition and reject conflicts. Include the vocabulary source in the TOML
snapshot and digest.
Regular GraphQL built-in and user directives are verified using existing validators.
Do not infer mock processing for other directives, and reject unknown `pythia_mock_` vocabulary.

The implementation syntax vocabulary is summarized in `crates/mock/src/schema/vocabulary.graphql`.
Equivalence testing does not confuse description/position/declaration order with meaning, and separates the namespace of directives and named types.
Keep the user source and add only unsupplied vocabulary definitions to the reserved logical source `pythia-mock/v1.graphql`.
If the entire vocabulary has been supplied, the same source will only contain comments, but the order and bytes will be included in the digest.
The vocabulary version is `pythia-mock-directives/v1`. When rebuilding, exactly match the appendix with the same binary.
Equivalence-confirmed vocabulary definitions are consistently excluded from the type/member budget. The physical byte aggregation of the user source before parse is maintained.

Each root requires one of CRUD/manual, and unmarked does not fallback to empty array or null.
Non-roots are uniquely classified into one of saved value projection, owner/inverse relation, and explicit manual.
Do not make non-root fields with arguments into simple stored value projections.
Fields between models must be specified as relation/manual, and records from another model will not be duplicated and saved as values.
Fields without arguments in non-model objects can be treated as DTO value projections, and depth/capacity is checked even when storing finite nested values.
Verify the solution using the concrete object of the dispatch destination, without ambiguously inheriting the meaning of the field declared in the interface.

### 4.2 Relation

Derive the target, cardinality, and nullability from the owner field's output
type. Initially, a stored relation may target only a concrete
`@pythia_mock_model` object; store its ID or list of IDs in the `key` slot.
A slot is a storage name distinct from the GraphQL field and must not conflict
with a key, field, or another slot in the same model. A stored slot name must
be non-empty. During `init`, reject only NUL, which cannot be represented as a
Luau type property.
Escape quotes, backslashes, newlines, other control characters and Unicode as string keys, and do not add implicit Name constraints.
Store a nullable singular relation as null and a plural relation as an ordered
list of IDs. Reject duplicate IDs and dangling references. Preserve the list
wrapper's nullability, and permit null items only when the item type is
nullable. Stored relation lists are limited to one level, so `init` rejects a
relation directive on a nested list. This does not prohibit nested lists in
ordinary values, payloads, or inputs.

- `inverse` refers to an existing coordinate that connects the target back to
  the owner. An inverse must not itself name another inverse as its positive
  side.
- If inverse is singular, impose corresponding uniqueness on the owner side. If owner is also singular, it represents 1:1.
- Owner singular/inverse plural represents 1:N, both sides plural represents M:N.
- Do not store independent ID arrays in both owner/inverse. Direct writing to inverse is prohibited.
- The owner column is returned in the save order, and inverse is returned in the registration order of the referencing model. The specified limit will be applied after filter.
- A nullable singular empty relation is null, and a non-null singular requires exactly one corresponding record in the final state.
- `[T!]!` does not prohibit empty arrays. If you need a minimum number of items, use an explicit `minItems` constraint.
- Only fields requested in selection are traced. Do not fully expand JSON type cycles.

Existence/type/direction/cardinality/argument inconsistency in declaration is rejected at **init**.
Multiple inverses pointing to the same owner cross the lower and upper limits of the number of referencing records for each target in the final state.
For a singular relation, the range is 0–1 when nullable and exactly 1 when
non-null. For a list without a mapped limit, apply `minItems` and `maxItems`.
The maxItems of a list with a mapped limit is an output check after the limit, and is not the upper limit of the entire original edge.
This local declaration matching is not a satisfiability solver for arbitrary
population ratios across multiple models. Validate actual ID existence,
uniqueness, minimum cardinality, and related constraints against the complete
seed and each Mutation candidate state. To allow mutually required records to
be created and connected in one Mutation, references to IDs created later in
the same candidate may be deferred until final validation. A read of an
unresolved relation is an error, never a false success or null. Reject type,
unknown-field, and authority errors immediately; reject final reference or
uniqueness violations at commit.
This is a decision to change the immediate check for the existence of references in ADR-0029 to the final consistency check for general-purpose transactions.
Required mutual owner relationships may be created with a temporary non-empty reference ID and updated to a numbered real ID within the same script.
If the temporary ID remains in the final state, it will always fail. There is no mechanism to erase the already recorded host error later.

Deletion uses RESTRICT. An ID referenced by another record cannot be deleted
unless the candidate state first removes that reference explicitly. A record
with only a self-reference may remove the record and edge together. There is
no implicit cascade, linked deletion, or silent removal of related IDs.
Fields returning abstract types are supported, but this ADR does not add
vocabulary for targeting an interface or union with a stored relation. Such a
field uses a manual resolver that returns a concrete model record or value
object and validates its membership.

### 4.3 CRUD and model API

| Action | Permissible root shape/meaning |
| --- | --- |
| FIND | Query, model list output. id/where is equal AND, unspecified/null means no filter, returns array after limit |
| CREATE | Mutation, single model output. Create a saved value from an explicit input argument, and Rust assigns an ID |
| UPDATE | Mutation, model single output. Required id and input argument. Omission is maintained, explicit null is allowed only if the save slot is nullable |
| DELETE | Mutation, Boolean output. Required id. false if absent, true if present and can be deleted |

To obtain a single FIND result, use the same `find` API from a manual script;
do not add separate `findOne`, `findAll`, or `upsert` APIs. `where` supports
only simple equality against scalar, enum, or ID slots. During `init`, reject
unknown fields, duplicate mappings, and type mismatches.
Each CREATE or UPDATE input field name maps to a storage slot. During `init`,
verify input/default compatibility with the stored type, required items, and
whether IDs or references may be changed. Even if the patch input field is
nullable, reject explicit null when the destination slot is non-null. Both
operations' mapped input arguments must be non-null input objects, and CREATE
must provide every required storage slot.
Only the outermost field is relaxed to nullable in UPDATE, and list items and wrappers of nested DTOs satisfy the saved type.
DTOs are whole value replacements and do not interpret nested fields as implicit deep patches.
If you need a payload wrapper, name conversion, complicated filter, etc., use manual. Do not add a general mapping expression language.

The only host API that is common to all models is `find/create/update/delete`, and it is independent of the number of public roots of GraphQL.
Query/nested context has only find, Mutation root context has four operations. Rechecking permissions in Rust.
Use it like `ctx.models["User"]:find({ where = { name = "Alice" }, limit = 20 })`.
find maintains registration order, default 20, and limits of 0 to 100, and does not clamp negative/exceeding values.
create/update returns a copy of the saved record, delete returns bool. The ID is immutable, and the numbering and registration order are also subject to transactions.
The ID is unique for each model, non-empty, and less than 256 UTF-8 bytes. Maintain seed ID reservation and numbering restoration upon rollback.
Numbering is a definitive string based on the regular GraphQL model name and monotone counter, and collisions with seeds are skipped.
Counter overflow is an error and does not fallback to a random number or counter of another model.

For general-purpose store numbering, use `Model-counter` if the model name, hyphen, and 20 digits of U64 decimal digits fit within the key limit.
For long model names or short String keys, switch to a prefix within the upper limit and a base36 counter of up to 13 digits.
The base36 alphabet is rotated definitively from all bytes of the regular model name and is not treated as an authentication hash or random number.
Any digits missing from minBytes are padded to the left with zero digits in that format. Do not temporarily generate IDs that exceed the key constraints.
If the ID space for a short key is used up, it will also be treated as an explicit error, and the counter/registration order will be discarded along with the failed transaction.

### 4.4 Scalar, value constraints, missing and null

`representation` is a closed enum: STRING/BOOLEAN/INTEGER/NUMBER/OBJECT/ARRAY/JSON.
JSON is any **finite and bounded JSON value**, and does not mean that it can be passed through Lua functions or userdata.
Do not infer DateTime/UUID etc. from custom scalar name. A scalar that does not specify a representation is rejected by init.
`@specifiedBy` is retained as metadata; codecs and validation functions are never acquired externally. Initial constraints are `min`/`max` (finite number), `minBytes`/`maxBytes` (UTF-8 string),
`minItems`/`maxItems` apply only to lists and arrays. Integer length values are non-negative and require `min <= max`.
Reject constraints on expressions that cannot be applied, unknown items, duplicates/contradictions, and requests that exceed fixed resource limits at init.
min/max applies to Int/Float or INTEGER/NUMBER, bytes applies to String/STRING, items applies to list/ARRAY.
These shape-specific constraints are not attached to mixed JSON representations, and common depth/node/byte limitations are applied.
Do not add format name, regex, optional Lua scalar codec, or JSON Schema interpretation.
Handle any necessary additional business constraints with a manual resolver, but do not weaken Rust's expression/constraint checking.
A field constraint applies to that value and does not implicitly propagate to all items in the list. Both scalar constraints must be satisfied.
INTEGER/Int requires that an integer exists in the intersecting interval, and Int requires that it also intersects with an existing i32 range.
Rather than uniformly narrowing INTEGER to ±2^53, integers that can be expressed more accurately are still allowed.

Apply the same Rust constraints to input literal/variables/default, seed, host write, and return value.
GraphQL Int has an existing 32-bit range. When passing an integer to Luau's number, only values that can be expressed accurately are allowed,
Do not round, truncate, or unconditionally convert large integers to f64. If you need a large integer, use an explicit STRING representation.
Owned custom scalar decimal/exponent literals also match the digits before rounding if the original decimal representation is a mathematical integer.
Decimal values that are mathematically fractional, together with generated or
built-in Float values, retain the existing finite-`f64` conversion rules.
Custom scalar output goes through ADR-0015 JSON container/null and node/depth/bytes inspection.

Value conversion distinguishes model records, non-model DTOs, and scalars;
Luau type checking alone is not authoritative at this boundary.

- Missing field/argument: unset or nil. Explicit null: `ctx.null` sentinel owned by host.
- nullable list and null item: Follow the schema wrapper and do not equate to Lua arrays with holes.
- Empty object and empty array: Distinguish by expected schema type. If you are unsure about JSON scalar etc., use an explicit host constructor.
- object/list cycle, function, unknown userdata, extra record field, non-finite value: conversion failure.

The null/JSON constructor only gives a representation of the value. Do not expose additional I/O, privileges, or mutable shared state.
Also reject nils in positions that return a single value as missing, and use `ctx.null` for nullable results.
`ctx.jsonObject(table)`/`ctx.jsonArray(table)` is a host-owned marker that indicates the shape, and the contents are inspected when used.
Add a shape to the container copied from Rust to the VM, so that empty objects/arrays can be round-tripped.
The codec accepts closed values that preserve VM affiliation and never passes raw strings or other values to functions in another VM.
When accepting a table, setting a JSON marker, or calling a resolver function, the VM to which it belongs is checked first. Also check scope failures again before committing.
The boundary where fixed luaur 0.1.8 saturates and reads VM's `2^63` to `Integer(i64::MAX)` is restored to the original value with the adapter.
`i64::MAX` itself is a limited compensation for the fact that it cannot be expressed in VM's f64, and does not allow rounding of Rust integers.

## 5. SDL example for blog fixture

The following is the user SDL that is combined with the included directive vocabulary, and represents traditional blog operations using explicit metadata.
authorId is a storage slot and does not save the author of GraphQL output twice.
```graphql
type User @pythia_mock_model(key: "id") {
  id: ID!
  name: String! @pythia_mock_value(maxBytes: 256)
  posts(limit: Int! = 20): [Post!]!
    @pythia_mock_inverse(of: "Post.author", limit: "limit")
}
type Post @pythia_mock_model(key: "id") {
  id: ID!
  title: String! @pythia_mock_value(maxBytes: 256)
  body: String! @pythia_mock_value(maxBytes: 4096)
  author: User! @pythia_mock_relation(key: "authorId")
}
input CreateUserInput { name: String! }
input CreatePostInput { title: String!, body: String!, authorId: ID! }
input UpdatePostInput { title: String, body: String, authorId: ID }
type Query {
  getUsers(limit: Int! = 20): [User!]!
    @pythia_mock_crud(action: FIND, model: "User", limit: "limit")
  findPost(id: ID, authorId: ID, limit: Int! = 20): [Post!]!
    @pythia_mock_crud(action: FIND, model: "Post", id: "id", limit: "limit",
      where: [{argument: "authorId", field: "authorId"}])
}
type Mutation {
  createUser(input: CreateUserInput!): User!
    @pythia_mock_crud(action: CREATE, model: "User", input: "input")
  createPost(input: CreatePostInput!): Post!
    @pythia_mock_crud(action: CREATE, model: "Post", input: "input")
  updatePost(id: ID!, input: UpdatePostInput!): Post!
    @pythia_mock_crud(action: UPDATE, model: "Post", id: "id", input: "input")
  deletePost(id: ID!): Boolean!
    @pythia_mock_crud(action: DELETE, model: "Post", id: "id")
}
```
The second fixture should be a separate domain such as Project/Member/Task. 1:1, M:N, self-reference, nullable, enum,
Pass interface/union output, custom scalar, non-model payload and manual resolver through real HTTP.
Furthermore, pass the SDL with the type/root/field names renamed in bulk to the same execution binary to run it normally.
No ordinary Rust branch detects a fixture name or special-cases `User` or
`Post`.

## 6. CLI, self-contained TOML, editing lifecycle

```sh
pythia-mock --help
pythia-mock --version
pythia-mock init --schema models.graphql --schema operations.graphql --out mock.toml
pythia-mock serve --config mock.toml
```
The `init` schema and output arguments and the `serve` configuration argument
are required. Preserve schema-argument order as source order, and reject
duplicate sources.
Implicit search, individual override of CLI/env, and fixed blog fallback due to old `init --out` will not be left.
The input is a bounded regular UTF-8 file. Maintain existing link/FIFO rejection, inspection/read with same descriptor, output non-overwriting.
Only after every schema, IR, and generated candidate passes validation does
`init` exclusively create the output TOML file.
Do not leave empty/partial TOML due to failures such as incorrect relation, and do not change existing output.
If the input bytes/source order/binary are the same, the output bytes will be the same, and the time, random number, and absolute source path will not be included.

The new format is `pythia-mock/v1`. Every level is closed and rejects unknown,
duplicate, or missing fields. Required sections are `format`, `server`,
`schema`, `seed`, and `resolvers`. The `schema` section records every source's
logical name and UTF-8 text, along with the SchemaDigest computed by the
existing method and the Mock vocabulary version.
The original file is converted to a logical name and embedded, and is not used as a read destination when serving.
The digest is a consistency check, not tamper authentication, and is
recalculated at startup. There is no separately editable IR field.

seed stores a JSON object with the model name as key in a TOML string called `seed.records_json`.
The reason for using JSON is to directly represent null/empty containers that cannot be represented in TOML, and is not a second schema.
init outputs an empty array to all models, and uses the exact name of GraphQL/storage in records. Don't put snake_case alias.
Reject unknown model/field, duplicate JSON key, ID duplication, invalid expression, and final reference inconsistency before listening.
Verify the relation after reading all the models, and do not change the validity depending on the order in which the seeds are written.

The resolver table actually matches the exact coordinate required by the script.
Each entry has `state = "ready" | "unimplemented"` and `luau`. Below is an excerpt of the structure, not the complete config.
```toml
format = "pythia-mock/v1"
# init writes the server settings and embedded schema.
[seed]
records_json = '{"User":[],"Post":[]}'

[resolvers."Query.getUsers"]
state = "ready"
luau = '''
return ctx.models["User"]:find({ limit = args.limit })
'''
```
CRUD generates a ready static body, and manual generates an unimplemented expected typed scaffold.
init reports the number of unimplemented items and coordinate and succeeds, but serve does not bind if even one item is unimplemented.
Always check syntax/expected type/compile even after editing to ready. Do not use only TODO string search to determine completion.
Don't make non-CRUD "implemented" with automatic empty array/null/error responses.

serve rebuilds the IR from the embedded SDL, inspects script/seed/config, then listens. Do not rewrite TOML.
The server can start without network access or Cargo when only the binary and TOML are moved to a directory that does not contain the original SDL.
To change SDL, init from the original SDL to another new TOML, and explicitly migrate Luau/seed by the user.
Automatic merge, update of edited TOML, and hot reload will not be added this time.
help/version, exit 0/1/2, numeric loopback, CORS, port 0 maintains ADR-0029.

## 7. Owned runtime schema connection to existing executor

Use the existing core's parse/compose/validation and SemanticSchema, and do not generate a Rust crate for each schema.
Add the minimum necessary owned path to the boundary between `SchemaDescriptor: &'static` and the static type name of `ResolvedObject`.
The specific Rust API name will be determined at the time of implementation and recorded in rustdoc, but the following are required.

- The runtime schema source is explicitly different from the generated descriptor, and the owned schema verified before startup is maintained.
- Do not use `Box::leak`, unsafe, process-global schema registry to satisfy `'static`.
- Maintain generated trait/typestate/ABI fingerprint matching. Don't fake dynamic input as an ABI 6 generated trail.
- Dynamic field lookup for mocks checks coverage/type in advance with common IR. Normally, generated consumers are not made dynamic.
- Associate schema, dispatch, custom scalar, and cache for each schema instance, and do not mix types/records from different instances.
- Connect to the final component graph of a single Endpoint builder. Don't create a separate HTTP executor or your own query evaluator.
- Pass alias/fragment/variables/default/OneOf, abstract completion, null bubbling, HTTP/error masking, and limits through existing routes.
- Distinguish model objects from storage records + snapshots, and non-model objects from typed bounded values.
  The result of an interface/union is checked for specific `__typename` and member matching in Rust, and does not fallback on unknown types.

Changes to core/runtime/façade/testkit necessary for this purpose are permitted by this ADR.
Core retains zero normal dependencies, Rust 1.85 compatibility, generated ABI 6, and the integrity of normal consumers.
The existing "dynamic registry prohibition" continues to protect production generated resolver/plugin contracts.
The dynamic-registry prohibition does not block the owned Mock schema path defined here, but the exception remains limited to the documented test and Mock boundary.

### 7.1 WP-RSM1 implementation API

`pythia::experimental::OwnedSchema::parse` inspects the logical source with the existing parser/validator,
`OwnedSchema::bind` matches dispatch, all concrete object fields, and all custom scalar registrations of the same instance.
Pass the returned `OwnedSchemaInput` to the regular `Endpoint::builder`. This entry point is separate from generated descriptor and ABI verification and connects to the core shared field-execution adapter. The schema type graph and verified scalar registrations are shared through `Arc`; each Endpoint owns its cache.
`OwnedSchema::object` connects a known concrete type to an owned scope, and completion also checks the same scope and abstract member set.
Store each concrete object type name once in the schema and share its `Arc` across objects. Do not allocate type-name bytes once per result.
The exact coordinate set of dispatch is also a slice of `(Arc<str>, Arc<str>)` so that the same parent type name can be shared without duplicating it for each field.
Distinguish between Generated and Owned with `SchemaInfo::source_kind()`. `generated_abi()` becomes `Option<u32>`,
Generated is `Some(6)`, Owned is `None`. This metadata accessor source-level change is not a revocation of ABI 6.
`ResolvedObject::type_name()` returns a `&str` borrowed from the object. Preserve the static constructor.

To prevent default-value amplification from newly exposed owned SDL, startup validation and each owned request share a coercion-work budget of 100,000 nodes and 16 MiB of logical bytes.
Include default expansion and value duplication through variables, arguments, directive arguments, and aliases in the same request budget.
Once exhausted, the budget remains exhausted for the current request; the next request receives a fresh budget. Existing APIs and limits for generated routes remain unchanged.
Logical size counts the UTF-8 bytes of strings and keys, one byte for booleans, and the greater of eight bytes or the integer string length for numeric values.
Reserve retained coercion diagnostic messages and rendered paths against the same byte budget before allocating their strings.
When the limit is exceeded, discard previously accumulated diagnostics, emit one short fixed `INPUT_RESOURCE_LIMIT`, and stop coercion.
If the diagnostic upper limit is 0, the text is not retained and only the failure status is maintained.
Do not underestimate integer conversion to ID/BigInt, and count null items as one node. The wrapper itself does not increase the number of nodes.
Scalar-container conversion also counts as work. Constant-count copies such
as public argument mirrors and indexes are bounded, but the logical-byte
budget is not an RSS guarantee and excludes allocator and index overhead. Use
the same scalar callback while validating defaults. Do not truncate long
GraphQL type names. Refer to the owner and field in default-cycle validation
by ID, and let possible-object indexes share `Arc` type names to avoid repeated
cloning. Executable-validation input and output types, variable sites, and the
field/fragment/merge work queue likewise borrow type names from the original
schema or document. Prevent retained memory from growing as the product of
selection or list-item count and type-name bytes, including before request
coercion begins.

Owned schemas also apply a shared budget of 100,000 items and 16 MiB to cost metadata before execution. Reserve type names, coordinates, argument names, and type signatures before copying them. If this budget is exceeded before resolver execution, reject the operation with `BAD_USER_INPUT` and no data. Offline operation-metadata checks use the same limit.
Execution has a separate 16 MiB logical-byte budget for copied response keys, type names, leaves, paths, retained error messages, and public directive metadata. Inherited directive sites share `Arc`, and error accumulation stops when the input budget is exhausted.
Descriptions and `defaultValue` fields from explicitly enabled introspection are charged to the same budget before copying or sequential rendering.
Exceeding this byte budget fails the entire operation, discards partial data, and returns `data: null`.
The response contains one masked `INTERNAL_SERVER_ERROR` without a path or location. Do not duplicate excessively long paths in errors.
This owned-schema-only contract is separate from the existing per-nullable-field node and depth restrictions and does not apply to generated routes.
This is also not a substitute for RSS, which includes allocator/index, etc., or for HTTP's encoded response byte limit.

## 8. Ownership of Luau, Status, Cancellation

Expected type declaration and wrapper are generated from IR and matched with model/input/argument/return value/Query/Mutation capability.
Validate keywords/collisions/escaping instead of embedding GraphQL names into Luau identifiers without inspection.
Recursive types are expressed by references, and creation declarations are not expanded indefinitely. Do not use saving to `any` as evidence of successful type checking.
The body is syntactically checked as an independent statement string, and the expected type wrapper is not escaped with extra `end` etc.
After determining the capacity of each wrapper and set, start the compiler/typechecker, and return individual failures to the corresponding coordinate.
Null is a nominal type, and omitted input is distinguished as nil. The abstract DTO type name literal can be used as needed.
Specify the singleton type like `__typename = "Packet" :: "Packet"`. This is not a cast that skips concrete type validation on the Rust side.
The outermost patch of UPDATE can express null based on the type according to the input SDL, but Rust rejects explicit null in a non-null storage destination.
Apply Rust type/permission/constraint/return value checking to Luau casts, table modifications, and caught exceptions.

If the root type also appears in the concrete/abstract return value of field, record `root_as_nested` in the IR.
The coordinate wrapper has `parent: Parent?`; root execution receives `nil`, while nested execution receives a typed parent.
For a Mutation coordinate, the expected context is `ReadContext | WriteContext`, and read-only `ctx.scope` distinguishes `"read"` from `"mutation"`. Write operations are exposed only after narrowing with `ctx.scope == "mutation"`.
The expected type of Query is always ReadContext. Static CRUD candidates also reject writes when nested with an explicit error,
manual can branch root/nested in the same body. Don't infer write permission just from the presence or absence of a parent.
Rust sets the scope value to the read-only context owned by the VM, and rechecks the permissions of the actual Session even after casting or spoofing the value.
Do not prohibit the root type from nested output or present only an unconditional write type to the same coordinate.

The adopted `luaur 0.1.8` VM generator mutates a process-wide flag non-atomically. Hold a shared process mutex from Mock or Lua compilation and type checking until all handles are destroyed.
The mutex is not a global registry and does not retain schema, dispatch, or VM handles. The existing execution limit is 500 ms, including absolute-deadline and cancellation or shutdown checks; expiry fails without commit. Type-check admission waits at most five seconds.
Multiple workers in one process still cannot run VM creation or type checking concurrently. Preserve the CLI's single-worker, finite-queue contract.
Native assertions may abort on concurrent flag changes during type checking, so `catch_unwind` alone is not considered sufficient recovery.
Prevent reuse of the same fixed version when third-party sources or the VM change. Environment inspection also returns only Rust values and never exposes the VM handle.
Copying and altering model records and fictitious IDs will be rejected, and non-model DTOs will be inspected separately using SDL's expected type.
The model copy also matches the presence of the save slot and rejects computed/surplus fields. The difference between object insertion order and exact numerical representation is the same.
The DTO contains only a value projection and does not source computed fields from the script. Associate every output object with the inspected snapshot. One worker owns the store and VMs, controls operation admission, and creates a fresh VM for each root.
The nested manual script is also read-only and passes the same snapshot associated with the parent result.
Pass `ctx` and coerced `args` to the script body, and also pass a type-checked
`parent` for nested execution.
Parent cannot be used by root. Type declarations distinguish between GraphQL output types and storage record types.
Do not expose Luau handles to Send boundaries and do not run synchronous VMs on HTTP runtime workers.

Mutation writes accumulate in candidate state containing every model, registration order, ID reservation, counter, and relation index. Exchange that state exactly once, after script termination, Rust return-value conversion, full consistency checks, and cancellation/deadline conflict checks all succeed.
Host error, memory/time/host-call/pcall re-entrancy excess is treated as a sticky failure and refuses to commit even after being caught.
Do not combine multiple mutation roots into one transaction and do not roll back nested/HTTP failures after commit.
Do not switch from snapshot to live midway. A read-only context also shares the budget and failures of lookups/relationship traversals/return value checks within a single call.
Final relation validation may seal a candidate before result validation, but
the exchange occurs only after both success and cancellation checks complete.
Closed candidates also retain the host's remaining value budget and return value failure, and reject commits of failed candidates.
The detailed revocation/commit agreement inherits from ADR-0029 Section 6.
The same instance of a revocation token is locked only once, and two different tokens are acquired in full order of addresses.
Passing the same token or an oppositely ordered pair to root and shutdown must not deadlock. Recheck both cancellation states and deadlines immediately before exchange.

The future AI boundary is `validated IR + expected resolver contract → ScriptCandidate`.
This change only separates the pure static template function from the inspection entry point.
AI/provider/plugin framework, download, model weight, and prompt optimizations are not implemented.
Future AI-generated candidates cannot change the schema, host permissions, or budgets and must pass the same type and Rust runtime checks.

## 9. Boundedness and non-goals

Maintain ADR-0029's limits of a 16 MiB VM, 500 ms per root, 256 host calls, 16
simultaneous `pcall` reentries, a 16-item worker queue, and 5 seconds for
operations and shutdown, together with the existing Endpoint body, output,
depth, and response limits.
Restore `pcall` depth on normal and exceptional returns, and reject the seventeenth level before re-entry. Test this in both debug and release builds.
Don't mistake the VM budget as the upper limit for Rust store, compiler/typechecker, or process RSS.

Due to generalization, the following fixed upper limit will be added to the initial implementation. There is no option to remove the upper limit.

| Boundary | Upper limit |
| --- | --- |
| TOML | 256 KiB as before. Total including embedded SDL/seed/script |
| SDL for init | Total 128 KiB excluding vocabulary, 64 sources. Aggregate before read/parse |
| Mock IR | model64, user named type256, field/argument/input member total 4,096, script coordinate256 |
| store | 1,000 records per model, 2,000 records total for all models. Logical bytes of all saved values/IDs/edges total 16 MiB, total saved nodes 100,000 |
| Save string | 4 KiB without field constraints. ID is 256 bytes. Prefer smaller explicit constraints |
| Value conversion | Candidate values are limited to depth 32 and 100 list items. Transactions also obey the same store limits and use checked arithmetic. |
| Value verification work | Each shared scan of input/default/save value/VM conversion/return value 100,000 nodes, 16 MiB logical bytes. Once exceeded, the same budget will not be restored |
| Rust store traversal | 1,000,000 work across root candidates. record/filter/side/index check is shared, excess is sticky failure |
| Luau generation | Main body 32 KiB per coordinate, each wrapper including common declarations 1 MiB, aggregate output 16 MiB |

The logical-byte budget is not a process RSS guarantee. The number of simultaneous copies in indexes, working state, snapshots, and the VM is also bounded.
In mock value validation, the root depth is set to 1, wrappers are not counted as nodes, and null/empty containers are also counted as 1 node/1 byte.
string counts UTF-8 bytes, bool counts 1, and numbers count 8 bytes (decimal lexeme bytes if holding BigInt).
The object key includes UTF-8 bytes once in addition to the value. Include the JSON key and the field name of the saved DTO in the budget.
The startup default is expanded once in another shared budget (Section 7.1) of the existing coercion, and the input and CRUD storage destination constraints are matched with the same value validation.
Include argument default in the directive definition in the same budget and check the constraints of nested input fields.
Sparse input/DTOs only check fields in the value and pre-planned required/default fields, preventing repeated traversal amplification due to type width.
Store create, update, and seed operations calculate the total-state capacity delta while borrowing the current state and reject excess before copying records or allocating index entries.
Include find options/where container, key, null filter, and explicit limit in the shared value budget.
Reading an empty relation and limit 0 also counts at least 1 work in the entry field/record/index lookup.
The codec measures the length of the LuaString using a standard C function and reserves the node/key/byte before copying the UTF-8.
Since the raw iterator of Lua table also traces free slots, the entry list for each table is shared within one decode, and the capacity is scanned at once.
The cache entry handle counts towards the same conversion budget as 1 additional node/byte and does not carry over the cache to the next host call.
Total simultaneously retained table capacity is limited by the VM's 16 MiB budget. Cycles are rejected, and values reached through a shared DAG are copied only within the inspection budget.
The string limit is not relaxed only with the key name `__typename` in JSON, and the exception of long regular typenames is limited to schema-derived metadata.
Direct VM allocation failures are also treated as sticky conversion errors, and the VM is not reused to the next resolver.
An inverse relation without an explicit `maxItems` or query limit may store more than 101 edges. Reading the field still fails value conversion when it exceeds the 100-item result limit. Do not truncate edges silently or attempt to undo an earlier commit during the read.
Check each entry of read/parse/IR generation/conversion instead of restricting the implementation only after TOML/JSON/SDL is fully expanded.
Restrictions also apply to recursive graphs, type declaration amplification, relation traversal via alias/fragment, and Rust host scan.
If the fixture cannot pass without weakening the existing budget, first check the expression/duplication allocation/algorithm.

Full disclosure of standard library, bypass from metatable, require/load, file/network/process capability are not added.
Tests for infinite loops, deep `pcall` nesting, and excessive allocation protect the runner with an external watchdog and never count an external kill as a normal return.
Hostile multi-tenant, complete isolation of arbitrary scripts, and support for arbitrary large schemas are not guaranteed.

## 10. Achievement Gate

Every ID maps to a concrete test, fixture, command, and assertion. Gates G-A through G-E and RSM-01 through RSM-22 passed locally on macOS.
Independent reviews A and B reported no unresolved findings. The implementation and validation record contains the complete ID trace, the 66-command common validation, post-change measurements, unexecuted CI environments, and source comparison. It does not reuse ADR-0029's Passed result as evidence.

| Gate / ID | Required demonstration |
| --- | --- |
| G-A/RSM-01 | Multiple SDL composition, IR of type wrapper/default/OneOf/enum/interface/union. Subscription Explicit Rejection |
| G-A / RSM-02 | Directive closed/location/conflict, root coverage, ID/slot/CRUD mapping mismatch is rejected at init/output unchanged |
| G-A / RSM-03 | Accepted correct declaration of 1:1/1:N/M:N/self/null. Reject contradiction of unknown target/inverse/type/orientation/uniqueness declaration |
| G-A/RSM-04 | Custom scalar expressions/constraints, JSON all shapes, reject unknown/not applicable/contradictory. Do not get external URL |
| G-B / RSM-05 | Execute real HTTP for all blog CRUD/bidirectional relations and another domain in the same binary. No Cargo/Rust branching by schema |
| G-B / RSM-06 | Works with the same binary even if the root/type/field name is renamed. Check Luau keyword/name collision/escaping |
| G-B / RSM-07 | manual root/nested, payload, interface/union concrete types and fragment, wrong typename rejection |
| G-B / RSM-08 | Correctness of expected typed script, static/dynamic rejection of Query/nested write, original Luau positioned redacted diagnosis |
| G-C / RSM-09 | find AND/order/limit, all models CRUD, omit/null, ID unchanged, absent, full width numeric/precision bound |
| G-C / RSM-10 | Two-stage verification of seed. Duplicate key/ID/unknown model, expired reference, uniqueness, cardinality, order independent |
| G-C / RSM-11 | 1:1 reconnection/M:N addition/deletion/self/cycle, inverse synchronization, RESTRICT, establishing required relationships in one transaction |
| G-C / RSM-12 | Restore all state/order/counter/index with normal/captured host error/invalid return/cancel after multiple model write |
| G-C / RSM-13 | Snapshot consistency, multiple Mutation roots, nested failure/disconnection after commit does not accidentally rollback the preceding commit |
| G-C / RSM-14 | Rejecting missing/null, empty object/array, null item, container scalar, cycle/surplus field/imaginary record |
| G-D/RSM-15 | loop/memory/host-call excess and debug/release pcall16/17. Non-commit after capture, return to same server and collect worker |
| G-D / RSM-16 | bounded parse/IR/declaration/all store/conversion, bounds exactly and one beyond. Do not replace amplification on the Rust side with VM limits |
| G-D/RSM-17 | busy/queue, early cancellation, single shutdown deadline/join, CORS/loopback/unauthorized capability |
| G-E / RSM-18 | init deterministic/non-overwrite/no-output failure, explicit unimplemented state, bind only after all ready verification |
| G-E / RSM-19 | Serve offline with only binary+TOML destination, no need for original SDL/Cargo, reject/drift/old format, no config change |
| G-E/RSM-20 | Instance separation/drop of owned schema, completeness of existing typed ABI 6, HTTP/GraphQL/observer/limit regression |
| G-E / RSM-21 | Existing 1.85/mock1.95.0/stable, zero core dependency/No Luau mixed, all SDK exact/quality, common validate and conventions |
| G-E / RSM-22 | Independent Review A/B, full ID trace, execution examples, measurements, handover of unexecuted CI/untracked artifacts and subsequent AI boundaries |

Add to common `validate.sh` and check to ensure that failure/skip/missing of required mock check does not result in aggregate passing.
During implementation, use focused tests and cache, and put the full suite in the final source stage.
Preserve the 400-line handwritten-file limit, the English prose rule from ADR-0043, semantic partitioning and ownership inspection for generated files, and the prohibition on manual edits to generated output.
Do not present Luau output from a running Mock as a permanent Rust SDK or exempt handwritten Mock code from the line-count checker.

Do not rewrite external applications in this work. SSH acquisition, package release, a clean commit, push, publish, tag, and extended performance or fuzz campaigns are not additional completion gates. Record any external CI that was not executed and distinguish it from local success.
A record of completion will be created in `docs/runtime-schema-mock-validation.md`, and the trail of ADR-0029 will be left as a history.

## 11. Impact and implementation considerations

The user's normal operation is "SDL with directive → init → necessary Luau/seed editing → serve".
Although it is not a product that automatically implements all GraphQL with just CRUD settings, schema/model changes are no longer Rust changes.
In addition to type declaration generation, the main implementation burden is owned schema, null/number conversion, and final consistency of relationships.
Security is placed on Rust side verification and existing executors, and all gates can be reproduced without AI.

The survey baseline is `545aad9b767ade4f1cfa6a3d0213abedab5a741c` (clean). Not subject to reset.
`crates/core/src/schema/` has an owned synthetic schema, and `crates/core/src/executor/value.rs` has an erased dispatch.
`crates/pythia/src/schema.rs` and `crates/facade/src/input.rs` currently mainly contain generated descriptor input.
Move the fixed contracts of `crates/mock/src/model.rs`, `arguments.rs`, `script/`, and `resolvers/` to the common IR.
These deficiencies are subject to permitted implementation and do not leave "no current API" alone as a design decision.

Remove fixed `generated/mock-blog/` from runtime dependency. As a normal ABI 6 product if left as a comparison fixture
Maintain regular registry/exact/quality and enable differential verification with the dynamic route of the same blog.
Fixed model/root/Luau type declarations that are no longer used and old mock config checks have been replaced with new generic checks.
Do not delete safety assertions or other product generation paths together. Inherit the deleted/relocated inventory.
