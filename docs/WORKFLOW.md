# Thin Workflow facade — optional SDK, not a new authority

Task c5e16cc5. The explicit design objective is **a little deterministic code to
reduce repeated Agent state assembly and mistakes**. Intent understanding,
engineering analysis, design and tradeoffs remain the Agent's job. This is not a
fixed reasoning pipeline, scheduler or autonomous dispatcher.

## What existing Core already does

`src/agent.rs:112` resolves and authorizes a bound node. The same read transaction
assembles goal confirmation, checkpoint, dependencies and resources. At line249 it
uses takeover.worker_write_gate — also used by submission.rs — to report submit
capability. No second policy engine is needed. `map_resume` already costs one
request. `map_submit` checks scoped identity, base checkpoint, owner/generation,
version, goal/completion gates and exact replay inside Core's transaction.

This candidate adds `integrations/workflow.py:run(request, call_tool)` as an
optional in-process SDK. The trusted host supplies an already authenticated Core
MCP tool client. It does not open SQLite, read tokens, spawn mapctl, choose a model,
create tasks or register a new MCP server. Do not accept a callback or Core result
from Agent-authored JSON as authenticated evidence. Existing installer does not
silently activate this SDK or edit MCP configuration.

## Operations and schema

Input/output schemas: integrations/workflow-request-v1.schema.json and
workflow-response-v1.schema.json. Current implementation:

| Operation | Dispatch | Boundary |
|---|---|---|
| resume / inspect | one map_resume | inspect is assigned context, not whole-project audit |
| submit | one map_submit, exact arguments | no local preflight, mutation, retry or new key |
| intake / accept / revoke / reopen / verify | BLOCKED, zero calls | no safe frozen worker MCP equivalent; operator CLI is not a fallback |
| correction.propose / apply / history | BLOCKED, zero calls | no first-class correction-event protocol; do not disguise it as submit |

```python
from workflow import run
view = run({'action': 'resume', 'arguments': {'project_id': 'p'}}, trusted_core.call_tool)
# Inspect view['required_user_action']; capabilities are the Core snapshot.
# A submitted request must be saved exactly before calling this SDK.
receipt = run({'action': 'submit', 'arguments': {'submission': saved_submission}},
              trusted_core.call_tool)
```

Output carries action/status, project/node, goal_revision, checkpoint/version,
generation, capabilities/source, required_user_action, reason/error,
receipt/idempotency, and full original context. Goal revision is the authoritative
confirmation_event ID, not worker generation. Unknown values are null, not false
permissions or invented IDs. Submit receipt does not contain project/current goal
or capabilities; those fields stay null rather than copying unverified request
selectors. Full context is retained so no source/evidence is silently truncated;
source text remains untrusted data, not instructions.

Capabilities are copied verbatim from Core, never inferred from status or a host
approval. They describe that read snapshot; later writes always recheck in Core.
CONFIRM_GOAL/complete prompts are deterministic presentation of Core takeover, not
a replacement write policy. NO_BINDING prompts association choice; FORBIDDEN and
EXPIRED_OR_REVOKED never become automatic new-project authorization.

## CAS, replay and unknown outcomes

Supported native submit conditions are base_checkpoint_rowid plus checkpoint
owner/generation/version. Goal_revision, host_identity and expected_version are
not new atomic submission conditions in V12. Passing such new outer preconditions
is BLOCKED; putting unknown fields in native payload is rejected by Core. Never
simulate atomic goal CAS by resume-then-write. expected_source_version remains the
existing resume read check, not a general write CAS guarantee.

Exact replay returns Core's original receipt IDs with created=false; altered
payload conflicts. Do not preflight with capabilities.submit: accepted/revoked
state can still permit retrieval of a prior receipt without writing. Wrong binding
or generation is still checked by Core. Accepted nodes never automatically reopen.
Transport failure or an unparseable submit response yields OUTCOME_UNKNOWN, no
receipt and no retry; preserve the exact request and reconcile with Core, not a new
key. A structured Core error is returned intact. No host/user attestation is
invented or attached by this layer.

## Correction proposal (not implemented)

Follow HOST-APPROVAL.md: Worker proposes only within valid assigned scope; applying
requires distinct live Core authority and appends a correction event referencing
the original fact. Goal/permission/binding/completion changes require their own
explicit authorization, never correction.apply -> submit or implicit reopen.
First-class correction history and atomic goal-revision/host-proof conditions need
Core changes; freezing V12 makes them BLOCKED here, not a local surrogate database.

## Actual comparison and acceptance

`workflow-fixture` is a separate test crate linking unchanged Core. It always creates
its own temporary DB, uses synthetic identities, and drives the actual MCP Session
with actual Core transactions. Test-only Operator setup supplies confirmations,
revocation and acceptance; the SDK does not implement those operations. No real
user database, host credentials or native Agent sessions are used.

Observed baseline vs facade for identical active-node context:

| Measure | Direct Core | Facade |
|---|---:|---:|
| Core calls | 1 | 1 |
| JSON bytes (Python default encoding) | 2288 | 2776 |
| Calls saved | — | 0 |
| Token use / model reasoning time | NOT MEASURED | NOT MEASURED |

No claimed Token or round-trip saving. Envelope adds488 bytes; mandatory deployment
is not justified on performance grounds. Deterministic convenience is explicit
selection of node ID, confirmation revision, checkpoint row/version, capabilities,
confirmation/stop prompt and receipt/replay status. These formerly nested field
lookups/branches are exercised by code, but reduced model errors or reasoning cost
has not been measured. An Agent already reading Core fields correctly may gain
nothing. Do not add an extra resume/project request merely to inflate the baseline.

The real Core fixture verifies no matching task, unconfirmed, active, revoked,
accepted, stale checkpoint, wrong generation, exact replay, changed payload and
blocked unsupported operations (25 recorded facade cases, 18 Core tool calls).
The stale-base scenario is sequential interleaving of old/new snapshots, not two
simultaneous native Agent sessions. Correction tests prove refusal only, not a
working correction feature. Fixture acceptance uses a logical resource manifest,
not product/hardware evidence. The first fixture attempts failed because test
acceptance metadata/resources were incomplete; raw failures are retained and Core
was not weakened to make the test pass.

## Reproduction and integration status

```
python3 -B -m unittest discover -s integrations -p 'test_*.py' -v
cargo build --manifest-path integrations/workflow-fixture/Cargo.toml --locked --offline --target-dir target/workflow-r1/build
python3 -B integrations/workflow-fixture/check.py target/workflow-r1/build/debug/devmap-workflow-fixture
```

This optional facade complements Installer and Host Approval contracts in the same
review candidate. It does not close native Codex/DSH/Harness attestation, MCP config
installation, immediate cached-session disabling, Windows or atomic approval-ledger
gaps. Overall BLOCKED; no push/publication. Frozen V12 and existing Rust tests are
unchanged. Existing independent findings/fixes are preserved in prior review ZIPs.

## P1 receipt validation follow-up

A successful receipt requires bool created, strictly positive integer checkpoint_rowid
and event_id (bool is rejected), and exactly 64 lowercase hexadecimal payload_digest
characters. Wrong types/values keep OUTCOME_UNKNOWN with receipt=null; no retry or new
key is issued. Shape validation is not authentication: trust still comes from the
already authenticated Core client.
