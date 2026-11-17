> Unreleased VNext candidate: schema 14 adds Agent-reported task scope reminders (a gentleman’s agreement).
> Frozen V12 / RC2 release artifacts are unchanged. Read [the migration and trust boundaries](docs/DIALOGUE-APPROVAL.md) before using this candidate.

# Development Map · 开发地图

换个会话，不用再从头跟 Agent 讲一遍项目。它可以翻开开发地图，看上次做到哪儿、改了什么、接下来该做什么。两个任务碰巧改到同一个节点时，先提交的结果会保留下来。另一个 Agent 在退出前存好快照，下次就能找回手头还没交付的改动，对照前一个结果接着做。

Starting a new chat? Your agent can check the map to see where it left off, what changed, and what to do next. If two tasks update the same node, the first submission stays in place. The other agent can save its unfinished changes in a snapshot before it exits, then pick them up next session and merge them with the first result.

支持 Codex 和 DeepSeek Harness（DSH），通过 MCP 和 Skill 接入。The included MCP server and Skill work with both Codex and DSH.

**作者 / Author** — EvanBo

The Rust/SQLite map also records adopted architecture, module dependencies, immutable source references and review evidence. One agent can use it alone, or several agents can share project context.

This is a source release candidate. The prepared source uses Apache-2.0, permits commercial use under that license, and includes EvanBo attribution in NOTICE and companion components. Cargo publishing is disabled in this candidate.

## V1.1 本地候选包 · RC5

按安装目标选择 `macos-arm64.tar.gz`、`linux-arm64.tar.gz`、`linux-x64.tar.gz` 或 `windows-x64.zip`，文件名前缀为 `development-map-v0.1.0-rc.5-`。把包解压到长期保留的位置，告诉 Agent 你要安装到哪个项目，以及地图数据库和凭据文件的位置。Agent 先运行 `install.py plan` 展示改动，获得一次安装许可后执行 `apply`，然后在新会话用 `map_resume` 验证。Linux on WSL 选 Linux 包。

Each local RC5 bundle includes the reviewed V1.1 scope core, CLI, MCP, project Skill and a project installer. The installer uses Python 3.9+. See the [Agent installation guide](packaging/agent-bundle/DEPLOY.md) for commands and host-specific setup.

