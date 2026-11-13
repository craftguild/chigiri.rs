# ADR-0038: Simplify Mock directive/model operations and align constraint vocabulary

- Status: Accepted
- Implementation: Implemented. Local integration acceptance with ADR-0039 completed, and the final designer implementation review passed on 2026-09-14.
- Date: 2026-09-10
- Decision clarification: On 2026-09-10, the designer approved the per-field relation semantics and the point at which runtime IDs are completed.
- Design approval: On 2026-09-10, the designer approved the main text and all recommendations in the seven-group SD-0 plan. Section 10 records the finalized decisions.
- Decision owners: Pythia maintainers
- Amended by: [ADR-0039](0039-request-atomic-mock-execution-and-soft-hard-limits.md) (2026-09-13, Accepted and Implemented), which defines request atomicity, Soft/Hard limits, failure responses, and subsequent format versions.
- Depends on: ADR-0030~0037 runtime schema, store, project, reference, inference, hook
- Amends: ADR-0030 model specification/required ID/CRUD candidate/value constraint, ADR-0031 ID specification write/paging default value/related operations
- Scope: Lab01 Mock vocabulary, IR, host API, scaffolds, seeds, references, and format migration. Generated ABI 6 remains unchanged.
- Evidence: Small regular expression evaluation and adoption decision
- Implementation goal: Goal for new agents (Gate SD-0 design decision has been approved. Implementation and verification are required separately)

The implementation provides the new vocabulary, shared value-constraint checks, automatic models, arbitrary IDs, explicit relations, seven operations, fifteen constraints, eight formats, and migration of Mock models, APIs, projects, and fixtures. On 2026-09-12, every `cargo test -p pythia_mock` test binary passed: `lib`, `cli_http`, `inference_native`, `module_vm`, `project_vm`, `reference_cli`, `runtime_vm`, and `vm_spike` (validation record, Section 31).
At that point, common validation and acceptance of the final source remained separate. The observation-budget reservation proposal documented the conflict between `after` hooks and the shared sticky budget, together with an implementation and reproduction tests.
The 3/4 and 1/4 allocations were presented pending additional design approval and were not included in the existing Section 10 approval.
Do not alter past ADR, goals, passed, or verification records to use them as proof of passing for a new contract.
Changes in this document will be applied in subsequent independent implementation tasks and will not authorize changes to external applications.

The subsequent execution-contract work is defined by the ADR-0039 implementation goal.
The 3:1 observation reservation plan was not adopted. Keep the existing model/API etc. and migrate to the new execution contract without repeating the wait for additional approval.
Acceptance of this ADR text and the earlier 101-command result are historical evidence for the old candidate; they do not establish ADR-0039 acceptance or final designer approval.
Local acceptance of the current implementation was completed by reconfirming ADR-0039's 102-command common validation, measurements, and independent reviews A and B.
See 0039 Validation Record for the final trail of model/API/constraint maintenance and modified execution contracts.

## 1. Background and policy

The previous design required users to learn seven Mock directives: model, relation, inverse, CRUD, resolver, value, and scalar.
Seeds are not generated for unmarked objects, which differs from the initial operation expected by users with SDL.
Since an ID is required for the model, it is easy to request unnecessary identifiers for value objects such as PageInfo, Viewer, and Payload.
Building a small Mock does not require learning a complete database design or a
dedicated CRUD mapping language.

The policy is to handle objects automatically, make only relations and computed fields explicit, and compose basic Luau operations.
GraphQL and Prisma remain distinct specifications. The design uses standard SDL custom-directive syntax, adopts Prisma's readable relation notation, and aligns its constraint vocabulary with `graphql-constraint-directive`.
General referential actions, a general-purpose ORM, a JavaScript-compatible VM, and a new regular-expression engine are out of scope.

## 2. Compatibility with old and new Directives

