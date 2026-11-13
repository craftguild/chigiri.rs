# ADR-0031: Add Mock model query/relation operations and generated reference output

- Status: Accepted
- Implementation: Implemented
- Local validation: MR-01 through MR-27 and Gates MR-A through MR-E passed. Independent reviews A and B reported no unresolved findings.
- Evidence: Implementation and validation record. Remote Linux and macOS CI remain unconfirmed.
- Date: 2026-09-08
- Decision owners: Pythia maintainers
- Depends on: ADR-0030 runtime schema, Mock IR, generic store, Luau type checking and execution infrastructure
- Related: ADR-0015, 0026~0029
- Scope: `pythia-mock` model host API, type declaration, static candidates, `reference`, verification and usage procedure
- Implementation goal: Goal prompt for new agents
- Activation: Applied in independent implementation task of ADR-0031. Do not add to past goals/completion gates of ADR-0030 that were being implemented at the time of design.

## 1. Background and decisions

ADR-0030 enables one Rust binary to create Mock IR and TOML/Luau from SDL and run GraphQL mocks against volatile stores.
Its basic model API provides `find`, `create`, `update`, and `delete`. To support list interfaces, search, pagination, and relation editing, the next program adds the following capabilities.

1. Find search operators, AND/OR, sorting, offset/limit.
2. count using the same conditional handling.
3. Connect/disconnect owner relation.
4. Generate text for humans and JSON reference for AI from the typed API contract and Mock IR used for implementation.

References are not separate handwritten specifications, but instead share definitions of registrations, types, permissions, default values, and restrictions used by implementations.
Natural language explanations and usage examples should be written only once as metadata accompanying the definition.
Do not automatically infer meaning from arbitrary Rust logic or parse Rust source/rustdoc with your own parser.

This ADR was added as a separate design program and does not alter ADR-0030's interim results, judgments, or validation history.
An implementation must first establish the current ADR-0030 design and implementation as its baseline. If ADR-0030 is incomplete, keep it incomplete rather than using these features to conceal defects or missing gates.

## 2. Compatibility and non-targeting

No backward compatibility layer will be provided for mock APIs before they are released to production.
In this ADR implementation, TOML is `pythia-mock/v2` and host API identifier is `pythia-mock-api/v2`.
The latter is a contract version that is uniquely determined from the format, and is not a setting value that allows the user to select a different authority/API.
Instead of automatically converting and coexisting the old v1/flat where, generate a new TOML at init and explicitly migrate the handwritten Luau/seed.
ADR-0030 does not require the current implementation of v1 to be changed to v2 before its completion.

Do not change the following.

- Regular GraphQL product and generated SDK ABI 6/MSRV 1.85, Mock-only MSRV 1.95, the adopted `luaur`, and core's zero regular dependencies.
- SDL remains the source of truth for types, models, and relations. The runtime must not treat reference JSON as additional configuration.
- Rust side type/permission/reference/capacity checking, per root transaction, snapshot, cancel/commit, sticky failure.
- No new AI inference, persistence, subscription, authentication, production sandbox.

Do not add `findOne`, `findAll`, `exists`, `upsert`, `save`, bulk writes, aggregation, `groupBy`, cursor pagination, general-purpose joins, conditional scans of relation destinations, regular expressions, or user-provided sort/filter callbacks.
Do not expose commit/rollback to Lua. A dedicated Markdown/HTML generator for humans and a built-in pager are also not required.
Publisher work, SSH acquisition, package release, and publication are outside the product validation gate.

## 3. Model API and common calling contract

The canonical call form is `ctx.models["Post"]:method(...)`.
Rust also checks the model name, argument count, receiver, the VM that owns each
value, its type, and its permissions.

