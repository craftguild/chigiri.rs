# Review tracking with Beads

Status: Normative

Effective: 2026-10-21

Beads is the authority for new repository review rounds, findings, correction
evidence, and independent re-review decisions. This replaces the deleted
Markdown review ledger (`docs/review-ledger.md`), including contrary
instructions in earlier ADRs, goal prompts, and retained validation plans. This document defines the workflow; it
is not another review sheet or finding/status index.

## Review rounds and findings

Create a Beads task for a review round and label it `review`. Record the scope,
governing contracts, reviewer and perspective, date, source identity, dirty/index
state, checks actually performed, unexecuted scope, and decision. A no-findings
review still records those details. Link the implementation task when relevant.

Create a child issue for each actionable finding. Use the native Beads priority
and an appropriate issue type, normally `bug` for a defect. Record:

- the contract and affected locations;
- reproduction steps and observed evidence, or explicit static reasoning;
- impact, correction direction, and independently checkable closure conditions;
- source identity, commands, outcomes, and limits of the evidence; and
- any existing finding ID, such as `R53-03` or `REV-001`.

The Beads issue ID is the new stable identifier; do not invent a second sequence
for new findings. Preserve legacy IDs in the title or description and link the
historical record. Reopen the same issue if a correction fails its original
requirement. Do not delete or replace failed runs or superseded review decisions.

P0 means urgent critical failure; P1 means a major safety or core-function issue;
P2 means an ordinary bug or contract inconsistency; P3 means a minor improvement.
Preserve historical severity scales when citing old records. Clearly distinguish
observed failures, established static reasoning, and unconfirmed hypotheses.

## Correction and independent verification

Use native Beads `open`/`in_progress`/`closed` states; no custom database status
configuration is required. Review milestones are recorded as follows:

1. Claim the issue before correction and keep it open or in progress.
2. Append a `Fixed` correction comment containing the source identity, changed
   behavior, focused validation, and residual limits. Add the `review:fixed`
   label. The implementer does not close the finding or claim verification.
3. An independent reviewer appends a `Verified` re-review comment with the exact
   reviewed source, reasoning or reproduced checks, outcomes, and limits. Remove
   `review:fixed`, add `review:verified`, and close the finding only after its
   closure conditions have been independently confirmed.
4. If the correction is insufficient, keep or reopen the same issue, remove the
   stale milestone label, and append the remaining evidence. Do not erase the
   unsuccessful correction.

Append evidence through Beads comments rather than repeatedly replacing the
original report. An implementer's pass report is review input, not independent
verification. A focused review is not a full validator or platform qualification.
Do not reuse another source snapshot's acceptance artifacts as proof of edits.

An accepting review round closes only when its decision is recorded and its
required finding issues are independently verified and closed. Record completed
inspection separately from approval: a round requesting changes remains open
while required findings await verification. Deferral requires an explicit
decision, owner, rationale, and revisit condition; use Beads deferral/dependency
mechanisms without treating deferral as verification or forcing a parent closed
over unresolved children. A scope decision may explicitly exclude deferred
follow-up from an acceptance gate, but must not silently change that gate.
Do not weaken a contract to close a finding without an explicit design decision.

## Boundary with automated tests

Review records document human decisions, findings, corrections, and verification;
they are not test oracles. Tests, acceptance checkers, and builds must run without
Beads, its database, or the former ledger. They must not read review comments,
issue states, milestone labels, closure reasons, or approval text to derive
expected results or decide whether current behavior passes.

Automated assertions come from the applicable normative specifications, ADRs,
and explicit version-controlled test fixtures. Reviewers consume test evidence;
tests do not consume reviewers' verdicts. Independent review may remain a
separate human acceptance or release gate, but is not a substitute for executing
tests or an input to their results.

The frozen tables in `scripts/fixtures/historical-acceptance-traces/` are
test-owned compatibility baselines, not a replacement review ledger. Their
historical provenance does not make past pass statements evidence for the
current source. Trace checks may verify ID inventories and explicitly approved
evidence migrations, but must respect requirements retired or superseded by
later ADRs rather than demand unconditional equality with historical tables.
Do not query Beads or regenerate fixtures from review records during tests.
Change test expectations through an explicit contract or fixture change, not
solely to match a review's status or to close a finding.

## Evidence and historical records

Validation documents and source manifests may retain commands and supporting
artifacts. Link them from Beads; do not maintain parallel finding/status tables
or create per-ADR or per-phase Markdown review sheets. Keep first-party review
prose in English and do not expose credentials or unredacted internal errors.

Every record of the former ledger was imported into Beads under the closed
epic `lab01-c43`, and the file was then deleted; Git history retains it. Each
review program is a child task whose sections are verbatim comments under
descriptive headings. Every historical finding is a child bug of its review:
legacy IDs are kept as `external_ref` `legacy:<ID>`, and unnumbered findings use
their Beads ID. Search there for precedent before new work. Imported statuses
are historical, not new verification, and old qualification gates are not
reopened. The outstanding `REV-001`
finding continues as `lab01-c30`, and unexecuted Gate P-D/P-E qualification is
the deferred issue `lab01-0g1`. When historical work is resumed, reuse or link
the imported issue for that record.

Review changed prose, public API contracts, errors, limits, lifetimes,
concurrency, and GraphQL invariants. Check the English policy, shared implementation Code
limits, and generated ownership. Automated checks do not replace review of the
diff and contract. Follow the repository's conservative profile: no commit,
push, or Dolt remote synchronization without explicit authority.
