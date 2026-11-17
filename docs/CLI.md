# CLI first quick start
Author: EvanBo. Apache-2.0; preserve applicable LICENSE/NOTICE when redistributing.

```sh
cargo install --locked --path . --bins
# A new private directory only. This runs the Rust example and prints real resume JSON.
cargo run --example solo -- /tmp/development-map-new-demo
```

The example creates map.sqlite, context files and agent-token.txt (0600 on Unix), binds one worker for one hour, and records an initial checkpoint. It prints capabilities and explicit missing-context gaps; it is not a completed development project. To resume from a separate process:

```sh
export MAP_AGENT_TOKEN="$(cat /tmp/development-map-new-demo/agent-token.txt)"
printf '%s' '{"project_id":"demo","node_id":"demo/work","limit":10}' |
  map-agent resume /tmp/development-map-new-demo/map.sqlite
unset MAP_AGENT_TOKEN
```

Check the example's printed actual IDs before calling; full operator commands and submission JSON are in [PROTOCOL.md](PROTOCOL.md). A trusted local operator provisions project identities; map-agent cannot mint credentials. Reader/manager is needed for global project browsing; worker is limited to its assigned node. One agent may use both roles where the operator authorizes this.

Record source paths, Git revision and real SHA-256; run the actual project's build/tests and reference the logs. Increment checkpoint version within the current generation. Retry exactly the same JSON and submission_key after an unknown acknowledgement; a changed body needs a new valid version/key. Unknown `progress` is rejected, and native remaining work stays in referenced checkpoint text. No map success means a test passed.

Local MCP uses the same library and needs no HTTP service:

```sh
map-mcp /absolute/path/map.sqlite --token-file /absolute/path/private-agent-token.txt
```

MCP stdout is protocol-only, not an interactive prompt. Have the platform start this process using [PLATFORMS.md](PLATFORMS.md). Exposed tools are map_resume, map_project and map_submit. `map_resume` treats `node_id` as the raw dev-node id (colons, including a literal `dev:` prefix, are preserved); use the typed `node_ref: "dev:<id>"` when a reference is intended. At most one of `node_id`, `node_ref`, `task_id` may be supplied. No acceptance, task execution, scheduler or messaging authority is exposed.

## Read-only status diagnostics (working source, after RC4)

These diagnostics are not in the frozen RC4 download binaries. Build the current working source to use them.

```sh
mapctl status
# With an existing authorized MAP_AGENT_TOKEN in the environment:
mapctl status /absolute/path/map.sqlite project-id [node-id]
```

The first form only reports that this CLI process works; database, host connection and task execution remain `unknown`. The second form reuses authenticated resume and its project/node restrictions; no operator bypass is added. Missing credentials, inaccessible databases, schema mismatches and revoked identities fail with a nonzero exit code; they never trigger creation, migration or repair. Omitting node-id follows the existing resume binding rules.

For MCP, use the existing tool after initialize/initialized:

```json
{"name":"map_resume","arguments":{"project_id":"project-id","diagnostic":true}}
```

The tool list remains unchanged. Without `diagnostic:true`, resume behavior is unchanged. Diagnostic output separates:

| Field | Meaning |
| --- | --- |
| `program.status=available` | This invocation is running; not a health guarantee for another installed copy. |
| `database.status=readable` | An authorized resume read on the supported schema succeeded; not a full integrity scan. |
| `connection.status=request_received` | Only this initialized MCP session received the request. It does not prove the client received the response. CLI reports `unknown`. |
| `connection.host_identity`, `host_health` | Always `unknown`; clientInfo is not authenticated host identity. |
| `task.progress_status=saved_records_only` | Checkpoint and takeover action describe stored map records. |
| `task.execution_status=unknown` | No host heartbeat exists; a checkpoint does not prove a worker is running, paused or dead. |

A missing checkpoint stays null; it is not inferred as zero-percent progress. `confirm_goal`, `continue` and `complete` are stored takeover decisions, not live execution status. Stored checkpoint text remains untrusted data. A failed diagnostic uses existing public status codes and does not expose inaccessible task details. No listener, HTTPS, background polling, scheduling, writes or automatic repair is added.

### VNext Agent scope reminders (schema 14)

`mapctl scope-check DB` and `mapctl scope-report DB` use `MAP_AGENT_TOKEN` and bounded
JSON stdin. These are reminder/record APIs, not Operator approval or OS permissions.
See [DIALOGUE-APPROVAL.md](DIALOGUE-APPROVAL.md) for fields and true/false examples.