## 已公开的 RC4 · Published RC4 downloads

 [v0.1.0-rc.4](https://github.com/YEvanBo/development-map/releases/tag/v0.1.0-rc.4)

## Choose the OS and CPU

先识别安装目标的系统与 CPU，再选下面对应的包。WSL 选 Linux 包；Apple Silicon 选 macos-arm64；Intel/AMD 64 位 Linux 选 linux-x64。不要根据当前聊天客户端所在系统替远端机器选包。

| Operating system | CPU | Download |
| --- | --- | --- |
| Windows | x64 / AMD64 | [development-map-v0.1.0-rc.4-windows-x64.zip](https://github.com/YEvanBo/development-map/releases/download/v0.1.0-rc.4/development-map-v0.1.0-rc.4-windows-x64.zip) |
| macOS (Apple Silicon) | arm64 | [development-map-v0.1.0-rc.4-macos-arm64.tar.gz](https://github.com/YEvanBo/development-map/releases/download/v0.1.0-rc.4/development-map-v0.1.0-rc.4-macos-arm64.tar.gz) |
| Linux | aarch64 / arm64 | [development-map-v0.1.0-rc.4-linux-arm64.tar.gz](https://github.com/YEvanBo/development-map/releases/download/v0.1.0-rc.4/development-map-v0.1.0-rc.4-linux-arm64.tar.gz) |
| Linux | x86_64 / amd64 | [development-map-v0.1.0-rc.4-linux-x64.tar.gz](https://github.com/YEvanBo/development-map/releases/download/v0.1.0-rc.4/development-map-v0.1.0-rc.4-linux-x64.tar.gz) |

[Complete source](https://github.com/YEvanBo/development-map/releases/download/v0.1.0-rc.4/development-map-v0.1.0-rc.4-source.zip). Each bundle includes CLI, MCP, Skill, configuration examples, full source and `DEPLOY.md`. No Rust toolchain is needed to run it.

On Linux/macOS, run `uname -s` and `uname -m`. On Windows PowerShell, inspect `[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture`. WSL uses a Linux package. Intel Mac, Windows ARM64 and 32-bit CPUs do not have prebuilt packages here; build from source instead of selecting a different architecture. Linux requires glibc 2.36+ (`getconf GNU_LIBC_VERSION`) and libgcc_s; these are not Alpine/musl or Android packages. macOS targets 11+ but was tested only on the current Apple Silicon host; it is not Apple Developer ID notarized.

## Agent installation

让 Agent 先读包内 `DEPLOY.md`：核验签名后解压到新目录，保留已有数据库、凭据和宿主配置；按示例合入 MCP 和 Skill，执行隔离 smoke 检查，再验证实际宿主连接。完成安装验收就交付，不自动展开其他任务。

1. Identify the target OS, CPU, version directory and explicitly selected project. Download the matching archive plus `.sha256` and `.sig` from the same release. Verify hashes and SSHSIG against a public-key fingerprint obtained through a trusted release channel.
2. Extract into a new directory: `tar -xzf <archive>` on Linux/macOS; `Expand-Archive -LiteralPath <archive> -DestinationPath <new-directory>` on Windows. Read `DEPLOY.md` before configuring anything. Preserve existing maps, credentials and settings.
3. Run `bin/map-mcp --version` (Windows: `bin/map-mcp.exe --version`). Git is needed for repository verification; Python 3 is needed for pending snapshots and smoke checks. No database server, Rust or Node runtime is required for the core binaries.
4. Merge `Skill` into the selected project at `.agents/skills/development-map`, preserving existing contents. Merge the supplied `config/` example into the installed Codex or DSH host configuration. Use absolute executable, database and token-file paths. Never print credentials or embed tokens in configuration; Unix token files require mode 0600.
5. Reuse the authorized project/task binding. If none exists, ask whether to associate an existing task or create one. Paths alone do not authorize a binding. Only a trusted operator records an actual user goal confirmation; installation itself does not create business tasks or confirmations.
6. Run `python3 smoke.py --work <new-nonexistent-absolute-directory>` with the installed Python command. Then verify the actual host MCP tools/list, authorized resume, EOF shutdown and restart persistence. Installing a Skill alone does not guarantee automatic invocation. Deliver after installation checks; do not expand unrelated project work.

Keep databases, backups and tokens outside the program directory. Roll back by pointing configuration at the previous version; never overwrite user data. See the bundle DEPLOY.md and [host integration guide](docs/PLATFORMS.md) for details.

## Linux x64 validation

2026-11-17: the public Linux x64 archive passed all 37 bundle assertions on an Intel/AMD x86_64 Windows laptop running Ubuntu 24.04 under WSL2 (glibc 2.39), with no QEMU runner. Archive SHA-256, internal manifest and SSHSIG were verified. This covers MCP lifecycle, goal correction, restart persistence, task-scoped revocation, exact replay, pending snapshots and backup/restore. This is native x64 instruction execution under WSL2, not bare-metal Linux or a model-backed Codex/DSH session. Bare-metal Linux remains unverified.

Linux x64 已在真实 x64 笔记本的 WSL2 Ubuntu 中直接运行，37 项包级检查通过，没有使用 QEMU。裸机 Linux、该平台的 Codex/DSH 模型会话尚未实测。

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

## Build and test

Rust/Cargo (1.89 or newer for the locked dependencies), a C toolchain (bundled SQLite build), and Git are required. The tested host was macOS arm64, rustc 1.98.0. Cargo.lock is retained. No scheduler, communications bridge, commercial harness, model API, Python backend, database server, or GPU is required.

```sh
cargo build --locked --all-targets
cargo test --locked --all-targets
node --test client/navigation.test.mjs
cargo run --example solo -- /tmp/development-map-solo-new
```

`--offline` may be added when all locked crates are cached. Node is needed only for the optional navigation client tests. The `solo` example uses a new directory and creates an isolated SQLite database, a one-hour agent credential file, and context files. It records initialization, not completed project development. Credentials and runtime databases must stay outside source control.

## CLI installation

请 Agent 帮你安装或配置时，Codex 建议选 GPT-5.6 Sol 或 GPT-6 Astra，DSH 建议选 DeepSeek V4 Flash，思考强度设为高（High）。自己按下面的命令安装也可以。

For help with setup, we suggest GPT-5.6 Sol or GPT-6 Astra in Codex, or DeepSeek V4 Flash in DSH, with reasoning set to High. You can also run the commands yourself.

```sh
cargo install --locked --path . --bins
map-mcp --version
```

Install the portable Skill and use MCP or direct CLI in DSH, Codex, or Gemini Antigravity: [platform guide](docs/PLATFORMS.md), [CLI quick start](docs/CLI.md), [commercial use and attribution](docs/COMMERCIAL.md).

## 本机 HTTP API / Local HTTP API

```sh
cargo run --bin development-map -- --version
cargo run --bin mapctl -- migrate /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve-scoped /absolute/path/new-map.sqlite
```

Both commands start an API server on a dynamic loopback port. `serve-scoped` prints the base URL and accepts provisioned bearer identities. `serve` prints JSON with the base URL and a one-use code. POST the code as `{"code":"…"}` to `/map/session` for a read-only API session.

Trusted local operators initialize projects, architecture and identities. Agents read and submit through `map-agent` or scoped HTTP. See [protocol](docs/PROTOCOL.md), [security](docs/SECURITY.md), [scope](docs/SCOPE.md), and [provenance](docs/PROVENANCE.md).

Native recovery references checkpoint text for remaining work and next action; it does not implement structured `progress`. Unsupported submission fields are rejected. The map never grants shell, device, scheduling, model, or task execution authority. Recorded status, submitted work, source verification and independent acceptance remain separate facts.

## Installer integration candidate

[安装与发现集成说明 / Installer and Host Adapter](docs/INSTALLER-REVIEW.md)：独立轻量安装器支持 Project（默认 Skill + 项目入口）、Global（简短发现规则）、Skill Only，以及计划审批、升级、卸载、故障回滚和全局/项目开关继承。只在临时目录完成验收；真实 Codex/DSH 自动发现仍为 **NOT TESTED**，Windows 安装适配暂不支持。开启不等于 Core 授权，MD 不是安全边界。冻结下载包未改动，本候选未发布。

范围提醒采用 **Agent 君子协议**：超出已记录范围时询问用户，再向 Core 回报 true/false。Core 不认证用户身份、不限制文件访问、不替代原有权限；详见 [范围说明](docs/DIALOGUE-APPROVAL.md)。

可选的薄 Workflow SDK 见 [WORKFLOW.md](docs/WORKFLOW.md)：统一状态与回执读取，不复制权限逻辑；当前对照未证明节省调用或 Token，不强制启用。
