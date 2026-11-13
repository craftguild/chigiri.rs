# ADR-0041: Mock reference topics and structured navigation

- Status: Accepted
- Implementation: Implemented
- Review: The residual U+061C rendering issue was corrected and re-reviewed;
  final design confirmation is pending.
- Date: 2026-09-15
- Decision owners: Pythia maintainers
- Approval: On 2026-09-15, the instructor requested that the follow-up goals
  and implementation of the research proposal be made explicit, with a
  human-readable presentation added the same day to reduce cognitive cost.
- Depends on: ADR-0031's shared `ReferenceDocument`; ADR-0035 and ADR-0037's
  built-in viewer and terminal-security contracts; ADR-0040's directive
  vocabulary
- Amends: ADR-0031's default text and reference JSON presentation, and
  ADR-0037's reference-view screen
- Preserves: host API v5, project v4, bundle v6, generated ABI 6 and ordinary
  MSRV 1.85, and Mock MSRV 1.95

## 1. Background and decision

The existing reference implementation already shares its contract with Mock IR
and can produce complete text and JSON documents. The complete document exposes
identity, the type graph, rule IDs, syntax, and short examples, but without a
catalog the TUI can only scroll through one finished text page.

This ADR adopts the follow-up program proposed by
Mock reference usability research:

1. Add the closed positional grammar `reference [TOPIC] [NAME]`.
2. Make the no-argument text output a short landing page and move the complete
   text to `reference all`.
3. Add a directive catalog to the shared document, distinguishing GraphQL
   standard directives, Pythia Mock directives, and user declarations.
4. Upgrade the complete reference JSON to v6 and return a closed,
   self-contained topic document when a topic is selected.
5. Replace the single-page TUI with structured navigation between topic lists,
   item lists, and detail pages.
6. Give text and TUI output the same man/less-style title, status, and key
   guidance.

This decision does not retroactively change ADR-0040's date, implementation
status, or acceptance history. ADR-0041 is accepted and implemented
independently.

## 2. CLI grammar and selection rules

The CLI grammar is:

```text
pythia-mock reference [TOPIC] [NAME] [--schema SDL ... | --config FILE | --config-dir DIR]
                      [--format text|json] [--no-pager]
```

`TOPIC` is one of `directives`, `methods`, `hooks`, `models`, `limits`, or
`all`. `NAME` is accepted only for the `directives`, `methods`, and `models`
topics. A third positional argument, an unknown topic, or a disallowed name is a
usage error with exit code 2 and no stdout output.

| Invocation | Text | JSON |
| --- | --- | --- |
| `reference` | Short landing page with six topics and copyable next commands | Complete v6 document |
| `reference TOPIC` | Topic item list | Self-contained topic document; `all` returns the complete document |
| `reference TOPIC NAME` | One item's detail page | Self-contained topic document containing one item |
| `reference all` | Complete text containing the previous reference information | Complete document, byte-identical to no-argument JSON |

An unknown `NAME` is a usage error with exit code 2. Candidate suggestions are
derived from the completed document and the closed vocabulary. `models` is empty
when no schema is supplied, and schema input must never present fictitious
User/Post models as real. Existing explicit-input, exclusion, schema-limit,
`--format`, and `--no-pager` semantics remain unchanged.

## 3. Directive catalog

The complete `ReferenceDocument` gains a stable `directives` sequence derived
from a fully composed semantic schema. With no input, the existing schema
synthesis path composes a small built-in schema containing five GraphQL standard
directives and four Pythia Mock directives. Schema, project, and bundle inputs
use the same verified IR semantic schema and retain user-declared directives.

| Class | Members | Execution meaning |
| --- | --- | --- |
| `graphql_standard` | `skip`, `include`, `deprecated`, `specifiedBy`, `oneOf` | Core GraphQL meaning |
| `pythia_mock` | `relation`, `virtual`, `constraint`, `representation` | Pythia Mock validation and execution meaning |
| `user_declared` | Other user definitions | Declarative information only; no implicit Mock execution meaning |

Each directive entry contains its kind, name, class, execution category, use,
repeatability, and locations. Each argument contains its name, type,
requiredness, default, and closed options. Locations and defaults are derived
from the semantic schema. Human descriptions and examples are presentation
metadata keyed once by stable names; they must not become a second handwritten
syntax table. Options for `representation.type` come from its lexical enum, and
values for `constraint.format` come from the executable's closed enum.

Each of the four Mock directives has separate positive and negative SDL
examples, exercised through the existing parser and Mock IR. A user directive
is identified with `user_sdl` origin and declaration-only execution; it is not
presented as a Mock command, host contract, or executable directive.