| Current | Subsequent policy |
| --- | --- |
| `@pythia_mock_model` | Removed. Concrete objects other than operation roots and similar special types become models automatically. |
| `@pythia_mock_relation` / `@pythia_mock_inverse` | Replaced by `@relation`. Relation fields identify their counterpart through the combined declarations. |
| `@pythia_mock_crud` | Deprecated. Removed automatic mapping of GraphQL arguments and host CRUD |
| `@pythia_mock_resolver` | Change to `@virtual` which represents a calculation field |
| `@pythia_mock_value` | Change to `@constraint` |
| `@pythia_mock_scalar` | Maintained. Explicit expression of custom scalar |

Old name aliases and compatible readers are not retained. Rather than silently ignoring obsolete directives, reject them by indicating where they should be migrated to.
`@relation`/`@virtual` are specific to Pythia and are not part of the GraphQL standard or Prisma SDL itself.
`@constraint` is also not a built-in directive of the GraphQL standard, but rather adopts the vocabulary of the reference implementation.
The shared SDL entry path performs bounded parsing, matches same-name directive definitions, and rejects unknown arguments and locations.

## 3. Automatic modeling, seeds, virtual

### 3.1 Concrete objects and saved values

Generate a model API and empty JSONL seed for each user-defined concrete object.
Exclude operation roots, introspection types, and vocabulary-support types, including the schema's Query and Mutation roots.
Do not create a storage model or seed for the interface/union itself, but maintain the existing concrete type dispatch.
Generating an empty seed does not synthesize random data or produce a ready resolver.

Not every object requires an ID. Objects such as PageInfo and Payload may be returned as ordinary values without IDs and stored as embedded values when needed.
Just because the type points to another object does not change it to a storage relation.
Normal nested objects/lists are treated as finite embedded values, and no exceptions are made for cross-reference lists.
Fields treated as relations require `@relation` for each field including the opposite side.
Simply adding a directive to another field will not reclassify the unmarked field to inverse or M:N, and will maintain the meaning of the embedded value.
Even if there is a cycle in the type graph, the seed/response JSON will not be expanded infinitely.

| field declaration | meaning |
| --- | --- |
| regular stored field | scalar or stored-value |
| `@relation` | Relation that refers to identity |
| `@virtual` | Value calculated by Luau (virtual) |

IR, seeding, type generation, and references share this semantic distinction.
Query/Mutation and resolver scaffolding for fields with arguments follow Section 3.2 and should not be confused with saved fields.

Even if the contents of IDless records are the same, they can be registered as separate records.
The internal registration order/row handle is an implementation detail and does not add a hidden ID field to the user's record.
No fixed Rust branch infers CRUD business meaning from model names, field names,
or aliases.

### 3.2 Scaffold and virtual

For Query/Mutation and fields with arguments, prepare a Luau scaffold with expected types without directives.
A calculation field without arguments is specified with `@virtual` and a corresponding Luau file is generated.
The abolition of the CRUD directive does not mean the abolition of host CRUD. The user/AI who fills in the scaffold combines seven operations.

- A virtual field is not a storage slot and cannot be written by seed, create, or update. Report that specific violation rather than treating it as merely unknown.
- Do not use virtual for where/orderBy of the save field. Rejects simultaneous specification with relation.
- Missing required modules, unimplemented states, and type mismatches will be rejected before startup using check/serve/export as usual.
- Also inspect the output of implemented resolvers with SDL/null propagation/constraint. Luau's type checking alone does not guarantee it.
- The authority of root/nested/hook is determined by the actual invocation scope, and write is not allowed since there is only a scaffold.

### 3.3 Seed Inspection

JSONL has one record per line, and empty files are also legal.
Missing required storage slots, mismatched types, unknown fields, duplicate JSON keys, and broken references will be rejected before listening.
Missing nullable storage slots are treated as null and not confused with empty arrays/objects.
Seeds are not required to contain virtual, inverse-relation, or computed fields.
Inspect storage slots required by IR such as hidden FK. Check the consistency of relationships after all seeds have been imported.
Read protection based on value/depth/node/file/total byte and descriptor is not relaxed.

