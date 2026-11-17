# Lightweight Installer + Host Adapter — review candidate

EvanBo / Apache-2.0. Independent integration layer, Python 3 standard library, POSIX only. No V12 Core code, database schema, authorization gate, scheduler, daemon, proxy or GUI is added. This is not a release approval.

## Installation choices and lifecycle

Default **Project** installs the portable Skill and a short owned project instruction block in one transaction. **Global** installs a short discovery rule and owned read-only status entry in an explicitly supplied actual host home; it contains no project_id/node_id and does not install a project Skill globally. **Skill Only** copies the Skill into the explicit project without adding instructions. It reports effective automatic discovery disabled. None of these modes provisions identities, creates map nodes, accepts user goals, reads credentials, or grants database access.

Independent CLI is deliberate: integrating this filesystem lifecycle into mapctl would change the frozen Core CLI contract. The Agent and a human use the same deterministic executable. There is no general package manager or dependency installer.

```sh
# Discover is read-only; all paths below are operator-selected examples.
python3 integrations/map-install.py discover --host codex --root /absolute/project
python3 integrations/map-install.py plan --host codex --root /absolute/project --mode enabled > /private/plan.json
python3 integrations/map-install.py apply --host codex --root /absolute/project --plan /private/plan.json
python3 integrations/map-install.py verify --host codex --root /absolute/project

# Global root is the ACTUAL chosen CODEX_HOME/DSH_HOME, never guessed/shared.
python3 integrations/map-install.py plan --host codex --scope global --root /actual/codex-home --mode enabled > /private/global-plan.json
python3 integrations/map-install.py apply --host codex --scope global --root /actual/codex-home --plan /private/global-plan.json

# Project inheritance of an explicitly selected global policy:
python3 integrations/map-install.py plan --host codex --root /absolute/project --global-root /actual/codex-home --mode inherit > /private/inherit-plan.json
# Apply that plan as above.

# Explicit Skill Only; convert an existing installation with --flavor project.
python3 integrations/map-install.py plan --host dsh --root /absolute/project --flavor skill-only > /private/skill-plan.json

python3 integrations/map-install.py status --host codex --root /absolute/project
python3 integrations/map-install.py disable --host codex --root /absolute/project
python3 integrations/map-install.py enable --host codex --root /absolute/project
python3 integrations/map-install.py uninstall --host codex --root /absolute/project
```

`enable/disable/uninstall` display a fresh plan and invoke the same approval/apply path. `--scope global` targets global policy; a project `--mode enabled/disabled/inherit` represents on/off/inherit. Changes to an existing installation retain its flavor unless explicitly changed. Global cannot inherit. There is no automatic Core authorization, and disable never revokes a token or erases user confirmations/checkpoints. Future enable still requires the existing Core checks for every operation.

| Global policy | Project override | effective_enabled |
| --- | --- | --- |
| off | on | true |
| on | off | false |
| on/off | inherit | follows global |
| absent | inherit | false |
| any | Skill Only | false |

Status recomputes inheritance on every call, checks the current owned instruction block and every installed Skill hash, and fails closed on missing/changed files or conflicting global state. Installed, configured enabled, effective_enabled and Authorized are separate. Authorized is always NOT_CHECKED by this installer. A project on does not require enabling a globally disabled policy.

The instruction rule asks engineering tasks to check live integration status before automatically discovering the map. Ordinary questions do not run that check. A false effective_enabled or failed status means no automatic map invocation. This is deterministic policy computation plus host instruction guidance, **not a security hook**. Cached sessions can ignore changed instructions; actual enforcement and spontaneous Agent invocation remain NOT TESTED. Restart and validate in a fresh real host session before claiming activation.

## Approval and file safety

Agent may identify the selected host and generate/explain a plan; it must not independently approve it or edit host files. Plan includes absolute files/directories, before/after bytes, modes, owned hashes, exact generated rule and a digest. Plans/journals may contain existing user instructions; keep them private. The operator reviews once and types `APPROVE <digest>` on an interactive terminal. Piped input is refused. No --yes or stored approval boolean is accepted. Recheck the plan after confirmation and again under a shared per-root lock; changed inputs require a new plan/confirmation.

Approval is operator-attested. A same-UID unrestricted Agent can emulate a terminal or edit files; TTY/MD are not identity authentication or isolation. Real user confirmation in the map remains operator-attested with user_ref as evidence reference. Untrusted workers require OS UID/sandbox isolation from installer targets, DB/WAL/backups and operator credentials. This layer never opens those resources or weakens V12 permissions.

The existing unique managed marker is retained. Version/host/scope/flavor, original owned block, file hashes and created directories are recorded. Updates replace only owned unmodified content; unowned existing Skill files, modified hashes/blocks, ambiguous markers, links, stale plans and scope conflicts are rejected. External text and original line endings are preserved. Uninstall removes owned files and blocks only; nonempty directories/user additions are retained. Two hosts sharing the same instruction root cannot silently adopt one another's block: the second is blocked, requiring an explicit shared-ownership design outside this scope.