| Method | Return value | Query / nested | Mutation root |
| --- | --- | --- | --- |
| `find(options)` | Record array | Allowed | Allowed |
| `count(options)` | Non-negative integer | Allowed | Allowed |
| `create(input)` | Copy of the created record | Forbidden | Allowed |
| `update(id, patch)` | Copy of the updated record | Forbidden | Allowed |
| `delete(id)` | True when deleted; false when absent | Forbidden | Allowed |
| `connect(id, relation, targetId)` | Copy of the owner record after the operation | Forbidden | Allowed only for models with an owner relation |
| `disconnect(id, relation, targetId)` | Copy of the owner record after the operation | Forbidden | Allowed only for models with an owner relation |

`options`, `input`, and `patch` must be explicit tables. Do not add argument-omitting aliases such as `find()`.
In Lua, you can express single acquisition by selecting the head from the array of `find({ where = ..., limit = 1 })`.
Return `ctx.null` for nullable GraphQL results and do not make nil an implicit GraphQL null.

Save, numbering, outermost patch, and RESTRICT of create/update/delete inherit ADR-0030.
The same snapshot is used for read-only, and the same candidate state is used for mutation root.
A read containing count also shares the work/value budget and failure status of the same call.

## 4. Search conditions

### 4.1 One closed predicate format

`find` options only accept `where`, `orderBy`, `offset`, and `limit`.
Where is omitted /nil/`ctx.null`, there is no condition. When specified, use one of the following.
```lua
-- One condition
{ field = "title", op = "contains", value = "Rust" }

-- AND; each child predicate is a leaf, all, or any node
{ all = {
    { field = "authorId", op = "eq", value = args.authorId },
    { field = "title", op = "contains", value = args.keyword },
} }

-- OR
{ any = {
    { field = "status", op = "eq", value = "DRAFT" },
    { field = "status", op = "eq", value = "PUBLISHED" },
} }

-- Test for null explicitly
{ field = "summary", op = "isNull", value = true }
```
Leaf requires field/op/value. For all/any node, only one key is required and mixed/unknown keys are rejected.
Empty all is true, empty any is false. Empty where table is invalid, and no condition omits where itself.
Children of all/any are contiguous arrays. Reject holes, circular tables, and excessive depth/count.
Since no flat map is left, field names such as `all`, `any`, and `op` do not conflict with the syntax.
field is the exact storage slot name of the model. Dot path, case conversion, and inverse/calculated field searches are not performed.

### 4.2 Operators and types

| op | target | value |
| --- | --- | --- |
| `eq` / `ne` | simple scalar/enum/ID slot | non-null value with the same representation |
| `in` | Same as above | An array of non-null values with the same representation. False if empty, duplicates do not change the result |
| `lt` / `lte` / `gt` / `gte` | Int/Float, INTEGER/NUMBER custom scalar | Finite values with the same numerical representation |
| `contains` | String, STRING custom scalar | non-null string |
| `isNull` | simple slot above | bool. true searches for null, false searches for non-null |

Do not search list/DTO, OBJECT/ARRAY/JSON custom scalar, and dereferences.
Do not implicitly allow contains or numeric comparisons for IDs and enums. String type keys are treated as Strings.
Size comparisons of bool/enum, locale/case-insensitive comparison of strings, and full-text searches are not initially targeted.
contains is a case-sensitive continuous partial match of UTF-8 strings without normalization or regex interpretation.
The empty string contains matches any non-null string. Enums only accept known exact tokens.

Omission of a nullable storage slot and explicit null are treated as the same null for search purposes.
For a null stored value, every leaf except `isNull` evaluates to false; `ne` does not match null.
Do not pass nil/`ctx.null` to the leaf value to make it "no condition". Use isNull for null search.
Validate every predicate before scanning, and reject invalid fields, operators, or values even with zero rows, `limit = 0`, or a short-circuited OR branch.

