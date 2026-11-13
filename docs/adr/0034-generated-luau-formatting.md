# ADR-0034: Format and validate generated Luau before saving

- Status: Accepted
- Implementation: Implemented (all 24 FMT IDs and Gates FMT-A through FMT-E passed locally; remote CI and designer review are reported separately)
- Designer review: Received 2026-09-09. The integrated ADR-0035 review passed with no findings at any severity (integrated verification record).
- Date: 2026-09-09
- Decision owners: Pythia maintainers
- Depends on: ADR-0030 Mock IR, ADR-0031 type and API generation, ADR-0032 editable project updates, and ADR-0033 inference candidates and approval
- Related: ADR-0027, 0028, 0029
- Scope: Static Luau generation for `pythia-mock`, formatting of AI resolver candidates, byte consistency across validation, approval, and storage, and the Mock-only formatter dependency
- Activation: A separate task after ADR-0033 implementation and designer review. Do not add it retroactively to completed MP or AI gates.
- Implementation goal: ADR-0034/0035 Integration Goal Prompt

## 1. Background and decisions

Resolvers are editable, typed Luau, but static renderers and AI did not produce
consistent indentation or line breaks.
Because users read, edit, and approve diffs of this source, newly generated content must be readable immediately.
Formatting after storage would change B/U/N comparisons, proposal digests,
approval content, and restored originals.

This ADR establishes the following principles:

> The system saves newly generated Luau only after formatting and verification.
> Do not overwrite existing files edited by users for formatting.
> Comparison originals, recovery material, and exports preserve existing bytes;
> they are not treated as newly generated source.

1. Embed the formatter in the Mock binary; require no additional CLI installation
   or network access.
2. Format each complete generated Luau source before saving it, never as a
   post-storage step.
3. Use the formatted bytes as a common input for type/conformance checking, digest, diff, approval, and placement.
4. An adopted AI candidate remains user-edited state U and is never promoted to pristine static baseline B merely because it was formatted.
5. The formatter is responsible for readability and determinism; it does not replace semantic verification or sandboxing.

The Accepted status approves this policy; it does not by itself claim that the
formatter has been integrated or that implementation acceptance has passed.

## 2. Target and non-target

| Route or material | Policy |
| --- | --- |
| Initial `mock.luau` | Format every newly generated source, run type, bounded-evaluation, and closed-shape checks, then save. |
| Initial Query, Mutation, and nested resolvers | Format the complete module and save it only after the existing type and module checks pass. |
| `generated/types.d.luau` | Combine the fixed host declarations with the formatted type declarations from Section 3.2 and include the result in the digest. |
| New or replacement resolver and `.dist` during update | Generate a formatted N and perform only the placement allowed by B/U/N and ownership rules. |
| AI resolver candidates from `infer` | Format, validate, store, and display the candidate text. |
| Resolver installed by `apply` | Recheck the stored candidate for conformance, then install the exact stored text without modification. |
| Existing edited `mock.luau` or resolver, retained `.dist`, or orphan | Never format automatically. AI replacement continues to require target selection and diff approval. |
| Proposal comparison original, backup, or recovery original | Preserve the original bytes. Do not modify or reject material merely because it is not canonically formatted. |
| Existing resolver body embedded in an export | Preserve it. Do not reformat the source project or bundle text. |
| JSONL seed, SDL, JSON, or complete TOML document | Outside this ADR's formatter scope. |

Targets come from the existing IR, role, and ownership inventory rather than an
extension glob. A new Luau candidate for `.luau.dist` is in scope. Do not require
active user source to be canonically formatted before `check`, `serve`, `export`,
or `reference`. Ownership and digest checks and generated-type readiness remain
mandatory; preserving unformatted source does not make invalid input acceptable.

Do not add public `fmt` or `format` commands, `--no-format`, per-project style
overrides, editor or LSP integration, recursive arbitrary-file formatting, or
automatic merging of handwritten source. Generated Luau is not subject to the
400-line handwritten-file limit; keep the existing coordinate-based layout.