## 4. Model API and selection rules

| Method | Return value |
| --- | --- |
| `find(options)` | Copy array of matching records |
| `count(options)` | Total number matching where |
| `create(input)` | Copy of the created record |
| `update(options, patch)` | Updated copy array of selected record |
| `delete(options)` | Number of items to be deleted |
| `connect(sourceOptions, relation, targetOptions)` | Post-operation copy array of the selected connection source |
| `disconnect(sourceOptions, relation, targetOptions)` | Same as above |

Colon calls, explicit tables, closed argument checks, Query/nested/hooks are readonly, and only the mutation root is writeable.
The reference and Luau types are generated from the same typed API registration and IR, and the description is not handwritten as a separate specification.

### 4.1 Select targets using the same QueryPlan

Both find/update/delete and connect/disconnect use the same where/orderBy/offset/limit and default values.
Retain ADR-0031's distinctions between leaf/all/any, scalar operators, null, numerical precision, storage constraints, and search operands.
The order is `where` → `orderBy` → `offset` → `limit`. Without `orderBy`, equal values retain registration order; omitted `offset` is zero.
When `limit` is omitted, return every match after the offset rather than retaining the old default of 20. An explicit limit of 0 returns an empty array.
Reject negative numbers/decimal numbers/unknown keys/invalid fields/over budget, and do not clamp or implicitly terminate at 100 items.
count receives only where and returns the total number of cases independent of paging.

Selection is made once in the candidate state before editing. Even if there are duplicate records with the same content, all records that match the conditions will be considered.
Do not reselect the target when changing the field with patch. update is outermost field replacement and does not add deep merge.
Always validate arguments, patches, permissions, and relation names, even when no records match. Updating zero targets returns an empty array, and deleting zero targets returns 0.
Connecting or disconnecting with zero source records returns an empty array. With zero destination records, edges remain unchanged and the selected source records are returned as copies. Type, authority, and budget checks still apply to these no-op cases.
Resource constraints that cannot handle full selection result in an error and do not return partial results or partial success.
Distinguish between the result array and the number budget to be manipulated, and explicitly test the value budget change due to the removal of the old 100-item rule.

### 4.2 ID and Key

create accepts explicit IDs and rejects conflicts with existing IDs. Do not arbitrarily change the case of the passed string.
For an eligible ID field, runtime `create` generates a lowercase ULID when the input omits the ID.
Runtime ID completion does not add an ID field to types that lack one and does not fill missing IDs in seeds.
Automatic ID completion is a runtime write convenience and does not provide fixture reproducibility.
For reproducible seeds/fixtures/golden data, the user specifies the ID. Runtime create allows omitting eligible ids.
Generation processes such as init/update/export do not add new IDs derived from time or random numbers, separating definitive artifacts from the convenience of runtime.
Automatic numbering target type, conformance with constraints, and explicit null follow Section 10 and approved SD-0 Section 5.
The key components registered as IDs and references remain unchanged, and changes by update are rejected.
Foreign keys are not referenced keys themselves, so they can be modified with existing type/relationship checking.

Changes to the failed root record, relationship index, registration order, and numbering management will be discarded.
Do not treat the ULID as a secret/authentication token and do not promise that the same random value will be regenerated after rollback.

## 5. Relations

### 5.1 Declarations, references, and inference