Comparison operands check type/representation/integer precision/finiteness and search input budget.
Storage constraints such as `min`, `max`, and `minBytes` do not automatically apply to query operands.
For example, you can specify 2 characters for a title with a minimum of 10 characters, and `gt -1' for a non-negative field.
This does not relax storage constraints; search operands and stored values are validated for different purposes.
The existing coercion will continue to check the scalar/input constraints of the arguments passed from GraphQL.
Do not introduce conversion from strings, integer rounding, or unconditional narrowing to f64 for numeric comparisons.

### 4.3 Evaluation order and static candidates

After type checking, resolve the predicate once to a validated QueryPlan and use the same evaluator in find/count.
Do not re-resolve field names or call Lua callbacks for each record.
All/any can be short-circuited in the order of definition, but checking for invalid input must not be omitted by short-circuiting.

The id/where mapping of the existing FIND directive is converted to all of eq leaf when generating static candidates.
If the mapped argument is omitted/explicitly null, the leaf will not be created and filtered as per the old contract.
This conversion is a directive and is different from the host API's leaf null rejection.
This ADR does not increase the number of CRUD directive actions or filter languages.
GraphQL resolver for advanced search, sorting, count, and relationship editing can be written in manual Luau.

## 5. Sorting, paging, count

### 5.1 find

The order is **where → orderBy → offset → limit**. Do not rearrange only the pages exported to Lua later.
orderBy is an array of `{ field = "title", direction = "asc" }`.
Asc/desc is required for direction, and unknown keys, duplicate fields, and unsupported types are rejected.
When `orderBy` is omitted or empty, results use registration order. Multiple keys take priority from left to right; registration order breaks ties after all keys compare equal.
The registration order is not changed when updating, and new records after deletion use the new registration order.

Only saved simple scalar/enum/ID slots are to be sorted.
Number is in numerical order without loss of precision, string/ID/enum is in UTF-8 bytes lexicographical order, bool is false → true.
Do not use enum declaration order as an implicit priority. Null/omission is the last in both asc/desc.
Null keys also proceed to the next key, and the last key is determined by the registration order. There are no locales or custom comparators.

If omitted, offset is 0, an integer between 0 and 1,000. limit is 20 if omitted, an integer between 0 and 100.
Explicit nulls for these and orderBy are rejected. FIND's mapped nullable limit is a static candidate and normalizes to the default value.
Negative numbers, decimal numbers, and numbers exceeding the upper limit are rejected without being clamped. Offset that exceeds the number of results is an empty array, and limit 0 is also an empty array.
Sorting and scanning may be optimized only if validation and observable semantics remain unchanged.
```lua
return ctx.models["Post"]:find({
    where = { field = "authorId", op = "eq", value = args.authorId },
    orderBy = { { field = "title" :: "title", direction = "asc" :: "asc" } },
    offset = args.offset,
    limit = args.limit,
})
```
Offset pagination supports ordinary UI development, but it does not keep pages
stable when Mutations occur between requests. Snapshot consistency within one
root is not a persistent cursor or a snapshot guarantee across requests.

In Luau 0.1.8, string literals inside optional arrays widen to `string`. Use singleton annotations for `field` and `direction`, as shown above. These annotations affect static typing only; they do not change the table shape or permissions accepted by the host. Generated reference examples must pass both type checking and real host calls.
For an array containing different fields, annotate the complete array, for
example: `local order: {{field: "title" | "id", direction: "asc" | "desc"}}`.
The host still validates every annotated field name against an actual storage
slot.

### 5.2 count

`count({ where = predicate })` returns the **total number of records** that match the same condition.
`where` may be omitted. `orderBy`, `offset`, and `limit` are not accepted and are rejected rather than ignored.
Do not implement count as `#find(...)` or cap it at 100. Zero matches return 0, and result sets larger than 100 are counted accurately.
Count scans stored records without constructing a Lua copy and never returns a partial or estimated value after overflow or budget exhaustion.

Query and nested reads use the same snapshot. A Mutation root reads the
candidate state, including its preceding writes. Share the implementation of
condition semantics, null and numeric handling, and error classification
between `find` and `count`; `count` must not consult a separate live store. Its
result is an exact Lua integer within the configured record limit.