## 3. Formatter and styles

### 3.1 Built-in and fixed policies

The initial formatter will be the Rust library API of [StyLua 2.5.2](https://github.com/JohnnyMorganz/StyLua/tree/v2.5.2).
The Cargo package is `stylua`, the library is `stylua_lib`, and the version is
pinned to `=2.5.2` with `default-features = false` and `features = ["luau"]`.
The normal `Cargo.lock` fixes dependency resolution. The implementation does not
run an external `stylua` process, download tools, search user, home, or working-
directory configuration, or use EditorConfig or LSP features.

The style identifier is `pythia-mock-luau-format/v1`, and the following is explicitly set.

| Item | Fixed value/meaning |
| --- | --- |
| syntax | Luau |
| indent | Spaces, 4 |
| column width | 100; a wrapping guideline, not permission to truncate long identifiers or strings |
| line endings | LF |
| quote style | `AutoPreferDouble`; string values remain unchanged |
| call parentheses | Always |
| simple statement | Never |
| block newline gaps | Never |
| sort requires | Disabled; statement execution order must not change |
| space after function names | Never |
| output verification | Full; enable formatter-side reparsing and AST matching |
| end of file | Exactly one newline after the final token or comment |

Unknown formatter settings are ignored. Any future formatter or style change
requires review of byte-level differences in candidates and type declarations.
Formatting must preserve ordinary comments, `--!strict`, quoted and long-string
values, types, operator grouping, and statement order.
Do not trim every line or change indentation and spaces with regular-expression replacement, because that can corrupt string values and comment boundaries.
Generated candidates containing a `stylua: ignore` control comment are not
adoptable because that directive disables formatting. The same text inside a
string is not a control comment. This restriction does not apply when ordinary
user-owned source is merely read.

Correction added on 2026-09-10: removing type-cast parentheses and wrapping
arguments could make the first and second formatter outputs differ even with a
fixed version. Before storage, run the same formatting policy at most three times
and accept only when two consecutive outputs are identical. Apply full AST
verification and the existing byte limit to every pass; fail if no fixed point
is reached. This correction prevents repeated placement changes and apply-time
reformatting. It does not change the style identifier, version, I/O boundary, or
isolation deadline.

### 3.2 Host type definition

The current `declare extern type PythiaNull with ... end` declaration is not
resolver source executed at runtime.
Do not assume that StyLua will accept arbitrary Luau definition syntax.

Rust-owned host declarations use dedicated, already formatted templates. Regular
type declarations derived from the schema and API are formatted with StyLua and
then combined with those templates.
A dedicated generator structure enforces this separation; regular expressions
do not extract unknown `declare` sections from source. The complete combined
declaration has identical bytes in the generated file, contract, type checker,
and inference context.
Do not weaken the nominal host null type to `any` or a table type. New host syntax
requires explicit template and validation updates.

### 3.3 Dependencies and licenses

Maintain Rust 1.85 as the ordinary product and generated-SDK MSRV, Rust 1.95.0
for Mock, ABI 6, and core's zero normal dependencies. Confine the formatter and
its transitive dependencies to Mock; do not add them to the GraphQL generator,
runtime or external applications.

Most dependencies use licenses already permitted by the repository. StyLua
requires a user-approved MPL-2.0 exception. Dependency inspection on 2026-09-09
confirmed that its required parsers, `full_moon 2.2.0` and
`full_moon_derive 0.11.0`, are also MPL-2.0.
On the same day, the user explicitly approved these limited package-and-version exceptions and asked that the approval be recorded. Only these three packages are permitted.
`deny.toml` limits each exception by package and version; it does not grant a
general MPL or GPL-family allowance.
Do not copy StyLua source into first-party implementation files; use it through the library boundary.
Distribution documentation identifies each version, its license, and where to
obtain the corresponding MPL source. If MPL-covered source is modified and
distributed, record the offered source and distinguish that obligation from any
claim that the whole product or an original file changed license.
Refer to [questions 8–11 of Mozilla's official FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/) and the license text.
Do not omit the transitive license check during implementation, and do not arbitrarily expand the permission range if additional exceptions are necessary.
Publisher identity, SSH, signatures, publication, tags, and package-release gates
are outside this ADR.

## 4. Static generation/update processing order

The normal path is: generate source from IR, format the complete source, run the
existing checks, derive digests and management metadata, then stage and place it.
Existing individual and total byte limits are applied both during generation and after formatting.
Type-declaration digests, snapshots, and management metadata all use the formatted
bytes; the file is never replaced independently afterward. During initial
`mock.luau` generation, format once before the existing settings checks rather
than evaluating two differently formatted configurations.

Update retains ADR-0032's comparison model:

- B: Last pristine static bytes represented by the digest in management metadata.
- U: Exact user-file bytes read at startup; never format or normalize them before comparison.
- N: A formatted and inspected static candidate generated from this IR and fixed formatter.

If `U == B`, update may replace it with N. A whitespace-only change that makes
`U != B` is still protected as a user edit. Existing rules for adopting
`U == N`, retaining `N == B`, and presenting `.dist` for conflicts remain in
force.
Even when a formatter introduction or upgrade changes N, the comparison remains
the same. U is never formatted and reclassified as pristine.
Do not overwrite an existing edited `.dist`. Updates to unedited `.dist` match ownership with existing record digest.
AI adoption does not advance B. Later `U == N` updates follow only ADR-0032's
explicit rules.

Do not place a static candidate that cannot be formatted, and never publish only
part of a multi-file update in the new style.
Reuse the existing transaction for concurrent-edit detection, no-follow reads,
hardlink protection, the short placement lock, rollback of every target, data
retention after restoration failure, and classification of cleanup failures that
occur after commit.

## 5. AI candidates and approval

### 5.1 Formatting is the responsibility of the Mock generator

The Codex adapter first completes terminal-event, final-message, and exit-status
validation while retaining the original response. Mock then validates the closed
candidate protocol and formats only resolver bodies. The formatter must not alter
the backend trait, JSONL seed, model description, optional instructions, or
reference context.

Pass the complete formatted module to the existing type, closed-module,
initialization, and partial-evaluation checks. Correction requests include the
candidate bytes actually checked and a private diagnostic, so unrelated failures
do not discard an already valid candidate. Preserve the initial attempt, at most
two correction attempts, serial batches, and one shared 600-second deadline.

Invalid syntax, an unsupported formatter, failed semantic or AST-equivalence
validation, and post-format capacity overflow are nonconforming. Never fall back
to an unformatted candidate merely because it was otherwise valid. Syntax and
capacity failures return a `format`-stage diagnostic to the existing correction
loop.
Do not misclassify abnormal formatter termination, environmental failure,
cancellation, or deadline expiry as a model-semantic error eligible for an
automatic correction request.
Diagnostics use a target, logical file and location, fixed reason, and exit class.
Normal logs omit raw source, provider messages, and absolute paths.

For failed candidates, the original text can be saved as non-adoptable material for recovery/investigation.
Protocol-invalid and formatting failures remain distinct, mid-operation
cancellation is `not_evaluated`, and the original text is never presented as
formatted output.
For candidates that have been formatted but fail the type check, the formatted text that was actually inspected is saved.
This does not weaken the format-and-preserve contract for adoptable Luau; it retains the exact failed material for diagnosis.

### 5.2 Proposal and apply

Update the proposal to `pythia-mock-proposal/v2` and record the required closed formatter information.
```json
{
  "formatter": {
    "name": "stylua",
    "version": "2.5.2",
    "style": "pythia-mock-luau-format/v1"
  }
}
```
This manifest section is neither a candidate signature nor proof that formatting
ran successfully.
It is also recorded as a generation environment in seed-only runs, but it does not claim that Luau formatting was performed on the seed.
The starting bytes define the original; the formatted and inspected text defines
the candidate; and the stored bytes define its digest and size. Preserve the
existing closed-key, identity, inventory, capacity, digest, path, and descriptor
checks.

Immediate adoption after `infer` and later `apply` both take the same candidate
through a full-text diff, per-file acceptance or rejection, review of the
accepted set, and final approval.
Verify that formatter metadata is currently supported and, in read-only mode, that reformatting each selected candidate with the same policy produces identical bytes.
Do not rely only on `valid` and formatter metadata; repeat existing type, seed, and relation-set checks.
Discrepancies are treated as errors that require re-inference/candidate creation, and candidates are not modified or re-stamped within apply.
Do not place a newly reformatted copy; install only the bytes retained for
approval.

Therefore: stored candidate = validation input = candidate side of the diff =
approval target = installed bytes.
Canonical style inspection is not required for normal user files, comparison originals, and backups.
Changes to originals or non-target files, schema, API, type or management drift,
cancellation, and commit ordering continue to follow ADR-0033.

## 6. Resource control and execution boundaries

Every formatter entry point accepts complete source under route-specific limits
and returns either verified owned bytes or a confidential error.
File IO, config search, Luau execution, and backend communication are not included in the formatter's responsibilities.
Type declarations are generated and formatted once per IR and then shared; they
are not reformatted for every resolver.

- Maintain existing limits, including 32 KiB per complete resolver, 256 KiB for configuration, 1 MiB per type or wrapper, and 16 MiB total.
- Keep caps on AI responses, decoded candidates, batches, and proposals, and charge formatting expansion to the same total budget before storage.
- Check input capacity before parsing and output capacity before candidate acceptance. Avoid unbounded copies of large inputs containing internal strings.
- Route formatting of AI-derived source and apply-time conformance checks through the existing isolated same-binary worker and process-ownership mechanism.
- A timeout does not prove that a non-interruptible parsing or formatting thread stopped; never detach and forget such work.
- Share absolute deadlines and cancellation across automated steps, and collect processes and pipes within the existing five-second cleanup budget.
- Exercise deep structure, high token counts, formatting expansion, and abnormal termination in debug and release builds. Distinguish library allocations from the OS process-wide limit.

Do not satisfy a limit by truncating an oversized formatter result or by storing
unformatted source after formatting fails.
Do not assume that routes using statically generated bounded IR and untrusted AI sources have the same reliability.
Formatting must not alter existing type-checking, VM or host budgets, Mutation
rollback, or null and relation semantics.

The internal same-binary inspection response returns the complete bounded bytes
of each selected candidate that was formatted and checked.
The parent verifies the request digest, response mode, exact selected-path set,
absence of duplicates, resolver capacity, and agreement with the original seed
bytes.
If formatting itself fails, retain the original as nonconforming evidence. If a
later validation step fails, retain the formatted bytes that were actually
checked.
Preserve the hidden worker entry point, closed request shape, absolute deadline,
and process recovery. This internal same-binary response change does not alter
the Codex adapter's candidate protocol or its contract with an external backend.

## 7. Compatibility and migration

Proposal v1 was never released publicly, so no compatibility reader or implicit
conversion is provided. Preserve existing material and create a new inference
candidate when migration is required.
Project v1, bundle v3, API v2, reference v2, and GraphQL ABI 6 remain unchanged.
The current update mechanism handles changes to statically generated bytes; no
compatibility layer embeds or reproduces an old formatter. If `check` or `infer`
finds ownership metadata from an old generator, instruct the user to update first.
Do not discard existing edited config/resolver and JSONL seed in the name of regeneration.

Update proposal schema, formatter metadata, fixtures, validators, renderers, and
documentation together.
Do not replace the existing GeneratorInfo of CLI/SDK with the formatter version, and treat the build revision and formatting policy as separate information.
Do not pull normal SDK generation or LSP/completion specs into this task.

## 8. Alternatives and Consequences

- Run an external formatter after storage: rejected because it introduces
  post-digest and post-approval mutation, environmental differences, another
  installation requirement, and partial-failure states.
- Hand-code indentation in each renderer: rejected because it cannot normalize
  arbitrary valid Luau returned by AI and scatters formatting logic.
- Ask the AI to format output: rejected because it is neither deterministic nor
  independently verifiable and adds unnecessary inference calls.
- Build a custom formatter or source converter: rejected initially because
  correct typed syntax, comment and string preservation, and equivalence checks
  would require substantial duplicate implementation.
- Normalize every user file before comparison: rejected because even a
  whitespace-only edit is protected by the ownership contract.

Benefits include editable source from the outset, readable candidate diffs, and deterministic formatting in the same CLI.
Costs include a larger Mock binary and dependency graph, longer builds, possible syntax differences between the formatter and `luaur`, initial generated-byte changes, and MPL distribution guidance.
Verify formatter compatibility against real fixtures before implementation. If
it cannot satisfy the contract, revisit the design instead of adding an
unverified fallback or substituting an arbitrary formatter.

## 9. Implementation order and acceptance

### 9.1 Work packages

1. WP-FMT0: Record HEAD and worktree state, inventory generation and storage
   paths, preserve existing changes, and verify StyLua syntax, licenses, and
   dependency boundaries.
2. WP-FMT1: Implement fixed policy, common formatting, host declaration, resource/secret errors, and connect to static init/update and digest.
3. WP-FMT2: Connect AI candidate/correction loop, proposal v2, isolated worker and apply read-only conformance test.
4. WP-FMT3: Add B/U/N, original preservation, PTY/HTTP, cancellation/abnormal termination/recovery, and upper bound regression.
5. WP-FMT4: Complete final common validation, dependency and license checks, measurements, independent Review A/B and post-fix re-review, and usage, migration, and handoff records.

Prioritize focused tests during implementation and reserve time-consuming shared full validation for the final stage.
When ADR-0034 and ADR-0035 share one goal, use completion and focused verification
of WP-FMT0 through WP-FMT3 as an intermediate checkpoint before continuing to
ADR-0035. WP-FMT4 and WP-DX35-5 may share final-source validation, measurement,
and independent reviews. The intermediate checkpoint does not pass every FMT
gate; final acceptance must trace every ID from both ADRs. Run the handwritten
400-line checker after every subtask, after formatting, and before handoff. Do
not hand-edit generated output. Real Codex calls are not an FMT gate; use a closed
fake backend and executable. Any additional real inference requires explicit
approval after disclosing transmitted content and possible charges.

### 9.2 Closed acceptance inventory

Initialize every ID below as Required / Not run, then record its individual
assertions and execution results rather than merely naming a test.

| ID | Required confirmation |
| --- | --- |
| FMT-01 | The pinned StyLua library and Luau settings handle two domains, renamed models, and current type syntax without external formatter or configuration discovery. |
| FMT-02 | The fixed style is byte-idempotent and deterministic across input order, locale, and working directory, with LF endings and the required final newline. |
| FMT-03 | Formatting preserves comments, the strict directive, quoted and long-string values, operator grouping, and type and statement order, while distinguishing ignore directives from the same text in strings. |
| FMT-04 | Fixed host nominal types and formatted public types produce identical bytes for files, type checks, inference context, contracts, and digests. |
| FMT-05 | Initial configuration and CRUD, manual, and nested resolvers are formatted and checked before storage; failure never publishes a partial project. |
| FMT-06 | Update derives formatted N, replaces `U == B`, adopts `U == N`, retains `N == B`, and applies the existing `.dist` conflict rules. |
| FMT-07 | A whitespace-only edit to U, edited config or `.dist`, and orphan files remain protected; unrelated resolver paths and bytes do not change. |
| FMT-08 | Repeating update with identical input creates no unnecessary diff or metadata rewrite, and type files, management metadata, digests, and stored bytes agree. |
| FMT-09 | `check` and `serve` accept valid unformatted U; export text, comparison originals, and recovery originals remain unchanged. |
| FMT-10 | An unformatted AI resolver is checked only after formatting, without changing JSONL, description, context, original text, or the adapter's original response. |
| FMT-11 | Syntax, formatting, and output-limit failures enter the bounded correction loop; failed material is not adoptable and an existing valid candidate is retained. |
| FMT-12 | Proposal v2 has closed formatter metadata, digest, and size fields; missing, unknown, and v1 forms are rejected, while seed-only proposals do not claim source formatting. |
| FMT-13 | `apply` rejects modified, falsely valid, and noncanonical candidates and never repairs policy mismatch by reformatting or restamping. |
| FMT-14 | A real TTY shows the formatted full-text diff, records per-file decisions, reviews the accepted set, obtains explicit final approval, and installs the identical bytes; non-TTY mode only saves. |
| FMT-15 | AI adoption does not advance B; later updates retain the existing B/U/N rules and protect unselected source and originals. |
| FMT-16 | Limits cover original and candidate bytes, formatting expansion, wrappers, batches, proposals, and long types and identifiers at their boundaries. |
| FMT-17 | Debug and release tests exercise deep, excessive, or otherwise abnormal input and formatter panic or exit, while the parent can continue and retry safely. |
| FMT-18 | The inference correction loop and apply-time conformance checks share absolute deadlines and cancellation and collect every process and pipe; detached work is never treated as success. |
| FMT-19 | Transactions recover every target and retain evidence across concurrent edits, ownership conflicts, move failures, missing backups, restore failures, and post-commit cleanup failures. |
| FMT-20 | Formatting errors expose stable logical locations, reasons, and exit classes without logging raw source, provider content, or absolute paths. |
| FMT-21 | Static and AI candidates preserve existing HTTP, relation, rollback, and offline-bundle behavior for two domains and renamed models in one binary. |
| FMT-22 | Common validation covers Rust 1.85, Mock 1.95, stable Rust, ABI 6, all-SDK exactness and quality, dependency boundaries, and narrow license exceptions. |
| FMT-23 | Measurements record binary size, cold and incremental builds, generation, formatting, validation, and observed memory before and after; unmeasured behavior is never claimed as improved. |
| FMT-24 | Independent Reviews A and B, correction and re-review of every finding, complete ID tracing, important untracked and failed material, migration procedures, and unexecuted CI are all recorded. |

Gate FMT-A is 01-05, FMT-B is 06-09, FMT-C is 10-15, FMT-D is 16-20, and FMT-E is 21-24.
Review A independently checks meaning preservation, type, formatting/storage, and B/U/N, and Review B independently checks untrusted input, process/resource, ownership/approval, and license boundaries.
Keep Linux and macOS remote-CI results distinct from local results, and never
include unexecuted environments in a Passed result.
Do not add changes to external applications, commit/push/publish/tag, or real AI calls to completion conditions.

## 10. Documentation and rationale

This ADR is the formal FMT contract. Keep the README index, architecture,
handoff, and conformance documents synchronized with its references and status.
Usage, migration, recovery, measurements, individual assertions, and execution
results are collected in the
integrated validation record.
Do not mark it Implemented until every gate is accepted, and report local
automation, manual UI checks, remote CI, and designer review separately.
Existing Passed results and validation trails for ADR-0032 and ADR-0033 remain historical evidence.

- [StyLua 2.5.2 package/features/license](https://github.com/JohnnyMorganz/StyLua/blob/v2.5.2/Cargo.toml)
- [StyLua Rust library and output AST matching](https://docs.rs/stylua/2.5.2/stylua_lib/)
- [MPL 2.0 text](https://www.mozilla.org/en-US/MPL/2.0/) / [Official FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/)
- [ADR-0032](0032-mock-project-layout-and-portable-export.md) / [ADR-0033](0033-agent-assisted-mock-inference.md)