`@relation(fields: [...], references: [...], name: "...")` is the Prisma-like relation entry point.
The existence of a relationship is clearly indicated using a directive, and a business relationship is not created based on the type alone.
When publishing inverse, add `@relation` to the field. A separate directive for inverse is not required.
fields corresponds to the saved FK of the connection source, and references corresponds to the keys of the connection destination in the same order.
Support single and composite keys, checking component count, component types, and uniqueness of the complete referenced key.
Derive cardinality from singular or list shape and nullability. A one-to-one relation also requires uniqueness of the foreign-key group.
Only fields with `@relation` participate in relation pairing; unmarked fields
are not candidates. If type, cardinality, and the foreign-key declaration
identify a unique matching relation, `name` may be omitted.
Multiple candidates such as author/recipient and ambiguous self-references should be identified by name, and should not be decided arbitrarily based on field name/generation order.
Conflicts in explicit names and unresolved/ambiguous declarations that require a corresponding target are rejected before startup.

`fields` and `references` declare the owner and foreign key. When omitted, counterpart resolution determines whether the field is an inverse or a managed many-to-many side. Do not interpret every argument-free relation as implicit storage.

There is no need to expose FK to GraphQL.
```graphql
type User {
  id: ID!
  posts: [Post!]! @relation
}
type Post {
  id: ID!
  title: String!
  author: User! @relation(fields: ["authorId"], references: ["id"])
}
```
`authorId` may exist in seeds, store records, and Luau without being added automatically to the GraphQL type.
The `User.posts` field above is resolved as the inverse through the pairing of
the declared relations.
Removing `@relation` from `User.posts` makes it an embedded value; declaring only `Post.author` does not convert it into a dereference.
If authorId is also declared in SDL, it will share the same saved value and will not have two values. Reject type mismatches.
GraphQL output follows selection and does not copy the entire storage-only slot to response.

The value of a compound reference is not a positional array, but an object with the field name of the reference destination.
For example, if references are tenantId/userId, it is `{"tenantId":"t1","userId":"u1"}`, and the key order of object does not change the meaning.
Reject missing/redundant components/type mismatches. A single-key seed reference maintains a concise representation of the key scalar.
The corresponding field set is determined by SDL/IR, and the key definition is not overwritten on the seed side.

### 5.2 Implicit / Explicit M:N

It targets both Implicit M:N, in which the intermediate model is omitted, and Explicit M:N, in which the user writes the intermediate type.
Implicit refers to the management of intermediate storage, and does not mean that it is a relation.
Implicit storage is selected when the `@relation` lists on the two identifiable
models pair uniquely.
Do not complete an unmarked cross-reference list from a one-sided `@relation`. If both sides are unmarked, both are embedded values.
Multiple relation-pairing candidates require a name; generation order never
resolves the ambiguity. A unique pairing may omit the name. The example below
writes the same name on both sides for clarity.
If additional attributes or complex keys for intermediate records are required, express them using an intermediate type of Explicit and two relations.
The range of identifiers used in Implicit is specified in Section 10, and full compatibility with Prisma is not implicitly promised.

Implicit seed does not require a separate link JSONL, but instead writes an array of reference keys to the relation field.
```graphql
type User {
  id: ID!
  posts: [Post!]! @relation(name: "Membership")
}
type Post {
  id: ID!
  title: String!
  authors: [User!]! @relation(name: "Membership")
}
```
User's seed is `{"id":"user-1"}`, Post's seed is
`{"id":"post-1","title":"Example","authors":["user-1"]}` is sufficient.
Only the seed's membership set may be declared on one side here; the SDL
relation itself must still be declared on both sides.
Derive the opposite membership set of a resolved relation from both declarations. GraphQL returns selected objects, not arrays of IDs.

- Omitting the relation field means ``do not declare a complete membership set from this side.''
- If an array is specified, it is the record's complete membership set. `[]`
  declares an empty set.
- If both sides declare conflicting sets, fail before startup rather than silently taking their union.
- Does not depend on file/record order, reads all seeds and then resolves.
- Reject duplicate references, non-existent targets, required relationships and uniqueness inconsistencies.

### 5.3 Relationship editing and deletion