## 6. connect / disconnect

The relation argument is the **owner side GraphQL field name**. Not the save slot name or inverse coordinate.
For example, if the save slot of Post.authors is authorIds, use `Post:connect(id, "authors", userId)`.
Export the owner name derived from schema to Luau literal union and reference, and check the owner/target model in Rust as well.
Do not expose unusable methods to models that do not have an owner relation.

| condition | connect | disconnect |
| --- | --- | --- |
| owner record does not exist | error | error |
| The same edge already exists/does not exist | No change if it is an existing edge | No change if there is no edge |
| target is absent | Error. A target already created in the candidate state is allowed. | The target need not exist, allowing unresolved candidate edges to be removed. |
| Multiple relations | Add target only once to the end | Remove only the specified target, keep other orders and null items |
| nullable singular, currently null | set to target | no change |
| Singular, another target already exists | Error. No implicit replacement | No change if different from specified target |
| Remove required singular matching edge | Not applicable | Error due to non-null violation. Replacement is represented by update |

targetId can only be a non-empty valid ID string. There is no separate signature that receives the array /record/null.
target resolves to the current candidate state. If you need to create future references, use create/update in ADR-0030.
If multiple nullable slots are null, connect will make it a one-element array, and disconnect will remain null if there is no change.
Removing the last element leaves an empty array; it does not become null unless the schema permits that transition.

Connecting to the same relationship or disconnecting from an unconnected relationship is idempotent, but does not omit permission, argument, owner, and budget checks.
The return value is always the owner record after the operation, not a bool or target record with or without changes.
No-op does not change the registration order, ID counter, or relationship order.

The implementation shares the same record construction, reference index update, value constraints, and transaction failure handling as the existing update.
Checks for `minItems`, `maxItems`, uniqueness, required inverse relations, and related constraints follow ADR-0030's candidate/final-state contract.
Distinguish between relational constraints that wait for validation and non-null shapes/resource limits that are immediately rejected, and do not commit final inconsistencies.
Do not create a second write path that changes the independent array of other records or inverses.
Additions that exceed schema constraints, writes from Query/nested, operations to inverse, invalid returns, and cancellations fail closed as before.
Even if you catch the exception with pcall, the failure state will not be erased, and the preceding changes in other models will also be rolled back along with the root transaction.

## 7. API contract shared by implementation and reference

### 7.1 Typed internal definitions

Consolidate information currently distributed across the method enum, Lua registration, type-declaration renderer, and argument inspection into typed internal definitions such as `MethodSpec`. The implementation may choose the exact name, but the following shared scope is fixed.

| Definition details | Consumption destination |
| --- | --- |
| stable method ID, Lua name, typed handler support | completeness of method set, runtime registration/dispatch, reference |
| arity, argument/return type, closed option/operator | host bounds checking, Luau type declaration, reference |
| read/write privileges, model/relation conditions | context generation, real call inspection, schema-specific reference |
| Default values, upper limits, applicable types | Execution processing and inspection, reference |
| Description, meaning rule ID, failure classification, usage example ID | human/JSON display, conformance and example test |

Do not define the signature argument independently using string for explanation and match for implementation.
Handler logic normally remains in Rust, with exhaustive correspondence to method IDs.
The type is the internal representation of the fixed API, not the user's plugin/optional expression/arbitrary VM instruction.
Resolve model input/record/search field/owner relation from Mock IR and embody the same definition.
Do not duplicate filter or sort type eligibility in separate runtime and reference conditionals.

Use ordinary Rust structs, enums, constants, and small declarative macros for the initial implementation.
Don't require complex proc macros, source scanning, or huge macros that hide the implementation.
English explanations and valid and invalid examples are written once in metadata. Do not make a handwritten Markdown or JSON method table a second source of truth.

### 7.2 What can be automatically generated and what requires testing

