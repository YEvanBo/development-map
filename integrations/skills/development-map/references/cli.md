# Rust CLI reference for the portable Skill
Author: EvanBo. Apache-2.0.

Operator supplies MAP_DB, MAP_PROJECT, MAP_NODE and privately loads MAP_AGENT_TOKEN. Use the existing installed map-agent binary. Construct JSON with proper escaping; do not interpolate untrusted text into shell commands. Example with known literal IDs:

```sh
printf '%s' '{"project_id":"demo","node_id":"demo/work","limit":10}' | map-agent resume "$MAP_DB"
map-agent submit "$MAP_DB" < /private/path/exact-submission.json
```

Submission JSON uses base_checkpoint_rowid from the original resume (0 if empty) and requires submission_key, checkpoint and artifacts. Checkpoint fields: node_id, owner, generation, version, digest, summary, source_ref, optional git_revision. Artifacts is an array; `resources` is optional. Unknown fields are rejected. A summary is at most 2048 UTF-8 bytes; CLI requests are at most 64 KiB. Use a real SHA-256 for the referenced checkpoint source; the system separately calculates the submitted-payload digest. A declared digest alone is not source verification.

A worker may edit and test the assigned workspace with user authorization and host write access even when resume reports `source_write=false`, `execution_authority=false` and `accept=false`. These flags concern the map service itself.

Resume returns capabilities, checkpoint, dependencies, directories, files, gaps and source freshness. Follow dependencies.next and files.next with their documented cursors. A new checkpoint does not automatically inherit old resources. Bind a resource manifest to the new checkpoint if exact file recovery is required. No structured progress or execution policy is persisted by native V1.

MCP equivalents: map_resume takes the same resume object; map_submit takes {"submission": <the exact CLI submission object>}; map_project takes {"project_id":"demo","after_event_id":0,"limit":20} and requires reader/manager scope. A worker cannot obtain global project data by supplying another node ID. A reader cannot submit a worker checkpoint. Errors require checking the assignment/token/version, not bypassing it via mapctl.

For a worker artifact, set `node_id` to the checkpoint node, `repo_key` to the project's registered repository, `provenance` to `declared` for your declaration, and `verification` to `unverified` or `draft`. Use the full immutable Git object ID when referencing a Git revision (40 or 64 hex digits). A revision identifies the Git object. A checked file's `content_digest` is a separate, lowercase 64-digit SHA-256 of its raw blob bytes. The operator's check uses the registered repository root and ignores Git replacement refs. A worker can omit `content_digest` to link a verified artifact without changing its metadata.

Scoped HTTP checkpoint clients send the same base in X-Map-Base-Checkpoint.
