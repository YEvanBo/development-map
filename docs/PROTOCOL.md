# Local protocol — EvanBo Development Map

The authoritative Rust inputs are in `store.rs`, `submission.rs`, `identity.rs`, `agent.rs`, `resources.rs`, `handoff.rs`. SQLite schema and upgrade SQL are retained under `src/`; current map schema version is 12.

## Trusted operator

`mapctl submit-resources DB` requires a project-manager credential in MAP_AGENT_TOKEN. Worker clients use `map-agent submit`, MCP or scoped HTTP. The database owner and in-process MapStore administration APIs have direct SQLite access. For untrusted workers, run the service as a separate OS identity with the database, WAL, backups and their directory private to the operator (0700 directory). Give the worker only its scoped service endpoint and token. A local stdio MCP process that opens SQLite must itself run in the trusted context; a worker shell sharing unrestricted filesystem access with that process has operator authority. Host message authenticity currently depends on the operator attesting the actual user reply.

`mapctl <operation> <database> ...` runs as the local OS user and is not a remote authorization boundary. Use a dedicated map database, not a live scheduler database.

| Operation | Remaining arguments / stdin |
| --- | --- |
| `migrate` | create/upgrade the map database |
| `project` | project_id name source_ref |
| `adopt` | project_id version source_ref digest variant adoption_ref |
| `architecture-node` | id project_id parent_id-or-`-` title kind source_ref sort_key |
| `architecture-edge` | from_id to_id kind source_ref |
| `node` | `NodeInput` JSON on stdin |
| `dependency` | from_node to_node (same project only) |
| `trace` | architecture_node dev_node relation source_ref |
| `register-repo` | repo_key absolute_repository_root source_ref |
| `link-repo` | project_id repo_key |
| `bind` | `{"token":"24+ characters", "binding":{...}}` on stdin |
| `revoke` | token on stdin |
| `backup` / `restore` | destination path, must not exist |

Bindings have subject, project_id, task_id, owner, role, node_id, generation, source_ref and expires_at (Unix seconds). Reader and manager scope a project; worker scopes exactly one node/owner/generation. These are capability roles that one agent may use under operator authorization, not compulsory separate agents. Tokens are hashed in SQLite. If an existing host `tasks` table is present, V1 also checks the worker task/node binding; the standalone database does not create or require that table.

## Project and task intake

One database holds multiple map projects and task nodes. The operator selects an existing project and a stable node ID after confirming the actual assignment. A directory, .devmap project hint, Git remote, Codex project, DSH workspace or session title can narrow the search. Only the database association and active worker credential authorize recovery. Use an explicit node assignment when the host lacks a stable task ID. Ordinary workers propose local architecture in checkpoints. The operator adopts architecture.

These local operator commands accept JSON on stdin. Confirmations and reminder receipts require a trusted operator or host adapter with access to the actual conversation. Worker CLI and MCP expose neither writer. The store trusts the operator's attestation; it does not authenticate a third-party transcript or verify technical claims in it.

| Operation | Contract |
|---|---|
| `lookup-host` | Supply `provider, workspace, task_id`. Use a stable workspace/instance namespace for that provider; qualify a workspace with its host instance if workspace IDs are only locally unique. Returns a candidate including its revision, or null. A candidate still requires a valid identity. |
| `bind` | Existing format; an optional `host_scope: {"provider":"codex","workspace":"local/team-1"}` explicitly associates the supplied task with an existing node. Missing host IDs authorize no automatic lookup. A fresh unscoped binding still requires goal confirmation before its first write. |
| `intake` | Supply `token, binding, confirmation`, optionally `host_scope` and `new_node` (NodeInput). The operator transaction creates or reuses the node in the named project, binds, and appends the attested goal and its initial checkpoint together. For a new project, also supply `new_project` with `name, source_ref, architecture_version, architecture_source, architecture_digest, adoption_ref` from the operator’s initial architecture adoption. Project creation and adoption share the same transaction. A conflicting retry cannot rename the project or replace the architecture. A conflict rolls back candidate creation. Retries with the same turn and goal reuse the receipt. |
| `confirm-goal` | Supply `project_id, node_id, generation, previous, goal, user_ref`. `previous` is the confirmation event returned by resume, initially 0. Use a reference to the actual user reply. A new confirmation appends a checkpoint, advancing the ordinary version/base. Workers re-read before submitting. |
| `record-reminder` | Supply `project_id, node_id, generation, message_ref, delivered_text` after observing the displayed reminder. Optional `codes` identify the topics actually shown. This separate event neither confirms the goal nor verifies product facts. Replaying the same receipt is idempotent. |
| `repair-host` | After reviewing the mismatch, revoke outstanding credentials for the old task/node or wait for expiry. Supply `key` (the lookup fields), `expected_revision, old_project, old_node, project, node, decision_ref`. The operator changes the candidate only; issue a fresh binding separately. Both projects receive an audit event for their own side. Historical checkpoints stay on their original nodes. |