Specify the connect/disconnect relation using the GraphQL relationship field name, and do not specify the hidden FK name.
Select the connection source/destination using a common QueryPlan, and add/remove edges for that combination.
Determine the source/target before editing so that the target set does not change midway through updates.
Connecting existing edges and disconnecting absent edges is no-op. On the other hand, implicit reassignment of singular relationships is not performed.
Reject the entire operation if it violates non-null, one-to-one uniqueness, or cardinality constraints.
The order of existing edges is maintained, and the order of addition is fixed to the order of selection. Implicit M:N also uses the same single relation state/index.

Keys are immutable and deletion uses RESTRICT. A target referenced by a surviving record cannot be deleted until the reference is explicitly changed or removed. Do not add cascading updates or deletes.
Edges/self-edges that are only inside the deletion set can be removed at the same time as the record.
Only Implicit M:N may remove the internal link connected to the deleted record, but the other record will not be deleted.
The Explicit intermediate model is a normal storage record and is explicitly manipulated by the user.
Required/cardinality constraints on the surviving side are checked even after link cleanup.
All of these behaviors integrate with the root transaction, snapshot, sticky
failure, and request-wide cancellation and commit fence; they do not create a
second store.

## 6. Constraints

### 6.1 Vocabulary to be adopted

All of the following 15 arguments of the reference implementation are targeted.

| Target | Argument |
| --- | --- |
| String | minLength / maxLength / startsWith / endsWith / contains / notContains / pattern / format |
| Number | min / max / exclusiveMin / exclusiveMax / multipleOf |
| List | minItems / maxItems |

String length and list size are Int, number bounds and multipleOf are Float, and others match the SDL input type of String.
Applies to FIELD_DEFINITION / INPUT_FIELD_DEFINITION / ARGUMENT_DEFINITION.
format is a String in SDL, but internally in Rust it resolves to a closed enum that only allows the following 8 values:
`byte` / `date-time` / `date` / `email` / `ipv4` / `ipv6` / `uri` / `uuid`.

Unknown arguments/formats, invalid types, inapplicable combinations, and obvious contradictions will be treated as pre-start errors and will not be ignored with a warning.
Non-nulls are owned by SDL's `!` and do not convert nulls to empty strings/0/empty arrays to satisfy constraints.
Use the same test definition for seeds, create and update operations, and GraphQL input, default, and return values.
Strict application rules such as element constraints on lists follow Section 10 and approved SD-0 Section 4, and the same vocabulary is not interpreted differently from one entry to another.

It uses the documented meaning and does not reproduce defects in reference implementations or truthfulness in JS.
For example, maxLength: 0 is not "no constraint" and only allows empty strings.
`errorMessage` and `uniqueTypeName` are out of scope. Reject them explicitly and document the incompatibility in the README and reference output.
If locale-sensitive behavior becomes necessary, it requires a separate ADR.
The schema wrapper does not gain scalar-renaming functionality.
Do not add custom formats, network queries, arbitrary validation callbacks, or JS execution infrastructure.
Retain explicit representation of `@pythia_mock_scalar` and existing basic representation checking.

### 6.2 Deciding to use regular expressions

The pattern engine is `regex` crate, and the evaluated 1.13.1 is used as the adoption standard.
`fancy-regex`/`regress`, proprietary engine, fork for JS compatibility, and exclusive child process isolation are not adopted this time.
There is no fallback to a different engine for each pattern, and no automatic rewriting of unsupported syntax.
Dependencies are closed to Mock usage and do not extend the dependency boundaries of the regular core/runtime/generated SDK.

With `fancy-regex` 0.19.1's count limit of 10,000, a 4 KiB input and a short nested lookahead takes about 766 ms,
Failed due to internal stack limit. Exceeding the existing 500 ms VM limit, responsiveness cannot be managed only by limiting the number of times.
Both engines were fast for ordinary alphanumeric input, so average speed alone
does not determine the choice.
For detailed conditions, success/failure, and measurement limits, refer to Evaluation record.