New files are 0600 and new directories 0700; existing file modes are unchanged. Per-file staged replace, file/parent directory fsync, a bounded journal and a shared root lock protect the transaction. Synchronous failures roll back only known before/after content; unexpected user edits preserve the journal and report BLOCKED. `recover` uses the same explicit host/root/scope and operator approval. Do not delete a journal to bypass a conflict. Multi-file writes are recoverable, not globally atomic to arbitrary readers. Power-loss recovery and hostile same-UID races are not certified.

## Host Adapter matrix

| Host | Actual instruction mechanism | Skill mechanism | MCP configuration | Verification |
| --- | --- | --- | --- | --- |
| Codex | Selected host home/project AGENTS.md; existing AGENTS.override.md takes precedence | Project .agents/skills/development-map | Existing TOML config/stdio registration; preserved, not automatically edited | Fixture PASS; actual loading/calling NOT TESTED |
| DSH | Actual DSH_HOME/project AGENTS.md; depends on agent-instructions provider/profile | Project .agents/skills; requires Skill provider/consumer | Existing YAML composition/headless patch; preserved, not treated as Codex TOML | Fixture PASS; actual profile loading/calling NOT TESTED |
| Antigravity/other | No reliable adapter implemented | Not installed | Not edited | UNSUPPORTED |
| Windows installer | ACL/replace adapter not implemented | Not installed by this candidate | Not edited | UNSUPPORTED; existing runtime archives unchanged |

Codex and DSH do not share a guessed GLOBAL.md or inferred home directory. No Hook support is assumed. No existing host MCP configuration is overwritten. If a host lacks the providers/global loader, use an explicit task prompt and already authorized MCP setup; report NOT TESTED/BLOCKED instead of claiming automatic discovery. Installing Skill/rules alone does not configure a missing MCP connection: use the existing host-specific examples in integrations/codex and integrations/dsh, under a separately reviewed setup plan. No real configuration or secret was inspected in this task.

Sources checked in the preceding implementation and reused for this bounded adapter:
- https://learn.chatgpt.com/docs/agent-configuration/agents-md
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/context/agent-instructions/README.md
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/skill/skill-filesystem/README.md

## Evidence tiers and real-session acceptance

1. File verification: actual instruction block + all owned Skill hashes. Fixture tests cover this.
2. Host configuration loaded: NOT TESTED until a fresh real host reports the intended instruction/Skill catalog.
3. CLI/MCP readable: NOT TESTED by the installer; verify via the existing authorized diagnostics/resume.
4. Agent spontaneous discovery: NOT TESTED until a real Codex/DSH session starts an engineering task without a Skill reminder and records actual discovery/call traces. Repeat with global off/project on, global on/project off, inherit and Skill Only. Use synthetic projects; test disable in a fresh session and explicitly record cached-session limits.

Also test a normal conversational question causes no map discovery, and no-binding/goal-unconfirmed still asks the user instead of creating/authorizing work. Config presence or old model test logs are not acceptable substitutes. No native host session was run here because tests were restricted to fixtures; these rows remain NOT TESTED/BLOCKED.

## Reproduction

```sh
python3 -B -m unittest discover -s integrations -p test_map_install.py -v
cargo test --locked --offline --all-targets
node --test client/navigation.test.mjs
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
```

Core fmt/clippy findings must be reported without editing frozen Core. The integration review package has its own unsigned SHA-256 inventory and patch. No V12 archive, Core manifest/signature or release asset is regenerated or signed in this task; prior worktree differences are preserved and separately attributed.

## Unified approval contract follow-up

Host approval proposals were superseded by [Agent scope reminders](DIALOGUE-APPROVAL.md). No native identity verification is required or claimed; Installer discovery limitations remain unchanged.

Optional deterministic Workflow facade: see [WORKFLOW.md](WORKFLOW.md). It reuses Core capabilities and does not authenticate users or add authority.

## P1 follow-up: Installer v3

Project instructions now call the owned `.agents/skills/development-map/scripts/map-status.py`
from the directory containing the instruction file, using the current physical working
directory as root. No download location or project-specific absolute path is embedded
in project MD. Global rules use the explicitly approved host-root status entry, not
the download directory. Both entries are hashed assets in the same install/upgrade/
rollback/uninstall transaction. Global installs still do not install the Skill itself.

The CLI restriction is a lifecycle safeguard, not a same-UID security boundary.
The status entry is a self-contained snapshot of installer code with a CLI restricted
to status/discover/verify; it opens no database. It is intentionally not a second
independent policy implementation. Use the distribution installer for lifecycle
changes. This adds roughly one installer-sized file per scope, avoids PATH or global
configuration changes, and survives removal of the source distribution. Existing
v2 rules upgrade through normal ownership checks and explicit approval. Tampered
status entries block upgrades/uninstall rather than being silently overwritten.

Cloning a project does not clone authority. If a copied installation record still
names a different absolute project root, status fails closed; explicitly reinstall/
resolve ownership on that machine. This fix guarantees independence from the original
download path, not automatic cross-machine identity or global policy rebinding.

Lifecycle plans inspect local ownership only (`limits.effective_enabled=null`), so
broken global policy does not prevent local disable/uninstall. Discovery still validates
global policy and fails closed. Local modified blocks/assets still block destructive
cleanup, and no global files are repaired or changed as a side effect.