## 4. Man-style text view

Every text page starts with the fixed title `PYTHIA-MOCK-REFERENCE(1)`. The
landing page uses `NAME`, `SYNOPSIS`, `DESCRIPTION`, `TOPICS`, `EXAMPLES`, and
`SEE ALSO`; topic lists use `ITEMS`; detail pages select from `SYNTAX`,
`ARGUMENTS`, `OPTIONS`, `SEMANTICS`, `FIELDS`, `RELATIONS`, and `IDENTIFIERS`.
The complete view also retains the existing `LIMITS`, `SCHEMA`, and `EXAMPLES`
material.

The landing page lists `directives`, `methods`, `hooks`, `models`, `limits`, and
`all` in that order. Each entry gives a one-line role and a copyable next
command. Topic lists lead with name and use, followed by classification or
read/write status. Detail pages present:

1. name and one-line use;
2. GraphQL/Mock/user ownership, or execution authority and eligibility;
3. syntax and the smallest valid example;
4. arguments, requiredness, defaults, and options;
5. key rules, failure examples, and related items; and
6. stable IDs and a `reference all` command for complete context.

`reference all` preserves all identity, method, option, type-graph, rule,
limit, schema-capability, and example information defined by ADR-0031. Standard
and topic text is plain UTF-8. Control and bidi characters are rendered visibly
without escaping ordinary non-ASCII text, Japanese text, or emoji, and the
meaning is identical across TTY, pipe, and file output.

## 5. JSON v6

Adding the directive structure advances the document format to
`pythia-mock-reference/v6`. The host API, proposal format, project format, and
bundle format do not change. The v5 reader and automatic conversion are
removed. Proposal identity and agent context use the complete v6 document.

No-argument JSON and `reference all --format json` are complete documents with
`directives` added to the existing root structure. A selected topic uses this
closed outer frame:

```json
{
  "format": "pythia-mock-reference-topic/v1",
  "reference_format": "pythia-mock-reference/v6",
  "api_version": "pythia-mock-api/v5",
  "input_kind": "schema",
  "readiness_checked": false,
  "selection": { "topic": "directives", "name": "representation" },
  "content": { "kind": "directives", "items": [] }
}
```

`content.kind` must match `selection.topic`. Every `items` variant contains the
syntax, use, constraints, and examples required for display. Directive items
carry `rule_ids`; every referenced rule's `id`, `description`, and
`failure_class` is included in the same topic document's `rules` collection.
Partial documents may not contain dangling references to arrays that were
omitted for size. Unknown fields, topic/content mismatches, and old-version
documents are rejected. Stable ordering makes identical input and selection
byte-identical.

Topic selection is limited to human CLI browsing and explicit partial JSON. It
does not mutate a current selection or persist terminal state. New entries must
not contain raw scripts, seed values, request values, initial state, absolute
paths, or environment values.

## 6. TUI navigation

Text output to a TTY starts the structured built-in viewer unless `--no-pager`
is present. The viewer has three levels: topic list, item list, and item detail.
Enter moves forward; Backspace returns to the previous level. The `all` command
opens the complete text in the same viewer. Existing navigation remains:
`j`/`k` or arrow keys, Home/End, PageUp/PageDown, `g`/`G`, `/`, `n`/`N`,
Space, `q`, and Ctrl-C.

`TOPIC NAME` starts on the detail page and Backspace returns through the item
and topic lists. The complete view uses the same fixed
`PYTHIA-MOCK-REFERENCE(1)` title. Status shows `(END)` at the end and
`--More--` otherwise. Search displays `/QUERY`; the footer advertises the
relevant man/less keys and shows location and search state without exceeding
the existing narrow-terminal input bounds.

Existing `NO_COLOR`, pre-initialization fallback, startup-failure handling,
signal handling, bounded I/O, and alternate-screen/raw-mode restoration remain
unchanged. Every TUI page is rendered from the common presentation view; the
TUI owns no parallel descriptions or syntax. JSON, `--no-pager`, and non-TTY
output bypass the TUI and write completed output directly. This ADR does not
add an external pager or a second document-processing system.

## 7. Resources, failures, and non-goals

Building a `ReferenceDocument` and rendering each text or JSON output are each
bounded at 16 MiB. Directive and navigation data are charged to those existing
reservations. Navigation retains only the completed physical page; it does not
copy every selection combination or schema graph. Existing screen, input, and
terminal-write bounds remain. Exceeding a bound is an error, never successful
truncation or silent item omission.

