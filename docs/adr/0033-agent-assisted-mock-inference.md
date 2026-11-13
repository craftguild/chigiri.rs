# ADR-0033: Add Mock inference with target selection, diff approval, and a Codex CLI adapter

- Status: Accepted
- Implementation: Implemented. WP-AI0 through WP-AI5, AI-01 through AI-28, and Gates AI-A through AI-E passed locally. The explicitly authorized two-domain real-Codex acceptance and final designer review also passed with no findings. Remote CI remains unconfirmed. See the validation record.
- Date: 2026-09-09
- Decision owners: Pythia maintainers
- Depends on: Mock IR for ADR-0030, model API/reference for ADR-0031, editing project/inspection/transaction for ADR-0032
- Related: ADR-0027, 0029
- Scope: `pythia-mock`'s `infer` and `apply` commands, candidate-generation trait, Codex CLI adapter, target-selection and diff-approval TUI, and saved inference candidates
- Activation: Implemented as a separate task after ADR-0032 was completed and reviewed; it does not retroactively change ADR-0032's completion gates.

## 1. Background and decisions

ADR-0032 provides a project format for editing typed Luau resolvers and JSONL seeds, with SDL as the source of truth.
Static `init` can generate explicit CRUD implementations, but users must still write behavior for search interfaces, non-CRUD operations, and practical initial data.
This ADR adds an independent development workflow in which AI proposes initial implementations or improvements and a human reviews and adopts the resulting diff.

1. AI is an assistant that produces editable candidates, not an execution authority that fully understands arbitrary schema semantics.
2. AI communication is isolated behind a small `InferenceBackend` trait. The initial release has one real backend: the Codex CLI.
3. The user selects inference targets before inference begins. Reference context may be broad, but write authority is limited to the selected files.
4. Candidates are saved, reviewed as file-level diffs, explicitly accepted or rejected, and then installed as one verified acceptance set.
5. Partial conformance is separate from overall readiness so that incomplete projects can be improved incrementally.

`init` and `update` remain deterministic static processes; `check`, `serve`, `export`, and `reference` do not invoke inference either.
AI is never called for each Query or Mutation during `serve`. After adoption, the existing Rust executor, store, and Luau VM execute the candidate without an AI dependency.
The ADR's Accepted status records a design decision and does not by itself establish implementation or acceptance-gate completion.

## 2. Boundaries and exclusions

| Responsibility | Owner | Does not own |
| --- | --- | --- |
| Request construction from SDL/IR/API, target/context selection, candidate verification, modification request | Mock inference processing | Codex command-line, internal format of session/authentication |
| Process startup, standard input/output, termination, cancellation, collection, conversion to backend results | Codex adapter | GraphQL, model operations, project ownership rules, meaning of `.dist` |
| diff, acceptance/rejection, simultaneous editing detection, placement/recovery | Mock project/UI | Codex-specific protocols and model internals |
| Types, permissions, values, relations, transactions, VM/host budget | Existing Rust inspection/execution infrastructure | Whether the AI evaluated it as "correct" |

Future Claude or local-LLM providers may be added as separate trait implementations, but they are not implemented in the initial release.
This decision excludes multi-stage fallback, automatic provider selection, a plugin system, and Rust ports of general-purpose AI SDKs or the Codex TypeScript SDK.
It also excludes model embedding or download, direct Responses API integration, AI-generated runtime responses, and model-learning or accuracy SLOs.

A backward-compatibility layer is unnecessary before public release, but project v1, bundle v3, API v2, and reference v2 remain unchanged.
The regular product and generated SDK retain MSRV 1.85; Mock remains on 1.95.0; ABI 6 and the core crate's zero regular dependencies remain unchanged.
Inference and TUI dependencies remain confined to Mock and do not enter the production Endpoint, GraphQL generator, or regular runtime.
Do not restore publisher, SSH acquisition protection, clean HEAD, package-release, publish/signature/tag to product completion conditions.

## 3. CLI and user operations

### 3.1 Inference

