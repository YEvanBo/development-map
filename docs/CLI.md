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
