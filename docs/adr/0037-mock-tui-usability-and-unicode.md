# ADR-0037: Improve Mock TUI layout, Unicode editing, and unified diffs

- Status: Accepted
- Implementation: Implemented (local implementation is completed / Designer review Passed)
- Date: 2026-09-09
- Amends: the interactive UI contracts in [ADR-0033](0033-agent-assisted-mock-inference.md) and [ADR-0035](0035-cli-tui-and-embedded-graphiql.md)
- Related: [ADR-0034](0034-generated-luau-formatting.md), [ADR-0036](0036-mock-luau-hooks-and-json-logging.md)

## 1. Background

ADR-0035 moved interactive CLI rendering to ratatui, but the resulting screens
still primarily passed lists of strings to `Paragraph`. The improvement over
the old UI was difficult to recognize. Non-ASCII escaping made additional
instructions difficult to read and edit in languages such as Japanese. Diffs
also emphasized full listings of original and candidate lines instead of the
changed regions, viewing position, and adoption state.

This ADR improves the everyday workflow of selecting targets, writing
instructions, reading changes, and approving them. Its UI acceptance criteria
do not retroactively extend the existing Passed results for ADR-0034 or
ADR-0035.

## 2. Scope and preserved boundaries

The scope is the TTY display and interactive input for `pythia-mock infer`,
`apply`, and `reference`. Screen state, input, and rendering are centralized in
the shared `cli/ui` layer; inference adapters and project transactions do not
contain terminal processing. Ratatui widgets such as `Block`, `List`, and
`Paragraph` give lists, input fields, diffs, and fixed footers separate display
responsibilities. The implementation neither returns to hand-built whole-screen
ANSI strings nor writes arbitrary source escape sequences to the terminal.

The following contracts remain unchanged:

- inference target selection, pre-submission confirmation, the initially
  unselected state, candidate validation, and final explicit approval;
- validation of formatted candidate bytes followed by placement of those same
  bytes, with no reformatting or restamping during review;
- original and candidate identity, concurrent-edit detection, B/U/N and
  `.dist`, cancellation, rollback, and ownership of recovery material;
- non-TTY CLI behavior, machine-readable stdout, reference JSON, and
  configuration/proposal storage formats; and
- GraphQL ABI 6, Rust 1.85 for the regular product and generated SDKs, Rust
  1.95 for Mock, and the 400-line limit for handwritten files.

Because the product has not been released, the old interactive UI is not kept
as a compatibility mode. The migration does not rewrite user source or
configuration. New theme settings, public CLI options, external pager/editor/
diff commands, side-by-side diffs, syntax highlighting, change folding, mouse
support, and additional GraphiQL or browser tests are out of scope.

## 3. Screen layout

### 3.1. Shared structure

Each screen has a title, body, status area, and fixed footer. Body sections use
borders and headings appropriate to their responsibility. The title identifies
the command and stage; status shows selection counts, viewing position, and
execution state; and the footer lists keys valid on the current screen. Cursor,
focus, selection, and validation failure are distinguished by symbols and text,
not by color alone.

The existing minimum of 32 columns by 10 rows and maximum render area of 200
columns by 60 rows remain in force. Narrow screens switch to vertically stacked
content and shorter labels. Decoration may be omitted, but target identity,
approval conditions, cancellation controls, and source text cannot be treated
as verified while hidden. Below the minimum size, the UI requests a resize and
accepts no new approval. Edits, selections, and viewing records survive resize.

### 3.2. Target selection and candidate lists

The list displays checkboxes, a highlighted cursor row, target category/name,
and validation state. During search it shows both the total selection count and
the number of selected items hidden by the filter. Filtering never implicitly
adds or removes selections. Invalid candidates expose their reason but cannot
be adopted, and remain visually distinct from merely unselected candidates.

```text
┌ infer · Target selection ──────────────────────────────┐
│ Search: user       Selected 2 / 12 · Hidden selected 1 │
├ Targets ───────────────────────────────────────────────┤
│ > [x] Query.getUsers                            │
│   [ ] Mutation.createUser                       │
│   [ ] seed.users                                │
├ Additional instructions ───────────────────────────────┤
│ Create a user with a localized display name.           │
└────────────────────────────────────────────────┘
 j/k Move  Space Toggle  / Search  i Edit explanation  Enter Next
```

