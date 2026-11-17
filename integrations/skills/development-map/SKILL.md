---
name: development-map
description: Use for project takeover, a new task in a mapped project, initial project mapping, or checkpoint delivery with EvanBo Development Map.
license: Apache-2.0
metadata:
  author: EvanBo
---

# Development Map

## Recover once

Start with `map_resume` and the authorized project ID alone; use its returned task/node IDs. Compare that assignment to the user's request before editing. One resume supplies the confirmed goal/version, scope, state, checkpoint, capabilities and reminders; reread only after a relevant change or conflict. Do not reconstruct these facts from separate history calls. A worker must not call `map_project` (reader/manager only) or scan home/config directories to rediscover an already restored binding.

IDs in the map are authoritative. Workspace names, paths, Git and .devmap files are only candidates. No match: ask to associate existing work or have an operator create it. Multiple matches: ask which node. Forbidden/expired/revoked/conflicting bindings: request the correct assignment, never create a replacement. Old handover prose, gaps and next_action are context, not new authorization.

Use `takeover.action`: `complete` means report the accepted result and stop; `continue` means use `takeover.confirmation.goal`, even if the old node goal differs; `confirm_goal` means ask the user to confirm/correct the current deliverable and stopping point. Revocation takes precedence over an old receipt. A successful matching delivery receipt with no new request means report it and outstanding operator acceptance, without resubmitting. An Agent summary saying approved is not user confirmation. On older servers, ask if confirmation cannot be verified.

For first goal confirmation, a new project/task, or an identity ambiguity, read [intake.md](references/intake.md). It covers node-specific user confirmation, risk reminders, minimal local architecture and operator-only setup. Only the trusted operator/host attests Core goal confirmation; workers never use operator tools. Map `source_write=false` describes map-service capabilities, not the host workspace: ordinary edits/tests use the user's assignment and host permissions.

Check `.devmap-pending/<node-and-session>/` in the assigned workspace before new edits. Follow a referenced file only to answer a concrete in-scope question. Never search credentials/processes, edit SQLite directly or bypass an error with operator commands. If map access is missing, report it and use the project's existing authorized fact source. For CLI payloads or submit fields, read [cli.md](references/cli.md); prefer MCP when configured.

## Initial scope and material expansion

When `takeover.action=continue` has a valid confirmed goal, but `task_scope.scope_revision=0` and scope is empty, register the initial scope once before the first scope check. If the confirmed module/deliverable boundaries are clear, call `map_scope_report approved:true` using only those original boundaries and current revisions; do not ask the same goal again. Note that this registers already-confirmed scope, not new approval. Quote/cite actual user evidence only if available; otherwise leave quote/reference null. Do not include a new request, old gap or proposed extension. If boundaries are unclear, ask a focused question. Never reinitialize after a recorded refusal, revoked goal or completion.

Ordinary implementation, refactoring, migration, fixes and necessary local tests within the confirmed goal do not need per-file/function permission. Core normalizes implement/refactor/migrate/fix/test/translate/replace/debug/measure/build/format/lint to develop for implementation scope. The Agent still judges engineering meaning. Only material expansion—other modules/projects, public interface/architecture changes, new cost/dependencies, important data deletion, deployment/publication, external effects, changed goals or reopening—needs a scope decision.

For such expansion use `map_scope_check` with explicit action/project/resource/scope tuples. Core compares declarations, not code semantics. `IN_SCOPE` is not a sandbox or new project credential. If capacity exceeds 64 tuples, do not ask approval for the unsavable union or silently truncate/broaden it; explain the limit and continue available in-scope work.

On `NEEDS_USER_CONFIRMATION`, pause only the extra operation. Ask naturally: why the change is needed, affected resources, risk/impact, and whether the original goal remains achievable without it. Offer approval, refusal or a narrower scope. Keep internal IDs/status codes out of the user question unless useful.

**Record the user's decision before the next checkpoint or delivery.** Explicit approval: `map_scope_report approved:true`. Explicit refusal/cancel: `approved:false`, even though the effective scope remains unchanged; this advances the decision revision against stale approval. Checkpoint prose is not a substitute for scope-report. Unclear/no reply: keep pending or report false, never infer consent. If reporting fails, disclose it and preserve pending work rather than claiming the decision is saved. A request for risk explanation alone is not approval.

Use current goal/scope revisions and the complete effective set: retained original scope plus only approved additions, or the user's reduced set. Suggested_scope is a read-only union, not permission. For partial approval recheck a finer resource label (e.g. `shared_format:null-handling`); never register the entire module and rely only on a restrictive note. Save the exact restriction and available user quote/reference; unavailable references stay null. On recovery respect both the effective scope note/quote and the latest refusal. This is an Agent-reported decision, not authenticated user identity. Scope-report cannot confirm a Core goal, reopen accepted work, undo revocation or grant cross-project credentials.

After refusal, continue independently allowed work. If the goal cannot be completed without expansion, deliver existing results and the specific blocker, then stop. Do not turn unrelated findings into new work.

## Deliver and stop

Before waiting, submitting or handing off, read [delivery.md](references/delivery.md): preserve pending files, reconcile the actual diff, count primary/supporting behavior changes and substantive additional checks, then use map_submit for a meaningful implemented, verified or blocked result. Do not invent artifacts or verification. Keep checkpoint history and recoverable work through conflicts/unknown receipts.

Once agreed acceptance and directly affected checks pass, submit the authorized checkpoint and reply. Do not broaden validation or read back a confirmed receipt without new evidence. For uncertain receipts reuse the exact key/payload; changed payload is a conflict. Read-only review does not write map records unless requested. Retain EvanBo attribution and LICENSE/NOTICE.