Registrations/types/authorities/numeric constants can be mechanically shared.
The meaning of sort→page, RESTRICT, snapshot/rollback, etc. cannot be automatically proven from registered metadata.
They are tied together with a stable rule ID, description, example execution with expected results, and an independent regression test.
The usage example will be passed to the actual Lua/host, and the test will fail if the explanation becomes outdated.
Correctness is not determined only by matching an output snapshot. For example, state after a count above 100 or after failure is asserted independently. Mutations such as adding a method or option only to the runtime, exposing it only in the reference, granting writes to Query, or changing a default or type must be detected.
Provide a shared contract check that detects mutations such as a missing rule-to-example correspondence.
Natural-language accuracy remains subject to review; matching type metadata does not prove that a sentence describes the intended semantics.

## 8. `pythia-mock reference`

### 8.1 Input and execution range

```sh
# Read the common API.
pythia-mock reference

# Read schema-specific types, models, and capabilities. Multiple schemas are accepted, as with init.
pythia-mock reference --schema models.graphql --schema operations.graphql

# Read the embedded schema from a self-contained TOML file.
pythia-mock reference --config mock.toml

# Produce JSON for AI or another tool.
pythia-mock reference --schema blog.graphql --format json > reference.json

# Save human-readable text. A pipe receives the same format.
pythia-mock reference --schema blog.graphql > reference.txt

# Disable the pager even in a terminal.
pythia-mock reference --schema blog.graphql --no-pager
```
If no input is specified, only the common contract is available. The schemas and config are exclusive, and there is no implicit search/configuration override.
format is `text | json`, default text. help/version follows existing offline principles.
The model/resolver filtering flag is not required in the initial version, and the target can be selected using the stable ID in JSON.

The schema input passes through the same bounded parse/compose/Mock IR validation as init.
config input inspects closed TOML envelope and embedded schema/digest/vocabulary, but
Semantic verification of seed, complete state/syntax/type/compile of resolver, and Luau execution are not performed.
It also needs to be able to be used to fix unimplemented or incorrect Luau.
Reject malformed TOML, old format, unknown key, and schema corruption.
The output from config clearly states "reference does not verify serve readiness."
Does not require rereading the original SDL, listening, creating a store, executing Cargo/rustc/Git, or connecting to AI/API/network.

### 8.2 Human display and pager

2026-09-09 planned amendment: [ADR-0035](0035-cli-tui-and-embedded-graphiql.md) has added the following external PAGER/less method.
Replace with ratatui's built-in viewer. From now on, use the viewer only when both stdin/stdout are TTY, and output the others directly.
Preserve the shared source, JSON, capacity, confidentiality, and non-readiness checks. The table below and MR-22 remain as history of the implemented baseline.
New viewer/terminal restoration/acceptance of non-TTY will be verified separately in DX35-07~12. This does not mean that the current binary has a built-in viewer.

| Conditions | Behavior |
| --- | --- |
| text and stdout are TTY, no no-pager | Pass the text to external pager |
| text, stdout is pipe/file | UTF-8 text without decorations/terminal control directly to stdout |
| json | Pure JSON document, whether TTY or not. pager/no color |
| no-pager | Direct output even in text. Result immutable in json |
| Unable to start pager | If necessary, issue a short notice to stderr and fallback to direct display |

TTY detection must not change the format to JSON implicitly. Text output is reference material with headings, signatures, constraints, and examples; it requires neither a Markdown renderer nor dedicated HTML. Saving the rendered text does not turn it into a normative specification.

The pager uses the normal local environment. If `PAGER` is set, parse it as an executable plus arguments; otherwise, try `less`.
Shell expansion, pipe, redirect, and command substitution are not evaluated. Unsupported PAGER notation is displayed directly as notice.
Do not interpolate schema/config/description to command, and pass the main text from stdin.
Do not restrict `PAGER` to signed or fixed-hash binaries or require special SSH or environment injection.