Candidate lists explicitly label items as adoption candidates, not inference
targets, and display their validation results. This screen alone can never make
an unreviewed candidate adoptable.

### 3.3. Editing additional instructions

The bordered editor supports multiple lines and scrolling and displays a real
cursor at the insertion point. Enter inserts a newline in instructions, Ctrl-D
finishes editing, and Enter completes the single-line search field. One editing
model handles paste, cursor movement, insertion, deletion, wrapping, and
cursor-following scroll.

```text
┌ infer · Edit additional instructions ──────────────────┐
│ Create a user with a localized display name.           │
│ Assign at least one author to each post.▏               │
│                                                │
└────────────────────────────────────────────────┘
 2 lines · UTF-8  /  Enter inserts a newline.  Ctrl-D submits.  Ctrl-C cancels.
```

No separate text is introduced between the edit buffer and submitted content.
Escaping and wrapping for display do not alter stored or transmitted bytes.
Pre-submission confirmation shows the same instructions, including non-ASCII
input, in readable form.

### 3.4. Diff and reference views

The diff uses a single-column unified format with original and candidate names,
old and new line numbers, `-`, `+`, or space prefixes, and viewing/adoption
state. Additions are green, deletions red, and unchanged text uses the normal
color unless `NO_COLOR` is non-empty. Prefixes, headings, and state labels must
support the same decisions without color.

```text
┌ apply · Query.getUsers · Original → Formatted candidate ─┐
│ Old New  Content                                         │
│  1   1   return function(ctx, args)             │
│  2     -     return {}                         │
│      2 +     return ctx.models.User.find({})    │
│  3   3   end                                   │
├ Viewed: complete · Validation: passed · Not approved ───┤
│ - removed  + added  space unchanged                     │
└────────────────────────────────────────────────┘
 j/k Move  PgUp/PgDn Page  g/G First/Last  a Approve
```

The diagram is illustrative and does not make its resolver body or terminal
width part of the fixture contract. The reference viewer shares line
rendering, position tracking, a fixed footer, the same navigation keys, and
existing `/` plus `n`/`N` search. Non-TTY text and JSON gain neither borders
nor color, and the external pager is not restored.

### 3.5. Key bindings

| Screen/state | Key | Action |
| --- | --- | --- |
| List | `j/k`, ↓/↑ | Move one item. |
| Diff/reference/view body | `j/k`, ↓/↑ | Scroll one displayed line. |
| List/view | PgDown/PgUp, Home/End, `g/G` | Move one screen or jump to the start/end. |
| View body | Space | Move forward one screen. |
| Target selection | Space, `/`, `i`, Enter | Toggle, search, edit instructions, or proceed to pre-submission confirmation. |
| Candidate list | Enter, Space | Open a diff; never approve an unseen candidate. |
| diff | `a` | Add only a validated, fully viewed candidate to the adoption set. |
| Candidate review/final confirmation | `c`, then explicit `y` confirmation | Approve only after full review. |
| Reference | `/`, `n`/`N` | Search next/previous match. |
| Description editor | Enter, Ctrl-D | Insert a newline; finish editing. |
| Search editor | Enter | Confirm the search. |
| Shared editor | ←/→/↑/↓, Home/End, Backspace/Delete | Move the cursor, move to the displayed line boundary, or delete the preceding/following grapheme. |
| Shared editor behavior | `j/k/g/G`, Space, and ordinary Unicode characters | Insert them as text; do not interpret them as viewer commands. |
| Every screen | Ctrl-C | Use the existing cancellation and worker-recovery path. |

Existing `q`/Esc return and cancellation behavior remains intact; those keys
are never assigned to application or affirmative responses. Lists, viewers,
and editors have explicit modes rather than hidden Vim-like input modes. The
footer shows only context-appropriate keys, and submission, candidate adoption,
and final application remain separate stages.

## 4. Unicode and safe display

