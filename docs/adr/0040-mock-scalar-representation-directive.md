# ADR-0040: Standardize Mock custom scalar declarations on `@representation(type: ...)`

- Status: Accepted
- Implementation: Implemented (local acceptance and designer review passed)
- Date: 2026-09-14
- Decision owners: Pythia maintainers
- Approval: The requester approved `@representation`, the required `type`
  argument, preservation of existing semantics, and removal of the old alias.
- Depends on: ADR-0038's scalar and constraint contracts; ADR-0039's atomic
  request execution
- Amends: The Mock scalar directive and argument names in ADR-0030 and ADR-0038
- Preserves: ordinary GraphQL ABI 6 and MSRV 1.85, Mock MSRV 1.95, the model
  API, validation, cancellation, and edit protection

Local acceptance for REP-01 through REP-12 and Gates REP-A through REP-C was
completed on 2026-09-15. The final designer review passed on the same date with
no new findings. Reference information architecture and item-level navigation
are handled separately in the
research and proposal; its CLI, TUI,
and document changes are not part of this ADR's implementation gates.

## 1. Background and decision

Mock's principal directives use purpose-based names: `@relation`, `@virtual`,
and `@constraint`. The custom scalar directive alone used the product-prefixed
name `@pythia_mock_scalar(representation: ...)`. This boundary selects the value
representation used by Mock; it does not implement a scalar's domain rules.

Only these names change:

| Previous contract | New contract |
| --- | --- |
| `@pythia_mock_scalar` | `@representation` |
| Required argument `representation` | Required argument `type` |

All other arguments and validation semantics remain unchanged. The change adds
no implicit conversion or scalar inference. `@representation` is Pythia Mock
vocabulary, not a GraphQL standard directive.

## 2. Declaration and semantics

After implementation, the checked-in SDL remains the authoritative vocabulary
and contains this sole declaration:

```graphql
enum PythiaMockRepresentation {
  STRING BOOLEAN INTEGER NUMBER OBJECT ARRAY JSON
}

directive @representation(
  type: PythiaMockRepresentation!
  min: Float
  max: Float
  minBytes: Int
  maxBytes: Int
  minItems: Int
  maxItems: Int
) on SCALAR
```

The enum name `PythiaMockRepresentation` is unchanged. This decision does not
rename other product-prefixed vocabulary. A textual replacement of the new
argument name must not rename internal Rust fields or value-representation
fields in reference JSON.

| Item | Contract |
| --- | --- |
| Target | Custom scalars only; built-in scalars cannot be redefined. |
| `type` | Required, non-null, and one of the seven enum values above. |
| `min` / `max` | Existing numeric constraints for INTEGER and NUMBER. |
| `minBytes` / `maxBytes` | Existing UTF-8 byte constraints for STRING. |
| `minItems` / `maxItems` | Existing element-count constraints for ARRAY. |
| Missing or inapplicable input | Reject a missing representation, unknown values or arguments, and inapplicable or contradictory constraints. |

The change preserves validation of all seven representations, integer
precision, finite numbers, JSON containers, null and omission semantics, and
capacity limits. Constraints on a scalar declaration and `@constraint` on a
field remain cumulative. `minBytes` is not a character-count `minLength`, and
the location of `@constraint` is not expanded to SCALAR. `@specifiedBy` remains
metadata for a specification URL; it neither selects a representation nor
causes the referenced URL to be validated.

```graphql
scalar DateTime @representation(type: STRING)
scalar NonNegativeInteger @representation(type: INTEGER, min: 0)
scalar Metadata @representation(type: JSON)

type Entry {
  id: ID!
  createdAt: DateTime! @constraint(format: "date-time")
  attempts: NonNegativeInteger!
  metadata: Metadata
}

type Query { entry: Entry }
```

The name `DateTime` and the STRING representation alone do not validate a date
and time format. In this example, `@constraint` on `Entry.createdAt` performs
that validation.