| Control | Initial value/meaning |
| --- | --- |
| pattern source | 1 KiB or less in UTF-8 |
| Target string | 4 KiB or less in UTF-8. Excess is rejected without being cut down |
| compiled size | `RegexBuilder::size_limit(4 * 1024 * 1024)` |
| DFA cache | `dfa_size_limit(1024 * 1024)` |
| syntax nesting | `nest_limit(64)` |
| Invocation | Single `is_match`. Do not use match-all search iterator or capture replacement |

Compile once at startup and share the same pattern. The number/total source/built/held resources will also be included in the existing shared budget.
Do not allow local limits to create unlimited caches for the entire schema.
Inspect normal cancellations/deadlines before and after evaluation, and do not reset the work budget for the same request/root.
The compiled/cache value is not the RSS upper limit for the entire process, and a single measurement does not guarantee time for all inputs.
Wrapping synchronous evaluation in a timeout is not claimed to make it
interruptible.
Failures caused by excessive pattern expansion, input size, or call count remain sticky, and partial mutations do not commit. Use the crate's case-sensitive, Unicode-enabled defaults and accept only flags expressed through the crate's pattern syntax.
Do not interpret the JS literal `/pattern/flags` or add another flags argument.
No implicit full-string match is added; users write anchors when they need a
full-string match.
Look-ahead/look-behind/back-references are shown as compilation errors before startup, along with the schema location.
Clarify the differences with JS, especially Unicode's `\d`/`\w`, character units, and escape/flag in the README and generated reference.
What we are aligning here is the constraint vocabulary, not complete compatibility with ECMAScript RegExp.

## 7. Format and non-destructive migration

The version of this section is the contract for candidate ADR-0038. Subsequent versions will be synchronized to the ADR-0039 Section 9 version, keeping old user files non-destructive.

Backwards compatibility of old schema/host API/save format will not be maintained as it is not yet released.
Add versions to new vocabulary and APIs and add them to project/bundle/reference/proposal/update-state/schema-snapshot.
At the same time, contracts that have different meanings are switched. The specific version table follows Section 10 and approved SD-0 Section 7.
Don't update the GraphQL generated ABI 6 and generator manifest format just with this mock change.

Do not add migration-only commands/automatic merge/old format readers.

1. Keep the old project and recovery materials as is, and make SDL changes in a separate working copy.
2. Init to another empty directory using the new CLI. Do not overwrite the old directory as part of the migration procedure.
3. User selects and transfers resolvers/hooks/seeds and modifies obsolete directives, write arguments, relationships and constraints.
4. Check the new project, perform the necessary HTTP checks, run the independent launch after exporting, and then switch the destination.

Do not fake compatibility by copying the old proposal or old update state to the new project.
Daily updates in the new format maintain existing B/U/N, edited U, .dist, simultaneous editing rejection, and rollback for all targets.
The formatter is applied to new candidates and does not format old user original texts/comparison standards/recovery materials without permission.
Infer's input reference is also aligned to the same API/vocabulary version, and actual AI communication is not a necessary condition for this normal verification.

## 8. Boundaries to maintain and implementation order

Subsequently, follow ADR-0039 only for root transactions, partial successes, and after failures. Don't redesign other models/permissions/GraphQL boundaries.

Regular product MSRV 1.85, Mock MSRV 1.95, ABI 6, Lua least privilege/pcall restriction/cancellation/recall,
Mutation root transaction and readonly hook, maintaining GraphQL's selection/null/error semantics.
Preserve the 400-line handwritten-code limit, the English prose rule from ADR-0043, semantic partitioning of generated output, and generated-file immutability.
Do not add signature/distribution/SSH/publisher gate, persistent DB, Subscription, or UI redesign.