Do not impose a fixed timeout on reading. Treat a pager that exits normally, including through `q`, as successful.
Abnormal termination of started pager is treated as an environment error, and the main text is displayed directly again to avoid double output.
It also waits/reaps when it terminates early, and when it receives a signal, it uses the existing safe process operation to stop and recover to a bounded state.
BrokenPipe caused by the end of reading the stdout pipe can be aborted normally, but I/O failures such as disk full are not hidden.
Tools that receive JSON use parse to ensure that it is a complete document, not just the exit code.

### 8.3 Structured references

The human-readable and JSON renderers receive the same `ReferenceDocument`.
The JSON format is `pythia-mock-reference/v1`, and schema-specific and non-specific types are distinguished by closed type.
At least structure the following and avoid confining important contracts to free-text explanations.

- `format`, `api_version`, `mock_format`, existing version information of generated binary.
- API method, type, option, and operator definitions and IDs; argument requirements and return values; effects and permissions; defaults and constraints.
- rule ID, error semantic classification, transaction/snapshot/null/number rules, usage examples and execution context.
- When inputting schema, SchemaDigest, model/field/storage slot/type wrapper/constraint/search/sort eligibility.
- Type graph containing input object/default/OneOf and non-model payload. Indicates omitability and nullability separately.
- owner/inverse, target, cardinality, operational relations, read/write API for each model.
- resolver coordinate, expected arguments/return value, root/nested actual call scope and capability.
- Luau permissions global, pcall and sticky failure, ctx.null/jsonObject/jsonArray, non-compliant items.
- `readiness_checked = false`; successful reference generation does not imply that script and seed startup checks passed.

If the same coordinate is used in both Mutation root and nested, the abilities of each scope are recorded as separate entries.
Just because there is a Mutation type in the schema, do not write the write ability to nested or Query.
The type graph is shared by stable ID reference, and circular type expansion and isomorphic copying of each field are not performed.
Preserve exact names as JSON strings rather than arbitrary simple field names, and properly escape them in Luau usage examples.
When no schema is available, the common display must not present the example
User and Post types as if they were the user's actual models. Label examples
unambiguously as examples.

This is a reference to AI and is not fixed to the OpenAI tool schema or the prompt format of a particular model.
Even if an AI rewrites the reference JSON, it cannot change the types,
permissions, or limits enforced by `serve`.
The source of the SDL description is separated as an explanation of user input, and it is not treated as a host contract or an instruction to the AI.
The resolver body, seed record, request value, absolute path, and environment value are not output to reference.
Human display control characters are escaped, and JSON also uses regular escape. Don't assume it's safe if you go through pager.

Fix the order of keys/types/examples and make them byte-identical for the same binary/same input. Do not include time or cwd dependent values.
If existing build information contains a Git revision, use that value. Do not
inspect the Git repository at reference startup or turn an unknown revision
into a falsely clean value.
Distinguish between API contract version and CLI build identity. The details of the structure are fixed to closed using the same type definition during implementation, and round-trip testing is performed.

## 9. Budgets and errors

Retain ADR-0030's limits for the VM, host calls, records, values, work, operations, and shutdown.
Each additional operation counts as one host call; internal scans, sorts, string comparisons, and relationship updates also consume work.
Read-only uses the same overall call budget, and count and no-op do not reset the budget.

| Additional Bounds | Fixed Upper Limit/Treatment |
| --- | --- |
| predicate | root depth 1, depth 8, total nodes 128. Empty all/any also 1 node |
| `in` | 100 candidates per leaf. Duplication is also included in the input budget |
| Compare string | 4 KiB per operand. The field name is handled using the existing IR/input byte limit |
| orderBy | 4 key, duplicate field rejection |
| offset / limit | 0 to 1,000 / 0 to 100. No clamp |
| query work | Share the existing root budget of 1,000,000 work units. Charge comparisons, traversal, sorting, and edge accounting; reject exhaustion. |
| reference | Apply ADR-0030's schema-input limit. Each generated `ReferenceDocument` and rendered output is limited to 16 MiB. |

