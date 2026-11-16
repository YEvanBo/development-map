# Development Map · 开发地图

换一个会话，Agent 也能从最近的检查点接着做。新会话可以查到上次交付了什么、证据在哪里，以及下一步要做什么。如果两个任务从同一个检查点开始，其中一个先提交，地图会拒绝另一个基于旧检查点的提交。在等待或退出前，Agent 按 Skill 保存交接说明和改动文件的私有快照；重启后就能找回未交付的工作，对照先提交的结果，合并后继续交付。

An agent can pick up where a previous session left off. The latest checkpoint shows what was delivered, where to find the evidence, and what to do next. If two tasks start from the same checkpoint, the first can submit and the second receives a conflict. Before waiting or exiting, the agent uses the Skill to save a private snapshot of its handoff notes and changed files. After restarting, it can recover its pending work, compare it with the first submission, and merge the two results.

**接入 / Integrations** — Codex 和 DeepSeek Harness（DSH）均可通过随附的 MCP 服务与 Skill 使用开发地图。Both Codex and DeepSeek Harness (DSH) can use the bundled MCP server and Skill.

**作者 / Author** — EvanBo

The Rust/SQLite map also records adopted architecture, module dependencies, immutable source references and review evidence. One agent can use it alone, or several agents can share project context.

This is a source release candidate. The prepared source uses Apache-2.0, permits commercial use under that license, and includes EvanBo attribution in NOTICE and companion components. Cargo publishing is disabled in this candidate.

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

```sh
cargo install --locked --path . --bins
map-mcp --version
```

Install the portable Skill and use MCP or direct CLI in DSH, Codex, or Gemini Antigravity: [platform guide](docs/PLATFORMS.md), [CLI quick start](docs/CLI.md), [commercial use and attribution](docs/COMMERCIAL.md).

## Local access

```sh
cargo run --bin development-map -- --version
cargo run --bin mapctl -- migrate /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve-scoped /absolute/path/new-map.sqlite
```

Both server modes bind only `127.0.0.1` on a dynamic port. `serve` prints a one-use launch URL for the built-in read-only browser view. `serve-scoped` prints the base URL and requires provisioned bearer identities for APIs. It does not enable browser cookie login.

Trusted local operators initialize projects, architecture and identities. Agents read and submit through `map-agent` or scoped HTTP. See [protocol](docs/PROTOCOL.md), [security](docs/SECURITY.md), [scope](docs/SCOPE.md), and [provenance](docs/PROVENANCE.md).

Native recovery references checkpoint text for remaining work and next action; it does not implement structured `progress`. Unsupported submission fields are rejected. The map never grants shell, device, scheduling, model, or task execution authority. Recorded status, submitted work, source verification and independent acceptance remain separate facts.