```bash
pythia-mock infer --config-dir mock --agent codex
```
`--config-dir` and `--agent` are required. The initial version accepts only `codex`; values such as `claude` are unsupported and produce a usage error.
The command does not add bundle input, format inference, implicit configuration discovery, or inference launched from `init` or `update`.
The following operations are provided as a lightweight TUI when both stdin/stdout are TTY.

1. Classify and display active inventory into Query/Mutation/nested resolver/seed. The initial state is that all items are not selected.
2. Search, navigate with arrow keys, and use Space to select multiple checkboxes. The interface also shows the count and list of selected items hidden by the current search.
3. Enter optional natural-language instructions, review the selected files and transmitted context, and explicitly start inference.
4. Display target/processing stage/number of revisions/remaining time and accept cancellation.
5. Open the saved candidate diff and accept or reject each file. Unreviewed candidates are rejected by default.
6. Show the accepted set's inspection results separately from overall readiness, and install the set only after final confirmation.

Do not implement a code editor, hunk-level adoption, or automatic three-way merge in the TUI. Users make original edits in their normal editor.
The TUI handles terminal resize, Ctrl-C and exit, and terminal restoration after abnormal termination. Raw CLI output never flows directly into the TUI.

For non-TTY use, specify both the targets and a new candidate-output directory. An arbitrary UTF-8 instruction file may replace interactive natural-language input.
```bash
pythia-mock infer --config-dir mock --agent codex \
  --target resolvers/query/getUsers.luau \
  --target seeds/User.jsonl \
  --instructions-file request.txt \
  --output proposal-dir
```
Only `--target` may be repeated. Each value must be a complete, active, project-relative path; reject globs, absolute paths, duplicates, and paths outside the target inventory.
Reject repeated singular options, unknown options, and empty target sets. In TTY mode, `--target` may provide the initial selection.
The confirmation step fixes the final transmitted target set. In TTY mode, `--instructions-file` supplies editable initial instructions.
When natural language instructions are omitted, a mock is proposed that respects the existing meaning using the SDL description, current implementation, and generated reference.

Non-TTY only saves the candidates and does not automatically apply them to the active file. Do not add `--yes`/`--force`/unauthorized application.
When TTY mode omits `--output`, candidates are saved in the dedicated area described in Section 6. An explicit path follows the same new-directory rules as non-TTY mode.

### 3.2 Reviewing and adopting saved candidates

```bash
pythia-mock apply --config-dir mock --proposal proposal-dir
```
`apply` is reserved for interactive approval and requires both stdin and stdout to be TTYs. It does not require `--agent`, an installed Codex CLI, or Codex authentication.
The same diff and approval UI can review completed candidates retained after an inference failure. It does not resume a session or repeat inference.
This common inspection and application process is also used when adopting immediately after the first infer.

Exit 0 means inference completed and valid candidates were acquired and stored for every selected target; it does not mean the project is ready.
Candidate nonconformance or absence and schema or ownership conflicts use exit 1. Usage, authentication, tool, or environment I/O failures use exit 2. User cancellation uses exit 130.
Useful completed candidates may be saved after a partial failure or cancellation, but the command still exits unsuccessfully and reports both the save location and incomplete targets.
Explicitly rejecting every candidate is a successful review outcome. If cleanup fails after installation, report that installation occurred, include recovery information, and exit nonzero.

## 4. Assembling target, context, and request

### 4.1 Readable range and changeable range

Only the active resolver/scaffold module and JSONL seed in the current IR/inventory can be changed.
Ready static CRUD implementations, existing handwritten implementations, and non-empty seeds are eligible; selection is not limited to unimplemented or empty files.
`mock.luau`, SDL, generated type/snapshot/update-state, `.dist`, orphan cannot be changed.
Deleting/renaming the target file, adding an arbitrary path, and changing unselected files are not included in the proposed protocol.
Missing active files and drifts in management information/SDL are resolved using existing update rules before restarting, and proprietary information is not repaired using AI.