String comparison/contains also accounts for the implementation's working bounds, including length or actual comparison bytes.
Long-string comparisons are not charged as a single work unit, and the
implementation uses neither hidden case folding nor backtracking regular
expressions.
Sorting uses bounded references or indexes rather than duplicating record bodies. Reserve the upper bound, including string-comparison work, before sorting, or use a fallible path that returns budget exhaustion safely without panicking inside the comparator.
Cancel/deadline also affects Rust's long scan/sort. Partial array, partial count, and partial relation changes are not considered successful.

Reference is constructed using type sharing and incremental byte accounting, and exceeding the upper limit is not detected only after full expansion.
Semantic and rendering failures are detected before text reaches stdout; a
bounded buffer may be used.
Do not silently truncate important specifications when the upper limit is exceeded. The output is not claimed to be atomic until failure after I/O starts.
Flow the rendered bounded text to pager and do not execute external commands derived from schema/script.

Host argument, permission, constraint, and budget errors use the existing redacted GraphQL field errors and sticky-failure behavior.
For `reference`, exit status 0 means success, 1 means a schema, configuration, or validation failure, and 2 means a CLI-usage, environment, or I/O failure. Diagnostics must not contaminate JSON stdout.
Do not confuse error semantic classification of machine references with error codes published to GraphQL.
Do not add public error codes or raw values solely for illustrative output.

## 10. Implementation order and achievement gate

At the time of design, all items were Required/Not run. Do not replace the RSM-ID of ADR-0030 with the additional ID of this program.

| Order | Work package | Results |
| --- | --- | --- |
| 0 | WP-MR0 | 0030 real baseline, distributed registration/type/limits inventory, v2 changes |
| 1 | WP-MR1 | Connection of common MethodSpec/model/authority and four existing operations. Difference inspection before and after sharing |
| 2 | WP-MR2 | predicate/sort/page/count, static FIND candidates, type checking/resource boundaries |
| 3 | WP-MR3 | owner connect/disconnect and common transaction/index update |
| 4 | WP-MR4 | ReferenceDocument, text/JSON, schema/config, TTY/pager/pipe |
| 5 | WP-MR5 | v2 regeneration/migration, real HTTP/VM and reference contract, common validate, independent review |

| ID | Required individual assertion |
| --- | --- |
| MR-01 | The method, arity, option, authority, and default value of runtime registration/handler/type declaration/reference match |
| MR-02 | Reject method missing/duplicate, write to Query, type/upper limit/explanation rule/example missing mutation |
| MR-03 | eq/ne/in/greater-smaller comparison/contains/isNull, type eligibility, the difference between operand and preservation constraints |
| MR-04 | Validate every condition even when `all` or `any` is empty or nested, the limit is 0, no records match, or a branch could short-circuit. |
| MR-05 | Search for null/omission/enum/Unicode/case, large integers/finite numbers without rounding |
| MR-06 | filter→sort→offset→limit, multiple keys, same value/registration order/fixed null, repeat execution match |
| MR-07 | offset/limit/sort default/bounds/type/unknown, out of range rejection, empty result after offset |
| MR-08 | Count over 100, 0, condition equivalent to find, pagination option rejected, partial count prohibited |
| MR-09 | find/count is same snapshot/candidate state, count after write, rollback of all states after subsequent failure |
| MR-10 | 1:1/1:N/M:N/self connect/disconnect, owner/inverse, relationship order, record after operation |
| MR-11 | Duplicate connect/missing edge disconnect no-op and individual contract without owner/target |
| MR-12 | Rejection of single implicit assignment, mandatory null/edge limit/minItems/inverse uniqueness, equivalency with existing update |
| MR-13 | script/host/return/cancel failure after relationship change, all models/order/ID/index restored even after capture |
| MR-14 | Write from Query/nested, statically/dynamically rejecting incorrect receiver/VM/arity/field/target type |
| MR-15 | predicate/string/sort/count/related operations budget, boundary exactly/exceeds, cancel and return to same server |
| MR-16 | Execute metadata usage example on real Luau/host. Typed positive and negative examples, independent result assertion |
| MR-17 | Correctness of common/separate schema/separate config reference, two domain and rename, type graph/ability/related target |
| MR-18 | Reference is possible even in unimplemented/wrong Luau/out-of-reference seeds. script/store/listen/Cargo/network not running |
| MR-19 | malformed/unknown/old config/digest/schema mismatch causes reference failure, existing file not changed |
| MR-20 | JSON closed/deterministic/full parse, types/examples/rule referential integrity, shared graph and output budget |
| MR-21 | TTY/pipe/file/no-pager/JSON, format immutable, stdout/stderr separation, control character escape |
| MR-22 | fake pager with arguments and stdin, spawn failure fallback, q/early termination/signal recovery, abnormal termination/actual I/O failure |
| MR-23 | Distinguish SDL description from contract by not outputting script/seed/raw request/absolute path/environment value to JSON/text |
| MR-24 | v2 init/serve, old flat where rejection, existing FIND null omission mapping, all fixture updates and RSM safety regression |
| MR-25 | Using all seven operations in real GraphQL HTTP and maintaining selection/alias/variables/fragment/abstract/null |
| MR-26 | Cover normal Rust 1.85, Mock Rust 1.95.0, and stable Rust; zero normal dependencies in core; ABI 6; every SDK exact/quality check; terminology; and common validation. |
| MR-27 | Independent Review A/B, all ID traces, measurements, important untracked artifacts, executed/unexecuted CIs and usage procedures |