Resume returns `takeover.action` scoped to the stable authorized node. The confirmation event ID is its goal revision. An owner handoff changes the execution generation and requires a valid new binding; the confirmed target persists. `confirm_goal` requests the current deliverable with the relevant initial risks. `continue` uses the attested goal, including corrections to an older goal. `complete` means a checkpoint for this goal revision has a matching acceptance. Later legacy writes and architecture changes leave the node closed until the operator records a newly confirmed goal. Stop. A successful worker submission alone does not establish operator acceptance. Untrusted checkpoint prose and old .devmap-handover content cannot manufacture a confirmation. A worker can act on the user's actual reply in the current session; cross-session reuse requires an operator-attested record. An unknown budget permits safe investigation. Pause the affected hazardous, purchase, freeze or market-release decision when its prerequisites are unresolved.

Run `mapctl migrate DB` with the new binary. Schema v12 snapshots the assigned node, owner, generation and active worker token hashes present during migration from a versioned RC database (v1–10). Newly created assignments require a traceable goal confirmation even when the host has no stable task ID. From experimental v11, existing confirmations and checkpoints are retained. Unconfirmed v11 assignments require confirmation because v11 did not record RC provenance. With writers stopped, restore the auto-generated before-map-v12 backup to a new path using the previous binary to roll back; keep the upgraded database for later history.

A host can pass a local project reference as an optional `project_hint` to `lookup-host`. A mismatch with a stable route returns a conflict. A hint alone creates no association. Keep session IDs ephemeral when the host lacks a stable task ID.

| Operator command | Input and result |
| --- | --- |
| `record-risk` | Pass `project_id, node_id, generation, code, status, detail, source_ref`. Use a reference to the actual user reply for `user_confirmed` or to technical evidence for `evidence_verified`. Other statuses are `unknown, inferred, deferred, not_applicable`. Prior observations are retained. The operator vouches for the referenced source. |
| `assess-risk` | Pass `project_id, node_id, kind` (hardware/software) and `action` (research/hazardous_operation/freeze/procure/launch). Returns pending prerequisite codes. Research accepts unknowns. Safety and compliance claims require evidence where applicable. Operation authority stays with the host. |
| `grant-reference` | Pass recipient node ID in `recipient`, source checkpoint row ID in `checkpoint_id`, and `decision_ref` to create a read-only grant. Only the recipient worker can use the returned `reference_id` with resume for the source checkpoint ID, summary and digest. |
| `revoke-reference` | Pass `reference_id, decision_ref`. Subsequent reference reads fail with the same public error used for an ungranted reference. |

For host-routed nodes, new worker submissions require a confirmed node goal. Previously provisioned unscoped workers retain the original RC submission contract. Exact submission retries on a completed node return the old receipt. New worker writes to an accepted head require an explicit reopen through a fresh operator-attested user turn.

## Agent recovery and submission

```sh
# Load MAP_AGENT_TOKEN privately from the local credential file, not from source control.
printf '%s' '{"project_id":"demo","node_id":"demo/work","limit":10}' |
  map-agent resume /absolute/path/map.sqlite
```

Resume returns node, checkpoint, registered directories, dependencies, resource pages, gaps, freshness and capabilities. Follow `files.next` and `dependencies.next`; do not equate one page with all project knowledge. Native remaining work, blockers and next action are in the referenced checkpoint text. `recovery` and `next_action` may be null and explicit gaps explain this. A CLI or HTTP success is not proof that source files or tests passed.

