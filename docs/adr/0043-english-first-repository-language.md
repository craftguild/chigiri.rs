# ADR-0043: Use English for First-Party Repository Content

Status: Accepted
Date: 2026-09-17
Implementation: Implemented

## Context

Pythia was developed as an internal tool, and its documentation, source comments,
tests, diagnostics, examples, scripts, and generated text contain Japanese. The
project is now being prepared for public open-source use. Requiring readers and
contributors to understand Japanese would make review, maintenance, and adoption
unnecessarily difficult.

The repository also contains tests for Unicode handling and historical design
records. Removing Japanese must not weaken those tests or rewrite the technical
decisions recorded by the project. Third-party sources and license texts must
remain faithful to their upstream forms.

## Decision

English is the repository language for all first-party content. This includes:

- documentation and contributor instructions;
- source comments and API documentation;
- test names, fixtures, assertions, and failure messages;
- command output, diagnostics, help text, examples, scripts, and configuration
  metadata;
- prose emitted by generators and first-party generated artifacts.

Stable protocol tokens, API names, file formats, and identifiers keep their
defined spelling. This decision does not restrict runtime data: Pythia continues
to accept and process Unicode input. Tests that need non-ASCII or display-width
coverage use language-independent fixtures such as combining Latin characters,
emoji, and symbols when that preserves the tested property.

The migration excludes `.git`, build output under `target`, vendored dependencies
under `vendor`, and verbatim third-party license text. First-party wrappers,
notices, inventories, and generated files that embed project-owned prose remain
in scope.

Historical documents are translated without changing their status, evidence, or
recorded decisions. Generated prose is changed at its owning source and then
regenerated or checked for exact consistency; generated files are not treated as
independent sources of policy.

The implementation-rules checker scans first-party text for Hiragana, Katakana,
CJK ideographs, and Japanese or full-width punctuation used in prose. It also
decodes source-level Unicode escape spellings so repository-owned generators
cannot reintroduce Japanese prose while remaining ASCII on disk. The scanner's
own pattern definitions and rejection fixtures are the only intrinsic escape
exception. A deliberate future exception requires an explicit repository policy
change rather than an untracked allowlist entry.

### 2026-09-24 amendment: Product name-origin notation

Permit one literal U+5951 ideograph solely as the Japanese origin of the name
Chigiri in the repository-root `README.md` introduction. The approved source
line is exactly `The name comes from the Japanese ` followed by that single
ideograph, with no other content on the line. It may occur once, before the
first `## ` section heading, accompanied by an English explanation and romanized
reading. This is a quoted name-origin token, not permission for Japanese prose.

The scanner masks only that literal token on the exact approved line. It still
checks all other content and paths, including other Japanese on that line,
duplicate occurrences, the same token in another context/file, Japanese
punctuation, and source-level Unicode escapes. An escaped spelling is not an
alternative way to invoke this exception. Do not exclude the whole README,
allow the code point globally, or add a general documentation allowlist.

Focused scanner tests cover the approved literal and these rejection cases.
The rest of the English-first policy is unchanged.

### Supersession

This decision supersedes the repository-language requirements in earlier ADRs,
work packages, review records, and implementation prompts. References that record
the former Japanese-language rule remain historical context; current contributor
instructions and review gates require English.

## Consequences

Public documentation and implementation context become accessible to a wider
contributor base, and new Japanese prose is rejected before handoff. The narrow
name-origin exception preserves the product's etymology in English prose. The
initial migration touches many files and can invalidate snapshots or generated
manifests, so formatting, generation checks, and relevant tests must run after
translation.

Some Unicode tests need new fixtures. Their assertions must continue to cover the
same byte, scalar, grapheme, and terminal-cell behavior; translating a fixture is
not sufficient when it would remove the property under test.

## Validation

The migration is complete when the repository-language scan reports no disallowed
Japanese text in its defined scope, generated-artifact checks pass, source
formats remain valid, and the workspace's relevant test suites pass. The ADR
implementation state changes to `Implemented` only after those checks succeed.