Implementation order is: finalize remaining specifications → vocabulary and IR → store, shared selection, and relations → constraints → Luau types, scaffolds, and reference → project and format migration → real HTTP, VM, and overall verification.
Run with small focused tests, and consolidate time-consuming common validations into the final stage.
Port the existing safety assertions to the new API. Do not delete old fixtures merely to adjust the reported pass count.

## 9. Acceptance items for subsequent implementations

Every item starts as Required / Not run. Adding this ADR or evaluating one regular expression does not make any item Passed.

| ID | Demonstration |
| --- | --- |
| SD-01 | Generate empty seed/API for unmarked object and exclude root/introspection/auxiliary type |
| SD-02 | Payload without ID/Embedded object/Multiple records with the same content as cross-reference list, meaning of unmarked field does not change due to declaration of other fields |
| SD-03 | Virtual is prohibited from being saved, required modules/unimplemented are rejected before startup, output constraints are also checked at runtime |
| SD-04 | Check required saved values/unknown fields/referential integrity of all seeds before listening |
| SD-05 | Explicit ID/seed reproducibility, missing seed ID non-completion, runtime limitation of lowercase ULID, create without ID, state discard in case of collision/failure |
| SD-06 | Generate types, arguments, permissions, and references for seven operations from the same definition |
| SD-07 | Cover an omitted limit, more than 100 matches, zero matches, explicit paging, and target selection before editing. |
| SD-08 | All update/delete, duplicate records, key unchanged, all model rollback for subsequent failure/cancellation |
| SD-09 | Explicit owner/inverse field, unique relation pairing/name/inconsistency, single/composite key, hidden/SDL public FK, uniqueness and self-reference |
| SD-10 | Implicit/Explicit M:N with both sides explicit, unmarked declaration not completed with one side declaration, one side seed/both sides inconsistent/order independent/out of reference |
| SD-11 | Cleanup with only conditional connect/disconnect, cardinality, no-op, RESTRICT and link |
| SD-12 | Total 15 constraints, 8 format positive/negative, 0/negative number/boundary/null/contradiction |
| SD-13 | Unicode/numeric precision, same constraints applied to seed/write/GraphQL input/output |
| SD-14 | Unsupported regex syntax/overexpansion/nesting/combined budget, Mutation rollback and return to the same server |
| SD-15 | Explicit rejection of old and new formats, migration to new directory, non-destruction of old U/.dist/backup/proposal |
| SD-16 | Two domains/rename/offline project/export, real HTTP in the same binary |
| SD-17 | 1.85/1.95/stable, ABI 6, all SDK exact/quality, VM recovery, common validate |
| SD-18 | Record individual assertions/measurements/unexecuted environments/untracked results and undergo design review |

## 10. Design decisions finalized in SD-0

On 2026-09-10, the decision owner requested design approval for Lab01's latest ADR. The designer reviewed the main text and all seven groups in the SD-0 decision table, then approved the recommendations together.
Sections 1 through 8 of the decision table are normative details for this ADR, including boundary examples and formal and budget tables.
Section 9 records research context and is not evidence that product tests passed.

| Group | Confirmed decision (details in SD-0) |
| --- | --- |
| String | Unicode scalar value unit. Combined characters, VS, and ZWJ are also single characters each, without normalization. UTF-8 byte budget maintained separately |
| Numeric | Exact comparison of finite numbers. multipleOf uses a positive/finite divisor and a canonical decimal value to determine the remainder as zero, without epsilon. Preserve integer precision and scratch limits |
| format | Fixed the acceptance range of eight formats with specific examples and validator candidates in Section 3. No implicit correction/external inquiry |
| Constraint application | Count condition is the outermost list, scalar condition is recursive non-null leaf. AND composition with scalar declaration without propagating to the children of object |
| Key | Save ID/String/Int/Boolean/enum non-null values and their pairs. ULID completion is only saved id:ID!/String! which is omitted in runtime create. A nullable composite FK is either all null or all components |
| relation | Implicit M:N is both sides explicit list and non-null save id. Can be edited from both owner/inverse, target 0 is no-op which returns source copy. No complement/implicit replacement of unmarked fields |
| Format and Budget | Simultaneous change to Section 7's seven formats and vocabulary v2. Adopts a total list limit of 1,000, shared Work/value budget, and Section 8 regex combined reservation/execution ledger |