Gate MR-A requires MR-01, MR-02, and MR-16; MR-B requires MR-03 through MR-09; MR-C requires MR-10 through MR-15; MR-D requires MR-17 through MR-23; and MR-E requires MR-24 through MR-27.
Review A independently checks model semantics/IR/type/transaction, and Review B independently checks reference matching/CLI/resources/secrecy/VM security.
Check the code and execution, correct and re-review all valid points of severity. Do not post Passed from existing reviews.

During implementation, use focused tests and regular cache, and integrate all tests into the existing common validate in the final source.
pcall16/17 stage, capture memory exceeded, debug/release regression of stop/cancel/worker collection are retained.
Preserve the 400-line handwritten-file limit, the English prose rule from ADR-0043, generated-file ownership checks and semantic partitioning, and the prohibition on manual generated-file edits.
Reference also does not require Rust generation/rebuild for each schema.
If the environment-dependent PTY/OS test cannot be executed, specify the exact CI job and the unexecuted range, and do not count it as a success.

## 11. Handover and influence

When adding the design, the implementation of ADR-0031 was made a separate request, and the policy was not to interrupt the 0030 goal of the agent during work.
Subsequent independent implementation tasks added subsequent sections and 27 IDs to architecture/handoff/conformance.
The prior ADR-0030 goals and validation records remain historical evidence. Record current execution examples, test-to-rule mappings, and migration and deletion inventories in `docs/mock-model-reference-validation.md`.

Users can write mocks by combining seven small operations and obtain the required API specifications in the same binary reference.
Although a common contract and schema-specific specific types/abilities can be passed to AI, it does not guarantee the correctness of business intentions or generated scripts.
In the future, AI will be placed on the side that creates ScriptCandidates, and there will be no entry point for reading references and changing host contracts.

The observation HEAD during design is `545aad9b767ade4f1cfa6a3d0213abedab5a741c`, a dirty tree in which ADR-0030 is being implemented.
Referenced `crates/mock/src/runtime/context.rs`, `luau/models.rs`, `store/query.rs`, `store/work.rs` and IR/vocabulary.
This is not an instruction to determine implementation completion, reset to a fixed revision, or delete uncommitted results.
external applications, publication/distribution, and SSH capability are not applicable. Perform stage/commit/push/publish/tag only if there is another explicit instruction.
