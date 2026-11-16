# Local protocol — EvanBo Development Map

The authoritative Rust inputs are in `store.rs`, `submission.rs`, `identity.rs`, `agent.rs`, `resources.rs`, `handoff.rs`. SQLite schema and upgrade SQL are retained under `src/`; current map schema version is 10.

## Trusted operator

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