## 3. Vocabulary composition and rejection boundaries

The directive uses the same vocabulary-composition path as
`schema::parse_sources`, so users may omit its definition. If users declare an
equivalent definition, existing equivalence checks compare argument types,
defaults, locations, repeatability, and other schema properties. Conflicting
or duplicate definitions are rejected rather than silently treating a
user-defined `@representation` with different semantics as Mock vocabulary.

Both definitions and uses of the old `@pythia_mock_scalar` name are explicitly
rejected with a concise pointer to the new syntax. Declaring the old name but
not using it is not an acceptance loophole. Likewise,
`@representation(representation: STRING)` is rejected for its unknown argument
and missing required argument. There is no old-name alias, old-argument alias,
fallback, or automatic SDL rewrite.

Detection is based on directive and argument positions in the AST. Descriptions,
ordinary strings containing the old name, and user fields named
`representation` are not rejected or rewritten. Type checking, IR validation,
and execution all use the new vocabulary; documentation cannot move ahead of
the implementation. Ordinary Pythia generator and core paths do not gain Mock-
specific vocabulary injection or execution behavior.

## 4. Versions, identity, and migration

Although value semantics are unchanged, the declaration syntax and vocabulary
source change the Mock SchemaDigest. Preserving the old digest is not an
acceptance criterion. Digest changes are verified separately from unchanged
value validation, Luau types, and HTTP results.

| Boundary | Result | Reason |
| --- | --- | --- |
| Vocabulary | `pythia-mock-directives/v3` | Breaking syntax change. |
| Vocabulary source | `pythia-mock/v3.graphql` | Bind the snapshot appendix to the new vocabulary. |
| Update state | `pythia-mock-update-state/v5` | Keep the prior generation out of update. |
| Schema snapshot | `pythia-mock-schema-snapshot/v5` | Keep the prior generation out of update. |
| Project / bundle | Remain v4 / v6 | Their outer shape, configuration, and execution contracts do not change. |
| Host API / reference / proposal | Remain v5 | Their structures and operations do not change here. |
| Ordinary generated ABI | Remains 6 | The change is confined to Mock SDL vocabulary. |

Current project metadata stores a structural schema summary and digest rather
than a vocabulary version. Accepting old metadata in update based only on a new
vocabulary number would create an implicit migration. Therefore only the two
management formats advance; no compatibility reader or automatic converter is
added. Existing vocabulary-identity checks reject v2 in embedded bundle
schemas, proposals, and native inference input. An unchanged outer format does
not permit an old vocabulary or management generation to run or deploy.

Migration is an explicit operation:

1. Preserve the old project, U/B/N artifacts, `.dist`, proposals, seeds, and
   recovery material.
2. Change the user-owned SDL to the new syntax.
3. Run `init` in a new empty directory to create current metadata and types.
4. Move resolvers, hooks, configuration, and seeds into the new project; run
   `check` before `serve`.

Projects created with the new vocabulary retain ordinary `update` and `.dist`
protection. Editing only an old bundle or proposal header or hash is not a
migration. `reference --config-dir` continues to read configuration and current
SDL without adding management-generation or readiness checks. A successful
reference command does not prove that an old project can be updated or served.

## 5. Updated and unchanged areas

Implementation begins with a measured inventory that includes:

- `crates/mock/src/schema/{vocabulary.graphql,mod.rs,legacy.rs,snapshot.rs}`;
- `ir/constraints/scalar_spec.rs` and validation of locations, values, types,
  and vocabulary conflicts;
- `project/state.rs`, generated metadata, and bundle, proposal, and native
  vocabulary identity;
- every CLI input path, reference output, inference context, Luau types, static
  fixtures, and fake-backend fixtures; and
- current usage procedures, reference-generation metadata, and executable
  examples. Historical ADRs and validation records remain historical.

