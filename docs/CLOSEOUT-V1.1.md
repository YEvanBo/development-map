# V1.1 scope closeout — unreleased candidate

2026-11-17. Source review and integration evidence are not publication approval.

## Changes since the V1.1 independent review

- Check detects an effective union above 64 tuples before asking for approval:
  `BLOCKED / SCOPE_CAPACITY_EXCEEDED`, required/limit counts, no proposed union or
  approval question. Existing in-scope operations still work. No silent truncation.
- `build`, `format`, `lint` normalize to `develop` only for `implementation` scope.
- CLI usage lists `scope-check` and `scope-report`.
- A real Codex test exposed module-wide reporting of partial consent despite a
  restrictive note. The Skill now explicitly requires a finer resource label for
  partial approval; a module-wide tuple plus restrictive prose is insufficient.
  The failed run remains in the evidence rather than being counted as a pass.

## Follow-up review and native-session repairs

- A confirmed old task with no scope decision now registers only the unambiguous
  existing goal boundaries once; it does not ask for the same approval again.
  This is a Skill rule, not automatic Core inference. Ambiguous goals still ask.
  Both native hosts were tested with zero initial scope events, without prompt
  instructions naming Skill or tools; both registered scope and delivered.
- Resume now distinguishes the effective consent quote (`user_reply`) from the
  most recent decision quote (`last_decision_user_reply`). A failed regression
  and passing test preserve evidence that a refusal cannot be hidden by old consent.
- DSH's real Skill response truncated the former 12,963-character entry to 8,188
  characters. That run also missed persisting refusal; it remains a FAIL. The entry
  is now 6,930 characters (6,934 UTF-8 bytes), with initial intake and detailed
  delivery rules in two linked references. Real DSH output contains the complete
  entry including its final line; corrected refusal is persisted before checkpoint.
- Codex's early partial-approval run used an overly broad module label. The final
  runs use subfeature labels and retain the precise user quote/limits. Core compares
  declared labels only; it does not prove their natural-language meaning.

## Evidence levels

- Rust: 125 tests, including 21 scope tests. Python: 45. JavaScript: 4.
  All exit 0. Three P2 regressions failed before the fix (17/20), passed afterward.
- fmt and strict clippy still fail on documented baseline issues. Their logs and
  the prior baseline reproduction are retained; they are not represented as PASS.
- Skill frontmatter validator passed using an isolated test dependency directory.
- Unmodified archived V12 and V13 sources were built separately. Each old binary
  created its own database and confirmed a test goal through its old CLI. Upgrade
  to schema 14 preserved all preexisting table rows (except the intentional schema
  version addition), both checkpoints and all six events. The current binary
  recovered the old confirmed goal with no inferred scope grant. Automatic backup
  restoration was readable using the corresponding old binary. Integrity checks
  passed. These are old-binary-generated databases, not production user databases.
- Frozen V12 ZIP SHA-256 remains
  `7fc1759a3fe942a4e4130b85f3b0f61306c02af7df1c18a8aaa1fae5ccf99b9e`.

## Native host test boundary

The test launches installed Codex CLI 0.169.0-alpha.11.1 and DSH 0.2.2-rc.3 with
temporary workspaces, project Skill/entry rules and per-process MCP configuration.
The prompts do not name Skill or specify which tool to call. The project entry does
refer to Skill, so this does not prove automatic discovery without an installed entry.
Models and MCP tools are real; user decision messages are scripted test-operator
inputs, not evidence of an authenticated end-user identity or desktop question UI.

Codex: normal work completed without an approval question; semantic expansion caused
a scope check and a natural question before any shared-module edit; refusal was
reported as false, existing work was delivered with a concrete blocker and then
stopped. A limited approval was saved with its exact quote and subfeature label in
the corrected run. Cross-session outcomes and commands are recorded in the review
package's live-session matrix and raw JSONL.

DSH: the initial attempt returned `MISSING_CREDENTIAL`. The user subsequently
authorized the desktop DS key for test-process use only. It was loaded only into
the child process environment; its value was not printed, copied into the review
archive, or saved in host configuration. Real DSH model/MCP sessions now cover
ordinary work, initial scope registration, expansion questions, risk explanation,
ambiguous replies and explicit refusal/partial delivery. The acceptance matrix
records the completed cross-session partial-approval outcome. The initial failure
remains in the evidence.

Native GUI question handling, every possible semantic expansion and production
database migrations are not proven by these tests. Scope remains a cooperative
Agent convention, not a sandbox. No release, push, real host configuration change
or real user database modification was performed.

The review package includes exact source, incremental diff, raw logs, synthetic
migration databases, scripts and a SHA-256 manifest. See its acceptance matrix for
the final per-scenario status. No Token or cost saving percentage is claimed.

Native-session call efficiency is not perfect: DSH's unseeded run made a rejected
submission without a base checkpoint, resumed, then corrected it. An earlier Codex
run used invalid submission arguments. These errors were rejected without duplicate
checkpoints; raw logs remain. Do not advertise one-call or zero-error execution for
all models. The deterministic Core API reduces required inference, not every model's
observed tool use.

On completed-task resume, DSH made only one MCP resume and no new submission or
source edit, but it did repeat local file/test verification. This is unnecessary
model work, not a duplicate delivery or scope expansion; the raw session records it.