The current SDL/IR and type, references from implementation contracts, all active resolver sources, and all active seeds can be used for context.
Do not add Rust branches keyed to schema-derived names or handwritten, AI-specific model API specifications.
External SDL referenced by IR is read by the existing Rust loader, and arbitrary repository sources, environment variables, and authentication files are not collected.
If schema/seed/source contains secrets, it can be sent via Codex, so specify it in the start confirmation and help.
Explicit infer execution of non-TTYs also assumes consent to this context scope, and configuration scanning alone does not initiate inference.

For large projects, context may be assembled by type, model, and relation around the selected targets and divided across multiple requests.
It is not required to send all active definitions at once each time. Do not infer without knowing the target and its type/constraints/API contract,
If the required context cannot be met, guidance will be given on target division, etc. There will be no silent truncation or automatic target expansion.

### 4.2 Candidate protocols and modified loops

The inference process maps the selected path to the opaque target ID and passes the instruction/context and expected JSON Schema to the adapter.
The model is returned as a closed candidate set with a target ID, complete file content, and bounded supplementary explanations.
The model does not specify an arbitrary path or shell command to apply the protocol.
Inspect JSON for unknown field/duplicate key/duplicate ID/unknown ID, target omission, non-UTF-8, depth/number of items/byte excess on Rust side.
The contents of Luau/seed are not considered correct even if they match the structured output.

Candidates undergo parsing, type checking, module initialization, and seed inspection. A failure reports a bounded diagnostic containing the logical coordinate, line, and stable reason.
A bounded diagnostic may accompany a correction request. The system must not make a candidate pass by expanding the model API or privileges, or by adopting it before repair.
Execute the selected requests in series, and each batch can be modified up to the first time and up to two times. Do not retry automatically inferring authentication/environmental failures.
The default automatic-processing deadline is 600 seconds across all batches and correction attempts; it is not reset for each call.
Time spent by a human selecting targets or reviewing diffs is excluded. If processing misses the deadline, completed candidates are saved and unfinished targets are marked incomplete.
A correction is a new request containing the required candidate and diagnostic. Session continuation is not a required concept in the common trait.

## 5. InferenceBackend and Codex adapter

The common boundary is one asynchronous request and one result. The following names describe responsibilities; they do not promise a stable public general-purpose SDK API.

| Element | Content |
| --- | --- |
| `InferenceRequest` | Owned instruction/context, opaque expected output schema. Do not pass Mock IR or project handle itself |
| `InferenceControl` | Remaining deadline, cancellation, input/output budget. Progress is notified using bounded common events |
| `InferenceResult` | Final response bytes, backend/CLI/model/usage information that could be obtained. Unobtained information is optional |
| `InferenceError` | Concealed classification such as unavailable/authentication/timeout/cancelled/output-limit/protocol/backend-failed |

`CodexCliBackend` resolves the user-installed `codex` executable from the normal `PATH` and invokes `codex exec`.
Mock inference does not construct the Codex argument vector, and the adapter does not perform GraphQL type checking or install candidates into a project.
The adapter uses the user's normal environment, authentication, and model settings. It does not download the CLI, log in, store an API key, or select a fixed model.

The adapter satisfies the following:

- Spawn directly with argv and send requests to stdin. Do not use shell expansion or embedding the source in the command-line.
- Use a private temporary working directory and do not pass live project as cwd/edit workspace.
- Explicitly specify `--sandbox read-only`, use settings that do not require interaction approval, `--ephemeral`, and `--skip-git-repo-check`.
- Request the final response shape with `--output-schema`. Keep the `--json` event stream separate from the final candidate JSON.
- Match exit status, successful terminal event, and complete final response. A message or exit 0 during the process alone will not be considered a success.
- Proceed with stdin sending and stdout/stderr reading at the same time, and handle blockages/mass output/inherited pipes as bounded.
- Stop and collect the process after timeout/cancellation/abnormal output. Do not consider it stopped just by joining handle drop.
- Convert raw stderr/events, authentication information, and absolute paths to fixed reasons and restart steps instead of sending them to diagnostics.