The new display replaces blanket `\u{...}` escaping of ordinary non-ASCII
input. It distinguishes UTF-8 byte offsets, grapheme-cluster boundaries, and
terminal display-cell widths. Cursor movement and deletion operate on
graphemes, wrapping and horizontal position follow display width, and input
limits and transmission apply to the original UTF-8 bytes. Vertical movement
uses wrapped display lines and the desired display column; the cursor never
lands inside a combining sequence or emoji. Existing dependency policy may
permit reuse of a Unicode library, but no Mock dependency enters regular
product crates.

The terminal's IME supplies committed UTF-8. Pythia does not implement an IME
or display uncommitted composition. It performs no NFC/NFD normalization and
preserves the representation entered by the user. If a byte limit would be
exceeded, no incomplete UTF-8 or grapheme fragment is inserted; the existing
buffer remains intact and the UI explains the refusal.

Control characters, ESC/OSC sequences, bidi controls, and invisible characters
that could spoof the display use safe visible representations. Natural
combining characters and emoji are not destroyed by escaping every zero-width
character. A representation derived from a dangerous character remains
distinguishable from an identical literal escape typed by the user. Display
tokens retain their source byte ranges, and color, borders, and safety
annotations are never written back into source.

The input parser retains its existing ESC state, bracketed-paste handling, and
pending-input defenses at frame boundaries. Newlines or `a`, `y`, and `q` in a
paste are never replayed as commands. Pasted or residual input cannot approve
content after a screen transition.

## 5. Diff computation and complete review

### 5.1. Displayed content

A deterministic line-oriented diff identifies additions, deletions, and
unchanged lines and shares equal lines so local changes are visible. It shows
all context; omitted hunks or folds never count as complete review. Tabs,
trailing whitespace, LF/CRLF endings, and a missing final newline are visible,
including when they are the only differences. Annotations are visually
separate from source and cannot be mistaken for source text.

Diffing bounds source size, search work, index count, and output capacity; it
never allocates an unbounded Cartesian-product matrix. The implementation fixes
the search limit in constants and tests. Normal input uses the unified view. If
the search budget is exhausted, the UI may fall back to a visible full-content
"delete all, then add all" representation. The fallback announces itself,
never truncates content while permitting adoption, and cannot block rendering
or cancellation indefinitely.

### 5.2. Review coverage

Approval requires complete display of the original and formatted candidate,
not a particular diff line count or final scroll position. Display rows and
tokens map back to both sources, and only byte ranges actually rendered on
screen become covered. Equal rows cover both sources, deletion rows cover the
original, and addition rows cover the candidate. Wrapped long lines, visible
representations of invisible characters, and line-ending annotations create no
coverage gaps.

Jumping to the end with `G` or End does not cover skipped content. Returning,
re-entering, or resizing also leaves unseen ranges uncovered. Content hidden by
the fixed footer or clipping does not count, nor does merely displaying a color
or state label. If source identity changes, old coverage cannot approve the new
bytes. Human reading speed has no timeout; the UI distinguishes content having
been rendered from a person having understood it. Complete display never
adopts automatically: `a` selects a candidate, and a later explicit approval
is still required.

## 6. Progress, resources, and non-TTY behavior