Selection and full rendering finish before stdout is written. Schema and
configuration errors exit 1; CLI usage, environment, and I/O errors exit 2;
success exits 0. Broken pipe is the only I/O condition treated as successful
early termination; disk-full and other write failures remain errors.

This ADR adds no host API, executable directive, Markdown or HTML renderer,
external pager, AI transport, hidden schema search, or compatibility reader.
external applications, publisher and SSH behavior, and publish/tag operations are outside
scope.

## 8. Implementation order and acceptance

| Gate | Work package | Result required |
| --- | --- | --- |
| REF-A | WP-REF0/1 | Baseline and ADR fixed; directive catalog and JSON v6 implemented |
| REF-B | WP-REF2 | Closed CLI topics, short landing page, list/detail text, and closed partial JSON |
| REF-C | WP-REF3 | TUI navigation, PTY/non-TTY behavior, restoration, and exit handling |
| REF-D | WP-REF4 | Identity and document synchronization, common verification, measurement, review, and handoff |

| ID | Required assertion |
| --- | --- |
| REF-01 | Closed CLI tests cover six topics, `NAME` availability, unknown/repeated/excess arguments, help, candidate suggestions, and empty stdout on failure. |
| REF-02 | Default text has six entries and fewer than 30 lines; full text has the fixed title and man sections; `all` retains the previous complete information and catalog. |
| REF-03 | The semantic schema yields exactly five standard and four Mock directives in stable order, with class and execution meaning kept distinct. |
| REF-04 | Directive locations and argument name/type/required/default data match the semantic schema; mutation tests cover seven representation options and eight format options. |
| REF-05 | Each of the four Mock directives has an independently valid IR example, and each negative example is rejected with the expected classification. |
| REF-06 | User directives report `user_sdl` origin and declaration-only execution without acquiring Mock execution meaning. |
| REF-07 | Method, hook, model, and limit list/detail counts, permissions, types, and values agree with the complete document, public API, and IR. |
| REF-08 | Schema input, a second domain, renames, and unknown-model candidate selection never mix in fictitious User/Post models. |
| REF-09 | Complete JSON v6 round-trips as a closed document, remains deterministic, integrates every ID reference, and rejects v1 through v5 readers. |
| REF-10 | Topic/content/name/readiness/version consistency, unknown-field rejection, single-item selection, local rule resolution, reference closure, and the 16 MiB bound are tested. |
| REF-11 | No-argument and `all` JSON are byte-identical complete v6 documents; context/proposal identity always uses the complete document rather than a topic or TUI state. |
| REF-12 | TestBackend covers topic → items → detail, `all`, full text, selection restoration, scroll and search positions, fixed title, `(END)`/`--More--`, and key guidance. |
| REF-13 | Real PTY tests cover man-style topic → items → detail navigation, `all`, scrolling, search, `q`/signal exit, terminal recovery, and unchanged non-TTY/JSON/`--no-pager` output. |
| REF-14 | Tests cover default-input readiness, secure bidi visualization with unchanged JSON meaning, non-execution and secrecy of VM/resolver/hook/seed/AI/network data, BrokenPipe success, and other I/O failures. |
| REF-15 | Formatting, Clippy, rustdoc, MSRV/stable, ABI 6, all SDKs, full Mock tests, the 400-line checker, measurement, documentation, and the complete trace pass. |

REF-A owns REF-01 through REF-06, REF-B owns REF-07 through REF-11, REF-C
owns REF-12 through REF-14, and REF-D owns REF-15. Run focused tests before a
single heavy common-validation pass against the final source. Run the
implementation-rule checker throughout. Verification records must distinguish
automated checks, human UI verification, remote CI, and designer review.

## 9. Handoff and history

The dedicated verification record owns the starting baseline, dirty-worktree
preservation, commands and results, source support, measurements, and
environments that were not exercised. This ADR does not copy a Passed status
from an earlier ADR. Staging, committing, pushing, publishing, and tagging are
outside the implementation request.

REF-01 through REF-15 and Gates REF-A through REF-D received local
implementation acceptance on 2026-09-15. See the
dedicated verification record for
measurements, source support, and environments not exercised. The designer
review that day requested changes: three medium and one low finding. The
2026-09-16 re-review covered the ordinary `SEE ALSO` command. The remaining
U+061C visualization issue was corrected using the complete 12-character
`Bidi_Control` set and a regression test. Final design confirmation remains
pending.

The ordinary product MSRV 1.85, ABI 6 and SDK checks, and VM
suspension/cancellation/worker-recovery tests remain mandatory. Records must
not conflate local release measurements with remote CI, unexecuted CI, or human
final design approval.