Confirm exact flag compatibility against official documentation and the installed CLI during implementation. Explicitly reject a CLI that lacks the required non-interactive, structured-output, or cancellation capabilities; never fall back to bypassing the sandbox or editing the project directly.
Trust the user's Codex environment, including MCP and hooks, without injecting a special environment that ignores the normal Codex user configuration.
Read-only mode is a policy for model commands, not proof that every external tool is isolated.
Distinguish the context collected by Lab01 from the privileges available to Codex itself. A prompt alone cannot enforce access restrictions.

Official OpenAI material was checked on 2026-09-09. The review covered exec and stdio behavior in [Non-interactive mode](https://developers.openai.com/codex/noninteractive), JSON events, output schemas, ephemeral mode, and existing authentication. These sources confirm external CLI behavior; they do not certify this product's safety.

## 6. Persistent candidates and diagnosis

Inference candidates use format `pythia-mock-proposal/v1`. The `.dist` format remains dedicated to static schema-update proposals and is not reused for AI output.
The default save location for TTY is `<config-dir>/.pythia-mock/inference/<run-id>/`. The run-id is reserved exclusively and does not overwrite existing runs.
`--output` is only for new directories and rejects conflicts with active/generated/SDL/existing proposals/recovery.
Do not place candidates in paths within the project other than the default inference area. Creating/owning/reading the parent also follows existing no-follow rules.

Each proposal stores the selected-target inventory; the original and candidate bytes and digests; schema, API, and project identities; prompt-format version; CLI build and backend information; instructions; context inventory and digest; validation results; and incomplete or failed targets.
It is not mandatory to store copies of the entire context text or raw process logs, and authentication files/environment snapshots are not saved.
The original/candidate source, seed, and instructions themselves may be confidential. The proposal is neither a hidden diagnostic nor a signed trail.
Specify this property in help/save result, and do not automatically commit/upload or automatically edit `.gitignore` of repository.

Responses that are being written are not published as candidates, and only completed targets are recorded with their originals.
Completed candidates remain reviewable after an interrupted run. Incomplete or nonconforming targets are not shown as adoptable.
Even after saving, re-inspect the closed manifest, file inventory, digest, and resource limits, and do not unconditionally trust modified candidates or intermediate writes.
Users normally edit active project files rather than proposal contents. Modifying a saved proposal must never enable automatic merging or reuse a previous approval.

The inference area is excluded from the active inventory used by `check`, `serve`, `export`, and `update`, and it is never an authority for SDL, types, or seeds.
The initial version performs neither automatic garbage collection nor recursive deletion of old runs. Document the storage location and purpose so users can remove obsolete candidates deliberately.
`apply` retains saved candidates and skips files already identical to the updated project. Previously unselected candidates may be reviewed later.

Diagnostics identify the logical path, coordinate, processing stage, fixed reason, acceptance or rejection state, partial-validation result, and overall readiness when applicable.
Because users explicitly review candidate diffs, that view may display source and seed contents. Normal error output remains separate from the diff body.
Terminal control characters included in source, model description, and path are rendered harmless, and ANSI commands in AI output are not executed as is.

## 7. Partial verification/approval/placement

### 7.1 Preserve preflight checks while enabling incremental edits

Inference preparation shares bounded evaluation of configuration Luau, SDL/IR loading, and capture of active ownership information.
An unimplemented resolver or in-progress Luau or JSONL file must not prevent inference, because those files are valid improvement targets.
Configuration, SDL, or ownership inconsistencies and I/O, path, or resource violations are rejected before inference.
Instead of making all of `CheckedMock::check` an unconditional preflight for `infer` and `apply`, the commands reuse its lower-level inspection components.

| Scope of inspection | Handling when applied |
| --- | --- |
| Adopted resolver | Required inspection of expected types derived from the same IR/API, independent parse/typecheck, bounded module initialization, and closed ready contract |
| Adopted seeds and related seeds | Confirm stored values/ID/order/numbering rules, owner/inverse references, cardinality, uniqueness, etc. using existing inspections |
| Unrelated unimplemented/nonconforming file | Preserve active bytes and report that overall readiness has not been reached; do not confuse this with a condition that blocks partial adoption |
| Overall readiness | A separate evaluation with the same meaning as the current check. Distinguish between pass/fail/not evaluated and reason |

Related seeds are those in the connected model set of the IR relation graph. Inspection combines adopted candidates with retained current seeds, never with unadopted candidates.
Static analysis does not claim to infer the full semantics of an arbitrary resolver, including its returned data and use of `ctx.models`.
A resolver-only type-check does not validate seeds or behavior; an acceptance fixture verifies those separately over real HTTP.
Partial tests also maintain common capacity tests such as source/seed aggregate. If a complete inspection is not possible, the reason will be clearly stated.

Reject an adoption set when the combination of accepted and rejected files breaks a relation, even if every accepted file is valid in isolation.
Report the required dependent files and prompt the user to select or infer them again, without expanding write scope automatically.
Type-invalid or unimplemented candidates cannot be accepted. A candidate that can be activated is not necessarily correct in its business semantics.
`check`/`serve`/`export` require total conformance as before, and do not relax the activation conditions due to partial application.

If native type checking lacks an internal interruption API, pass only a fixed snapshot to an internal checker process in the same binary.
The parent stops and collects that process under the same absolute deadline and cancellation policy as the inference batches. The checker reuses the same IR, API, and lower-level inspectors instead of reloading configuration or source into a different snapshot.
Internal IPC uses a closed inspection format separate from project, bundle, and API formats. Input, output, depth, and item counts are bounded during expansion, and each response is matched to its request bytes.
The original process-global admission and individual VM budget are maintained within each process, and are not reclaimed just by a thread drop or kill request.

### 7.2 Simultaneous editing and transactions

Save the original bytes at inference start and show diffs between those originals and the candidates. Before adoption, compare the current target bytes with the saved original.
If the target changed, reject adoption instead of letting approval of a stale diff overwrite it. Do not automatically rebase or mislabel reversal of user edits as recovery.
If the current bytes are already the same as the candidate, it is clearly indicated that writing is not necessary. Mismatches in schema/API/types/management generations require re-inference.
When a non-target file changes, report the context difference and repeat dependency verification and readiness evaluation against the latest retained file.

Do not hold exclusive locks during long inference or human review. Approval is bound to the candidate, original, and verification-snapshot identities.
At the start of placement, a lock is acquired and reverified, and if the input changes after inspection, the old verification results are not used.
Pass approved bytes to the installer as owned candidates. Do not reopen path and install different contents.
Reuse or extract the ADR-0032 transaction responsibilities and install every adopted file by rename as one rollback unit.
Preserve protection against symlinks, FIFOs, path-name swaps, shared inodes, failure of any move, cancellation, and missing or changed backups.
If one recovery action fails, continue restoring the remaining files and retain the primary cause, every recovery failure, and recovery material. Distinguish cleanup failure after commit from transaction failure.

AI adoption must not advance `update-state.json` baseline B to the candidate. AI output is treated as user-edited state U.
Later static updates follow the existing B/U/N rules, preserve the AI-derived implementation, and present N as `.dist` when required.
The existing U=N same-bytes adoption rule remains unchanged. If AI output already equals the static candidate, no special overwrite-prohibition state is created.

## 8. Boundedness and the limits of trust

Preserve ADR-0032's limits for SDL, modules, JSONL, bundles, types, and management metadata, as well as the existing VM, host, and GraphQL budgets.
Inference context, JSON Schema, JSONL events, final JSON, diffs, saved originals, and candidates also have per-item and aggregate byte, item-count, and depth limits.
Apply budgets while rendering, reading, and parsing; do not wait until an expanded DOM or unbounded stdout buffer already exceeds the limit.
WP-AI0 measures and records concrete transport and proposal budgets against maximum existing inputs, then fixes them as shared implementation constants.
The AI budget must neither silently narrow an existing acceptance range nor expand an execution limit.

The correction loop and all batches share the 600-second automatic-processing deadline. Stopping and collecting work after that deadline has a separate finite cleanup budget.
A kill request alone is not treated as collected. Human review is excluded from this timekeeping. A large number of targets are handled by split execution.
VM's 500ms/16MiB/pcall16 stages, process-global admission, sticky failure, and mutation rollback remain unchanged.
When inspecting AI candidates, do not call resolve itself without permission, and separate normal type/module/seed inspection and actual HTTP behavior inspection.

AI output and commands embedded in SDL descriptions cannot change target, authority, inspection, or adoption rules.
On the other hand, Lab01 does not prove the business meaning of the candidate or the harmlessness of the user's entire Codex environment.
The operational responsibility remains to pass a development context that does not contain secrets and check the diff of the generated code.

## 9. Implementation order and achievement gate

### 9.1 Work packages

1. WP-AI0: Confirm the actual baseline, ADR-0032 review state, and active ownership and inspection components. Fix the protocol, budgets, actual CLI capabilities, and planned trace for every AI ID.
2. WP-AI1: Implement the common traits, fake backend, request and context assembly, candidate inspection, and correction loop; complete focused verification without Codex.
3. WP-AI2: Implement the Codex process adapter. Use a fake executable to verify I/O, terminal outcomes, cancellation, deadlines, collection, and confidentiality.
4. WP-AI3: Implement proposal storage, partial inspection, snapshot matching, integration with the existing transaction, B protection, and recovery.
5. WP-AI4: Connect the TUI and non-TTY boundaries of `infer` and `apply`, diff approval, PTY tests, and real CLI and HTTP checks.
6. WP-AI5: Complete real Codex acceptance in two domains, common validation, independent review, correction and re-review, operating and recovery instructions, measurements, and handoff.

### 9.2 Closed acceptance inventory

The initial state of every ID is Required / Not run. Do not reuse prior MOCK/RSM/MR/MP Passed evidence for this trace.

| ID | Required validation |
| --- | --- |
| AI-01 | infer/apply/help/version, explicit codex, required/exclusive/duplicate/unknown option. Execution of unsupported agent/no fallback |
| AI-02 | Selection of active ready, manual, or edited resolvers and non-empty seeds; rejection of out-of-scope, missing, glob, or duplicate paths; no unauthorized target expansion |
| AI-03 | Separation of all active contexts and writing targets, optional instructions/default requests, division of related contexts, refusal to change boundaries by input text commands |
| AI-04 | Supply types/permissions/constraints from real IR/API/reference. No Rust branches with unique domain names or handwritten second API definitions |
| AI-05 | Fake-backend substitution through the common trait, isolation of Codex details, adapter independence from GraphQL and `.dist`, and no new dependency in the regular product |
| AI-06 | No shell argv/stdin, dedicated cwd/read-only/non-interactive, normal authentication, explicit diagnosis of CLI missing/non-conforming/authentication failure |
| AI-07 | Separate event JSON from candidate JSON and require the final message, terminal event, and exit status to agree. Reject success based only on exit status 0, intermediate output, or a missing or failed terminal event. |
| AI-08 | stdin/stdout/stderr conflict, bulk output/inherited pipe, single deadline/cancellation/stop/recall. Secret/raw stderr non-exposing |
| AI-09 | JSON duplicate/unknown/depth/size/target ID/missing rejection. Up to 2 corrections, total batch 600 seconds, no environmental failures, no infinite retry |
| AI-10 | PTY test for TUI search/arrow/Space/all unselected/hidden selection confirmation/instructions/start approval/progress/cancellation/device restoration |
| AI-11 | Per-file diff and acceptance plus final approval; no installation without approval; harmless rendering of control characters; no changes when every candidate is rejected |
| AI-12 | Non-TTY explicit target/output, candidate storage only. apply requires TTY, can be reviewed later without Codex |
| AI-13 | Proposal closed format/original/candidate/digest/identity, modification/missing/intermediate writing/output collision/path unauthorized rejection |
| AI-14 | Retain completed candidates after interruption or partial failure; support later adoption and prevent duplicate application without treating incomplete work as success or repeating inference |
| AI-15 | Separation of configuration, SDL, and ownership preflight from overall readiness; inference remains available to improve unimplemented or nonconforming files and invalid seeds |
| AI-16 | Candidate independent/module ready/initialization check. Do not bypass Rust boundaries due to cast or capture VM failure |
| AI-17 | Inspect JSONL stored values, IDs, numeric precision, registration order, and relation constraints against retained seeds in the connected model set; distinguish unrelated nonconformance |
| AI-18 | Reject a set whose accepted and rejected combination is inconsistent even when files pass individually; report partial application and overall ready/nonconforming/not-evaluated state accurately |
| AI-19 | Editing during inference/after confirmation/just before deployment, schema/API/generation drift, rechecking with latest dependencies, no long exclusive locks |
| AI-20 | One transaction for all adopted files; handle each move, post-stage inspection, cancellation, and invalid backup; continue remaining restoration; distinguish post-commit cleanup and retain recovery material |
| AI-21 | AI adoption does not advance B; subsequent static updates preserve edits; `.dist` and proposals remain distinct; iterative and independent edits remain protected |
| AI-22 | Unselected file/config/SDL/generated/dist/orphan unchanged, proposal not executed/not exported. Existing check/serve overall check unchanged |
| AI-23 | Per-item and aggregate budgets for new input, output, storage, and rendering; retain existing SDL/VM/store limits and regular 1.85, Mock 1.95, stable, and ABI 6 contracts |
| AI-24 | Confirm second domain/rename, Query/Mutation/Search/seed/relations with independent HTTP assertion with fake backend |
| AI-25 | Measure real Codex inference in two domains: create an initial manual implementation and improve an existing implementation from instructions through selection → candidate → approval → HTTP verification |
| AI-26 | After adoption, check/serve/export is possible without Codex/network. No implicit inference from init/update/reference etc. |
| AI-27 | Common validation; existing VM stop, rollback, return, and recovery behavior in debug and release; all-SDK exactness and quality; 400-line and English-prose rules; both diff inspections |
| AI-28 | Review A/B and re-review after modification, all ID assertion traces, measurement/untracked/unexecuted environments/use/cancellation/recovery procedures |

Gate AI-A covers AI-01 through AI-05, AI-B covers AI-06 through AI-09, AI-C covers AI-10 through AI-14, AI-D covers AI-15 through AI-23, and AI-E covers AI-24 through AI-28.
Review A independently confirms IR/reference/candidate/partial verification/GraphQL semantics, and Review B independently confirms process/object ownership/TUI/save/simultaneous editing/recovery.
Do not accept a subagent's self-report as evidence. Correct every reasonable severity finding and obtain re-review.

Normal CI and common validation use deterministic tests with a fake backend and executable; they do not require Codex installation, authentication, network access, or billing.
AI-25 is a separate real-backend acceptance that the user explicitly authorizes. If it is not run, record that gate as unverified and distinguish it from successful local product validation. A fixed mock response or alternate backend must not be presented as successful real inference.
Record independent behavioral assertions for the adopted code and seeds, together with any remaining manual corrections, rather than requiring byte-for-byte model output.

## 10. Handover to subsequent task

During implementation, add this program to the architecture, handoff, and conformance documents and synchronize the CLI README and usage instructions.
This decision change adds only the ADR and document index; it does not alter existing Implemented or Passed records or active sources.
Provide the next agent with the complete implementation goal prompt.
This ADR is normative. Its existence alone does not mean implementation has started or an acceptance gate has passed.

Subsequent validation records are collected in `docs/agent-assisted-mock-inference-validation.md`.
Record tests, individual assertions, and execution results for every AI ID; the actual CLI version and available model information; generation, revision, and adoption results in two domains; normal build, rebuild, and binary-size measurements; and target count, context bytes, inference duration, retry count, and cancellation recovery.
Mark unavailable usage or model names and unexecuted operating systems, remote CI, or real inference explicitly rather than guessing. Do not copy logs containing secrets into the validation record.
Run focused verification first and consolidate full verification in the final source. Do not shorten the cycle by omitting existing shared checks.
Live application sources are out of scope. Do not stage, commit, push, publish, or tag without explicit instructions.