The inference progress screen reserves fixed regions for batch/attempt,
elapsed time, current stage, and cancellation guidance. When the transient
communication view from [ADR-0036 Section 7.2](0036-mock-luau-hooks-and-json-logging.md#72-temporary-display-of-infer-communication)
is connected, its bounded history appears in the body without displacing the
status or footer, and following the tail remains distinct from reviewing older
messages. This ADR neither implements communication-event collection or hooks
nor makes ADR-0036 a prerequisite. If ADR-0036 is implemented first, its
existing communication display remains and provider-specific protocol stays
out of the UI. Reviewing communication history never replaces complete
candidate approval.

Existing limits remain unchanged.
Instructions remain limited to 64 KiB, search to 1,024 bytes, `DIFF_BYTES` to
4 MiB, `SCREEN_BYTES` to 128 KiB, rendering to 200 columns by 60 rows,
`REVIEW_PAGE_OFFSETS` to 256 × 1,024, and terminal I/O to a 250 ms deadline.
Unicode indexes, diffing, and review coverage also have finite budgets; reaching
a limit produces an explicit error or the complete-content fallback described
above. Resource pressure never removes an approval condition.

Slow or disconnected TTYs, panics, errors, and signals retain existing
terminal restoration, bounded I/O, and worker cancellation/recovery. Human
input wait time is distinct from machine I/O deadlines, and waiting for the UI
never stops backend pipe draining. Non-TTY execution never starts interactive
screens and retains plain text/JSON and stdout/stderr separation. `NO_COLOR`
only disables TTY color; it changes neither validation nor selection nor the
amount of information.

### 6.1. Implemented display/editing budgets and large paste handling

The common screen fixes title, status, and footer rows and gives the remaining
rows to the body. Layout reserves the bottom-right cell within the 200-by-60
maximum and guarantees at least two body rows at 32-by-10.
The three-line instruction preview appears only on terminals with at least 16
rows. If `NO_COLOR` is non-empty, no foreground or background color is added,
but selection marks and bold/underline remain. Ratatui display state is cleared
only initially and after resize; ordinary editing renders cell differences.

Mock directly pins `unicode-segmentation = 1.13.3`, already resolved through
ratatui, and combines it with the existing `unicode-width = 0.2.0`. No new
dependency enters regular crates. A display token contains the source UTF-8
range, either a natural grapheme or a reversible dangerous-character notation,
and its cell width. Japanese text, base-plus-combining sequences, and family or
flag emoji remain intact, while control, bidi, and invisible characters remain
distinguishable from literal backslashes. Heading labels are limited to 4 KiB
and never substitute for complete review. The text viewport is limited to
64 KiB; the rest of `SCREEN_BYTES` is reserved for fixed regions and terminal
control sequences. Unrendered ranges do not become covered. Communication text
that is already safely represented is not double-escaped, and wrapping occurs
at grapheme or escape-unit boundaries. Non-TTY line splitting also preserves
complete UTF-8; a display unit larger than the line budget counts as display
loss.

The editor's grapheme-boundary index and rendered line count are each limited
to 65,537 entries, and the total rendered string is limited to ten times the
input maximum.
Indexes are rebuilt only when the value or width changes. Cursor and display
notations are never stored in or transmitted with the text.
Bracketed paste is collected in a bounded 64 KiB temporary buffer and inserted
atomically when the closing marker arrives.
Overflow, invalid UTF-8, or a newline pasted into a single-line search rejects
the entire paste and preserves the existing buffer. For scalar input, at most
one pre-grapheme buffer/cursor state is retained so an over-limit grapheme can
be rolled back atomically. Grapheme assembly is capped at 64 KiB plus four
bytes; after that, the editor explains why input is paused until movement or
deletion. Ordinary character input contained in one read of at most 512 bytes
may be processed in one frame, but batching never crosses an operation key or
screen boundary.

On 2026-09-10, a user report that copying and pasting 15,000 characters took
about ten minutes was added to the regression scope for UX37-03, UX37-07,
UX37-10, and UX37-22. Real-PTY tests enter 15,000 predominantly ASCII and
Japanese characters with and without bracketed paste. From the start of input
to rendering the end, the test target is under five seconds, with less than
32 KiB of terminal output for bracketed paste and less than 512 KiB for normal
character transfer. Measured values and conditions are recorded in the
integrated verification record and are not an absolute guarantee for every
user terminal.
Tests compare exact pre-send text and the prompt/schema hashes received by the
fake backend.
Idle 25 ms input waits are not applied during transfer; readable input and
write retries use a 1 ms signal wait. When writes progress, they continue
immediately without relaxing the existing 250 ms output deadline,
cancellation, or restoration.

### 6.2. Implemented diff search, index, and coverage budgets

Borrow the original and candidate text, including line endings, and run bounded
LCS alignment only over the range remaining after their common prefix and
suffix. A hash is only a prefilter; equality always compares the original bytes.
The matrix is limited to 1,000,000 `u32` entries, the comparison count to
2,000,000, and actual byte comparisons to 32 MiB.
Budget is checked before allocation or comparison. If no bounded diff fits,
render a full-deletion/full-addition fallback.
Common context is never folded, and the fallback notice itself is part of the
metadata that must be completely reviewed.

The original and candidate logical-line indexes contain at most 262,144 entries
in total. Source range, trailing-space position, and hash use 24-byte entries,
up to 6 MiB; alignment results use at most 262,144 12-byte entries, up to
3 MiB. Trailing-space position is computed once per source line. Wrap positions
contain a logical item and source offset as two `u32` values, with at most
262,144 entries and the existing 2 MiB limit. Only position indexes and the
needed viewport are built; the complete rendered string is not retained.
Original plus candidate plus safe-display metadata, and wrapped display content
plus separators, each remain within the 4 MiB `DIFF_BYTES` limit. A line or
viewport remains within 64 KiB. Overflow is an explicit error and never permits
adoption of unrendered content.

Review records use independent bit sets for original and candidate source
bytes, safe-display metadata bytes, and each diff line's ending annotation.
Source plus metadata is limited to 4 MiB, so each source bit set is at most
512 KiB plus rounding; annotations are limited to 32 KiB.
Source byte accounting distinguishes LF and CRLF, and an end-of-line annotation
is shown only when its complete annotation fits.
Literal backslashes from source are doubled in the display so they cannot be
confused with annotations.
Render tabs as `\t`, trailing spaces as `\x20`, and distinguish line endings as
`\ [LF]`, `\ [CRLF]`, or `\ [NO LF]`.
Rows hidden by the fixed footer or viewport capacity do not count. Coverage is
recorded only after a same-size frame has been written successfully.
A page counts as reviewed only after every visible row is displayed completely;
review may finish on a page other than the final page once all ranges are
covered.
When all gaps become covered, pending adoption input is discarded; only a new
`a` entered after the completion notice is accepted. Navigation and returning
do not become approval. Re-entry creates a new review record, and existing
proposal/placement identity checks reject concurrent edits or candidate
changes.

## 7. Implementation and convergence policies

1. Preserve existing UI, approval, cancellation, and I/O budgets while splitting
   layout, editor, and diff responsibilities into smaller modules.
2. Move the list/fixed footer/progress/reference to a common layout and check the location in TestBackend.
3. Implement a Unicode editor with safe display/source separation and verify
   real non-ASCII PTY input.
4. Connect bounded unified diffs to navigation, color, coverage tracking,
   large jumps, and long-line negative cases.
5. Test the complete infer/apply flow with a fake backend, then run final-source
   validation and reviews.

Use focused checks during implementation and reserve heavy common validation for
the final source. Keep comments and rustdoc in English, enforce the 400-line
handwritten limit, and never satisfy it through minification or hand-editing
generated products.
Normal formats remain unchanged, and the new UI adds no old-parser
compatibility branch, alternate interchange format, or publisher gate. It
requires no external applications change, real AI communication, authentication change,
SSH access, publication, or tag.

## 8. Acceptance

On 2026-09-10, the integrated source completed the common 85-command suite,
measurements, independent Reviews A and B, fixes, and rereview. Individual ID
and source evidence is recorded in the
integrated verification record.
The same day's
designer review
also Passed with zero findings at every severity. Human UI/IME testing and remote
CI remain unconfirmed.

### 8.1. Decision

The following inventory was Required and entirely Not run when the design was
added. It remains separate from the previous AI, FMT, DX35, and HK decisions;
their Passed results do not approve this new UI.

### 8.2. Closed acceptance inventory

| ID | Required verification |
| --- | --- |
| UX37-01 | Test target/candidate lists, checkboxes, cursor, status, and fixed footer with buffer/style assertions. |
| UX37-02 | Preserve search and hidden-selection counts, invalid-candidate non-adoption, the initially unselected state, and pre-submission confirmation. |
| UX37-03 | Verify the multiline-instruction/single-line-search distinction, real cursor, insertion, and cursor-following scroll. |
| UX37-04 | Test minimum, normal, maximum, and resized layouts, state retention, and approval blocking on a narrow screen. |
| UX37-05 | Test `j/k`, arrows, page keys, `g/G`, Space, mode changes, and typed editor input. |
| UX37-06 | Verify shared reference layout, keys, and search while preserving non-TTY text and JSON. |
| UX37-07 | Verify that Unicode input, paste, pre-submission display, and transmitted bytes match without blanket escaping. |
| UX37-08 | Test combining characters, emoji, full-width and ASCII text, mixed navigation, Backspace/Delete, and wrapping. |
| UX37-09 | Test exact and near-limit input, malformed UTF-8 rejection, dangerous characters, and literal escapes. |
| UX37-10 | Test valid UTF-8, bracketed paste, residual input, and input retained across screens. |
| UX37-11 | Verify deterministic unified diffs and old/new line numbers for identical, added, deleted, replaced, and empty files. |
| UX37-12 | Test red/green styles, `NO_COLOR`, prefixes, and all examples; disabling color must not alter content or decisions. |
| UX37-13 | Verify complete rendering of differences in tabs, trailing spaces, LF/CRLF, missing final newlines, long lines, and Unicode text. |
| UX37-14 | Test that `G`, reverse navigation, resize, clipping, and return do not mark unseen ranges reviewed; reject incomplete review. |
| UX37-15 | Test original/candidate token matching, drift after source changes, and approval/install of the exact configured bytes. |
| UX37-16 | Test diff search and capacity limits, full-content fallback, non-approval when coverage is incomplete, and resource limits. |
| UX37-17 | Verify fixed progress status and cancellation guidance and, when the ADR-0036 communication view is present, coexistence with the bounded body. |
| UX37-18 | Exercise the complete infer/apply flow with a fake backend. |
| UX37-19 | Preserve existing PTY slow-write/disconnect, signal/error, terminal recovery, bounded I/O, worker recovery, and PTY-return behavior. |
| UX37-20 | Test non-TTY stdout/stderr integrity, disabled interaction, absence of real AI calls, and unchanged CLI, save-format, and recovery contracts. |
| UX37-21 | Run common validation, MSRV/stable, ABI 6, every SDK, and dependency/quality boundary checks. |
| UX37-22 | Enforce 400-line, fmt, and diff checks; map every ID to test/assertion/result evidence; record size, rendering, editing, and diff measurements. |
| UX37-23 | Perform independent UI/Unicode and security/resource reviews and record corrections and rereview. |
| UX37-24 | Provide a short fake-fixture manual procedure and screen data, clearly separating completed human verification from unverified items. |

### 8.3. Gates

| Gate | Scope | Local decision |
| --- | --- | --- |
| UX37-A | UX37-01–06: screen and interaction | Passed |
| UX37-B | UX37-07–10: Unicode editing and safe input | Passed |
| UX37-C | UX37-11–16: diff, color, coverage, and resources | Passed |
| UX37-D | UX37-17–20: integration, cancellation, and non-TTY behavior | Passed |
| UX37-E | UX37-21–24: shared quality, review, and handoff | Passed |

`TestBackend` verifies borders, layout, style, and cursor state; real PTY tests
verify the input parser and terminal restoration. Acceptance does not rely on
adding large snapshots: small fixtures cover multiple screen sizes and negative
assertions. No real AI or new browser infrastructure is used. If remote CI is
not run, the OS/job and unverified scope are reported explicitly.

### 8.4. Manual verification and handoff

The handoff includes a reproducible procedure using a fake backend and a small
proposal.

- Search the list and distinguish hidden selections from invalid candidates.
- Enter Unicode instructions with an IME and verify the same text after cursor
  movement, deletion, line breaks, and on the confirmation screen.
- Verify diff color and line numbers, `j/k` and `g/G` navigation, and rejection
  when intermediate content remains unseen.
- Verify `NO_COLOR`, narrow-terminal blocking, and terminal restoration after
  cancellation.

Passing tests is not equivalent to human approval of readability. Automated
gates, manual verification, and designer approval are recorded separately. A
user performing manual verification receives the procedure and screen data;
unverified work is never relabeled Passed.

## 9. Consequences

The product contract now requires natural reading of Unicode instructions and
changes and discoverable controls. Complexity remains localized in
display/source mapping, the Unicode editor, and bounded diffing. This separates
the ratatui migration itself from verification of user-visible improvements
and preservation of existing safety properties.