`map-agent submit` takes a JSON bundle:

```json
{
  "submission_key": "demo-work-generation1-version1",
  "checkpoint": {
    "node_id": "demo/work", "owner": "EvanBo", "generation": 1,
    "version": 1, "digest": "at-least-16-characters-use-real-sha256",
    "summary": "Actual result; remaining work; next action",
    "source_ref": "checkpoint.md", "git_revision": null
  },
  "artifacts": []
}
```

The optional `resources` field is a versioned manifest. A bundle and its artifacts/resources commit atomically. The same key and exact payload return the original receipt; changed payload under the same node/key is a conflict. A submission key is scoped to the checkpoint node (schema v8 composite key), so two nodes may reuse a key; a v7 database is migrated with its existing receipts preserved. Checkpoint versions must increase within an assignment generation. `GET /v1/map/nodes/{id}/checkpoints` is the read-only receipt history; it includes `submission_key` and is authorized by the node/project scope, so a new session token can inherit the same authorization and query it without the original submitter token. Top-level unknown fields, including `progress`, are rejected. `summary` has a 2048-byte UTF-8 bound. Checkpoint `digest` is the submitter's declared source digest; the receipt `payload_digest` is independently calculated over the submitted bundle. Neither proves a test passed.

No resources on a new checkpoint means no exact resource set on that checkpoint; old resources are not silently carried forward. Use a manifest for the new version if exact source recovery and acceptance gates are required. Git revisions must refer to actual immutable objects; uncommitted files must not be described as Git-verified.

## Scoped HTTP

Start `serve-scoped`; send `Authorization: Bearer ...`. Global read examples (reader or manager identity):

- `GET /v1/map/projects`
- `GET /v1/map/architecture?project_id=demo`
- `GET /v1/map/children?project_id=demo&parent_ref=architecture:demo:v1`
- `GET /v1/map/navigation?project_id=demo&node_ref=dev:demo/work`
- `GET /v1/map/search?project_id=demo&q=work`
- `GET /v1/map/changes?project_id=demo&after_event_id=0`

Worker node pages: `/v1/map/nodes/{URL-encoded-id}` and suffixes `/checkpoints`, `/resources`, `/dependencies`, `/artifacts`, `/architecture-traces`. Submission: `POST /v1/map/submissions`, same bundle as CLI. Check pagination and response gaps. Query parameters cannot substitute a different authorization target for the handler's real route selector.

The default server does not expose `jobs.enqueue`, process start/stop, model execution, task creation, notifications to an external service, or device access. `handoff` only changes the recorded map owner/generation using compare-and-swap. There is no result-event dispatch, claim, review inbox, or scheduler endpoint.

Worker submissions bind to the checkpoint used to start work. Send its rowid as base_checkpoint_rowid (0 for a new node). The flat HTTP checkpoint route accepts X-Map-Base-Checkpoint. An omitted base means an empty node for a new worker write. Existing successful submissions can be replayed with their original key and payload. A stale base rejects the whole write. Resume and reconcile pending files with the winning checkpoint before issuing a new submission.

The operator can withdraw one node goal with `mapctl revoke-goal DB` and JSON fields `project_id, node_id, previous, user_ref`. Use the current confirmation revision and the actual user withdrawal. Resume returns the withdrawal revision for the next confirmation. A revoked goal blocks new worker writes even for legacy bindings. Resume accepts optional `task_kind` (hardware/software) to return outstanding `missing_reminders` from recorded delivery receipts. CLI stderr and MCP error text encode domain errors as JSON with `status_code` and `message`; probe or syntax errors remain transport diagnostics.

For reminder receipts, pass the observed confirmation_event as goal_revision (zero before first confirmation). Stale receipts are rejected. Goal changes or withdrawal begin a new reminder cycle.

The opt-in global HTTP worker can write only on an active migrated RC assignment with its original owner and generation and no host route, past detachment or confirmation. Node credentials using the RC exception must also predate migration. Acceptance stops new writes; the exact prior submission can still return its original receipt. An explicit new goal reopens a completed node for scoped workers. Moving a route preserves this protection on the former node. Enrolled nodes require a node-scoped worker credential for new submissions.
