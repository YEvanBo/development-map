# Agent scope reminders — final V1 product, schema 14 candidate

The Agent asks the user; Core compares declared boundaries and records the Agent's
true/false report. This is a **gentleman's agreement**, not a sandbox, user identity
check or proof of actual consent. An Agent can edit files without calling it. No PKI,
key registry, signature receipt, nonce ledger, daemon, GUI or approval server.
Frozen signed V12 archives/releases are unchanged. No real DB/config/credentials used.

## Minimal protocol

1. `map_scope_check` / `mapctl scope-check DB` receives project_id, node_id and
   `operations`: explicit `{action, project, resource, scope}` tuples.
2. Existing task identity and goal state are checked first. Unconfirmed, revoked or
   accepted work is BLOCKED; this API cannot confirm a Core goal or reopen a task.
3. Every exact declared tuple in the saved effective set -> IN_SCOPE. Missing tuples
   or no explicit operations -> NEEDS_USER_CONFIRMATION, exact scope_difference,
   reason, current goal_revision/scope_revision, suggested_scope (program-computed union) and a question instruction. Use suggested_scope only after explicit agreement; for partial approval recheck only the agreed additions to obtain the narrower union.
4. Agent explains the extra work in normal conversation. Explicit yes -> report true;
   no/cancel/ambiguous -> false; silence -> pending or false. Never guess approval.
5. `map_scope_report` / `mapctl scope-report DB` records the decision in map_events.
   True replaces the effective set with exactly the complete user-approved set.
   Include retained original items plus agreed additions; if the user narrows the
   set, preserve that limitation. False advances the decision revision (so stale approval cannot overwrite refusal), records the refusal and leaves the prior set
   unchanged. It does not stop independently allowed original work.
6. `map_resume.task_scope` and subsequent check calls recover the current goal's set
   across sessions. New goal revision does not inherit old scope. Stale goal/scope
   revisions conflict instead of overwriting a newer choice. No automatic retry.

The first scope set is reported from the user's existing confirmed task boundaries;
if those boundaries are clear, do not ask the same goal again. If only free prose is
available and its boundary is unclear, Core cannot infer one: Agent asks the user.
The original Core goal-confirmation requirement remains separate and unchanged.

## Exact matching and when to ask

Tuple matching preserves action/project/resource/scope association; it does not form
a cross product. Labels are literal case-sensitive logical boundaries, not glob/path
patterns. `*`, control characters and empty labels are rejected. Up to 64 tuples,
256 UTF-8 bytes per field. No filesystem/path canonicalization or code semantics.

If the proposed union exceeds 64 tuples, check returns BLOCKED with reason
SCOPE_CAPACITY_EXCEEDED, required/limit counts, no suggested_scope and no approval
question. Do not silently truncate or broaden labels. Existing in-scope work remains
available; a smaller effective set must still reflect the user's actual boundaries.

For `scope=implementation`, actions implement/refactor/migrate/fix/test/develop,
translate/replace/debug/measure/build/format/lint all
normalize to develop. Choose a module/deliverable boundary, not every file or line.
Ordinary code work, necessary supporting changes and directly affected regression
within it do not need repeated permission. Core trusts the Agent's declaration.

Use distinct scope labels and exact actions for material public-interface/architecture
changes, extra dependencies/cost, important data deletion/overwrite, deployment,
publication, external effects, changed goals and reopening. A new module/project label
also differs. Agents must declare these honestly rather than disguise them as an
implementation detail. Natural-language risk/impact and alternatives belong in the
question and decision note; Core neither infers nor validates them.

Reporting a tuple for another project does not create its credentials, node or write
permission. IN_SCOPE only means "declared scope matches the record". All preexisting
Core submission, resource, accepted/revoked and identity gates remain authoritative.
Correction/reopen/goal changes cannot be performed through scope-report; use existing
authorized mechanisms, then establish scope for that new confirmed goal.

## Example: module expansion, then user limitation

Check:
```json
{"project_id":"p","node_id":"a","operations":[{"action":"fix","project":"p","resource":"shared-format","scope":"implementation"}]}
```
Expected if only export was saved: NEEDS_USER_CONFIRMATION, difference shared-format.
Agent question (fixture): “JSON 导出还需要改共享格式模块，否则原目标做不完。只改空值处理，影响两个调用点，需要补一个回归测试。你允许这部分吗？”
User (fixture): “可以，只改空值，不要调整公共接口。”
Agent reports true with scope tuples for export and shared-format:null-handling implementation;
note records "仅空值处理，公共接口不改". The prose limitation is behavioral context,
not semantic enforcement: choose finer exact resource labels if comparison must
separate subfeatures. A later public_interface tuple still differs and asks again.

Report shape (replace revisions with current check/resume values):
```json
{"project_id":"p","node_id":"a","goal_revision":10,"scope_revision":20,"approved":true,"scope":[{"action":"develop","project":"p","resource":"export","scope":"implementation"},{"action":"develop","project":"p","resource":"shared-format:null-handling","scope":"implementation"}],"user_reply":"可以，只改空值，不要调整公共接口。","message_ref":null,"note":"仅空值处理；公共接口不改"}
```
`approved` must be a JSON boolean. user_reply/message_ref are optional and nullable;
quote the user when available, never invent native IDs or substitute an Agent summary
for a quote. A summary belongs in note. Truthfulness is the Agent's responsibility.
Records say agent_reported_user_decision, identity_verified=false; they do not prove
user agreement. The response does not carry new authorization tokens.

Refusal/cancel/unclear fixture: approved=false with an accurate note; original export
work remains allowed. No-reply fixture: no report; extra module remains out of scope.
For a user limit, send only the agreed complete tuple set, never the broader rejected
proposal. No task can inherit another node's recorded set.

## Interfaces, trust, migration

CLI requires existing MAP_AGENT_TOKEN; MCP uses its existing configured token.
Scope check is read-only. Scope report requires a current worker or manager and only
adds a decision record; a reader cannot write. Existing role/host/generation checks
are reused. No Operator key is handed to workers. Scope never changes Worker write
gates or binds a project. Operator-attested host workflows may supply context, but
are optional. Codex command approval and DSH tool approval are not goal confirmation.
Native Codex CLI and DSH headless model/MCP sessions have been exercised in disposable workspaces;
user choices were supplied by a scripted test operator. This is not a desktop UI or
cryptographic user-attestation test. The initial DSH credential failure was resolved
by explicit user authorization for test-process-only key use; no host configuration
was changed. See [closeout evidence](CLOSEOUT-V1.1.md).

Schema 14 only advances compatibility metadata and uses existing map_events. No new
scope tables. v12 -> v14 preserves existing facts; v13 -> v14 preserves old prototype
approval tables as inactive history without exposing their endpoints. Old prototypes
and tests remain in prior review archives, removed from active candidate to avoid two
competing product models. Existing baseline/Installer/Workflow tests are retained.

Migration makes the existing automatic backup. Test on copies. Before new writes,
restore a backup to a new path with its old binary if reverting; keep the upgraded
copy. After new writes, prefer forward repair rather than losing decision history.
Never drop old approval records as part of this migration. Older binaries reject newer
schema. No user database or frozen V12 source ZIP is migrated/overwritten by this task.

Resume includes effective scope note/user quote and last decision context, including
`last_decision_user_reply` separately from effective `user_reply`. A later refusal
must not be presented as though the earlier consent were the latest user reply. Do not discard user restrictions when continuing a task. See the host-specific evidence and remaining limits in [CLOSEOUT-V1.1.md](CLOSEOUT-V1.1.md).