Generated and management artifacts must be regenerated through the canonical
CLI, not edited by hand. Rejection fixtures for previous versions and tests of
old-name errors must not be mechanically renamed. Rust IR representations, the
Luau model API, hooks and state, request atomicity, Soft and Hard behavior, and
seed consistency remain unchanged. New dependencies, new reference topics or
TUI behavior, real AI communication, application migration, and SSH or publisher
gates are outside scope.

## 6. Implementation order and acceptance

| Gate | Work |
| --- | --- |
| REP-A | Rename vocabulary, AST, and IR; focus tests on value semantics and rejection boundaries. |
| REP-B | Update identity and both management formats; regenerate canonically; verify CLI and fake inference plus nondestructive migration. |
| REP-C | Verify reference output, examples, ordinary-product non-impact, common checks, and designer review. |

| ID | Required assertion |
| --- | --- |
| REP-01 | Accept seven representations and six constraint arguments in the new syntax; test automatic vocabulary composition and equivalent definitions. |
| REP-02 | Reject old directive definitions and uses, old arguments, mixed syntax, unknown values or arguments, duplicates, missing `type`, and null `type`. |
| REP-03 | Reject built-in overrides, invalid locations, and conflicting definitions without rejecting ordinary strings or field names. |
| REP-04 | Preserve independent expectations for precision, finiteness, UTF-8 bytes, arrays, nulls, containers, and boundaries. |
| REP-05 | Test constraint composition across scalars, fields, inputs, and arguments, including the date-time example and the distinction from `@specifiedBy`. |
| REP-06 | Keep the new SchemaDigest, appendix, and vocabulary version consistent; reject v2, altered digests, and both old management formats. |
| REP-07 | Reject old projects, updates, bundles, and proposals without changing U, `.dist`, seeds, backups, or indexes. |
| REP-08 | Exercise canonical CLI `init` → `check` → `update` → `check` → `export` → `serve` without the original SDL. |
| REP-09 | Through real HTTP, test valid custom scalars, invalid returned values, full rollback after a late failure, and recovery of the same server. |
| REP-10 | Keep text, JSON, reference, and fake-inference vocabulary consistent; reference must not execute resolvers, seeds, or AI. |
| REP-11 | Preserve ordinary ABI 6 and MSRV 1.85, Mock MSRV 1.95 and stable, exact quality checks for all SDKs, and existing safety regressions. |
| REP-12 | Record every ID, command, result, and source mapping, important untracked artifacts, unexecuted environments, and migration procedures. |

REP-A owns REP-01 through REP-05, REP-B owns REP-06 through REP-09, and REP-C
owns REP-10 through REP-12 plus complete ID reconciliation. Focused tests and
normal caches are used during implementation; the expensive common
`validate.sh` pass runs once against final source. Existing invalid-value,
rollback, and secrecy tests must not be removed to admit valid new syntax. The
400-line handwritten-source limit, English first-party prose, generated-output
ownership checks, and canonical regeneration remain mandatory. Automated
verification, designer review, unexecuted remote CI, and manual verification
must be reported separately.

The 2026-09-15 implementation verified vocabulary v3, both v5 management
formats, canonical generation, rejection of old paths, real HTTP behavior,
reference output, fake inference, and ordinary-product regressions. REP-01
through REP-12 and Gates REP-A through REP-C passed locally. The
dedicated validation record is
authoritative for source mappings, commands, corrections, independent
read-only review, measurements, and unexecuted environments.

## 7. Design baseline and follow-up

The observed baseline was main commit
`3d785000d889673b57f0569bff605da7d4a2d98f`, with a clean worktree and index.
This is not an instruction to reset future implementation work; implementers
must recheck current state and preserve existing changes. Drafting this ADR
initially added design documents only. Product implementation was later
completed under the
dedicated goal prompt.
Staging, committing, pushing, publishing, tagging, real AI connections, and
changes to other repositories were not authorized by that work package.

Human reference improvements are considered separately using the
research results. They are not a
prerequisite for this rename and must not be added retroactively to this ADR's
completed acceptance record.