Adopting a dependency candidate does not mean that dependency closure, MSRV, license, and actual build inspections have been completed.
The regex reservation ledger does not guarantee strict interruption of process RSS or synchronization processing. Maintain recorded limits.
If it becomes necessary to change the scope of acceptance, format, or budget during implementation, present the rationale and proposed changes and obtain additional approval.

Gate SD-0 design approval is complete. Compare the approved decisions with the measured baseline and proceed to SD-A without requesting approval again for the same seven groups.
Separately record the design approval and subsequent implementation acceptance in the validation record.

## 11. Implementation acceptance and termination trail

The following history records verification of the ADR-0038 candidate. ADR-0039's design approval resolved the later approval dependency, but did not adopt the proposed 3:1 observation-budget split. New execution contracts use separate RA IDs and validation records.

The candidate implemented concrete objects, arbitrary IDs, embedded values, explicit relations, virtual fields, seven shared QueryPlan operations, fifteen constraints, eight formats, shared regex resources, the new seven-format migration, project edit protection, references, and fake inference.
Individual assertions, corrected failures, independent reviews, and execution results for SD-01 through SD-18 are recorded in validation record Section 32.

Section 31 of the same record retains the Claude-run crate-test report as handoff history.
The latest full run, `run-96nc4lb5`, reached its 1,800-second command deadline during an inference test after the Mock library and CLI HTTP suites had passed.
That timed-out run did not complete and left no active process. The shared plan divides all test binaries into finite groups under Rust 1.95 and stable Rust.
Inventory library, binary, example, benchmark, and integration targets against Cargo metadata so omissions fail validation.
Preserve product MSRV 1.85, generated ABI 6, all-SDK exactness and quality, and existing cancellation and worker-collection checks.

Additional independent review A found missing filter validation and readiness issues. Both were corrected, and the read-only re-review found no remaining issue.
Review B confirmed a misclassification in the shared observation frame; the fix stopped further after-hook diagnostics once that condition was reached.
The corrected behavior preserves outer after-hook exceptions, diagnostics for initial conversion failures, and before-hook processing. This implementation change is separate from the decision not to adopt the proposed allocation.

Local release measurement can be performed on the same binary and does not require remote CI or deployment.
The designer's final implementation review passed on 2026-09-14 with no findings at any severity.
Linux/macOS remote CI, human UI/IME confirmation, and real AI connection are classified as unconfirmed.
As per Implementation Goal Section 11, all required local verification, review corrections, usage documentation and additional design approvals are completed.
The earlier handoff state was "Local implementation acceptance completed / Waiting for designer approval"; the designer subsequently Passed the final implementation review on 2026-09-14.
Only an exit ticket verified against the final source may establish Passed status for Gates SD-A through SD-E.

## 12. Reference

- [graphql-constraint-directive vocabulary](https://raw.githubusercontent.com/confuser/graphql-constraint-directive/master/lib/type-defs.js)
- [graphql-constraint-directive README](https://github.com/confuser/graphql-constraint-directive)
- [Prisma v7 relations](https://www.prisma.io/docs/orm/v7/prisma-schema/data-model/relations)
- [M:N in Prisma v7](https://www.prisma.io/docs/orm/v7/prisma-schema/data-model/relations/many-to-many-relations)
- [ULID specification](https://github.com/ulid/spec)
- [regex 1.13.1](https://docs.rs/regex/1.13.1/regex/)

It is not a contract to follow the referenced master/latest unconditionally.
Pythia's contract is the vocabulary, explicit differences, and boundary vectors recorded in this ADR and its supporting record; dependency-version updates require separate review.
