# Development Map · 开发地图

换个会话，不用再从头跟 Agent 讲一遍项目。它可以翻开开发地图，看上次做到哪儿、改了什么、接下来该做什么。两个任务碰巧改到同一个节点时，先提交的结果会保留下来。另一个 Agent 在退出前存好快照，下次就能找回手头还没交付的改动，对照前一个结果接着做。

Starting a new chat? Your agent can check the map to see where it left off, what changed, and what to do next. If two tasks update the same node, the first submission stays in place. The other agent can save its unfinished changes in a snapshot before it exits, then pick them up next session and merge them with the first result.

支持 Codex 和 DeepSeek Harness（DSH），通过 MCP 和 Skill 接入。The included MCP server and Skill work with both Codex and DSH.

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

请 Agent 帮你安装或配置时，Codex 建议选 GPT-5.6 Sol 或 GPT-6 Astra，DSH 建议选 DeepSeek V4 Flash，思考强度设为高（High）。自己按下面的命令安装也可以。

For help with setup, we suggest GPT-5.6 Sol or GPT-6 Astra in Codex, or DeepSeek V4 Flash in DSH, with reasoning set to High. You can also run the commands yourself.

```sh
cargo install --locked --path . --bins
map-mcp --version
```

Install the portable Skill and use MCP or direct CLI in DSH, Codex, or Gemini Antigravity: [platform guide](docs/PLATFORMS.md), [CLI quick start](docs/CLI.md), [commercial use and attribution](docs/COMMERCIAL.md).

## 本机网页 / Local web view

想看看 Agent 做到了哪一步？运行下面的 `serve` 命令，打开终端打印的本机链接，就能在只读网页里搜索节点、看架构树和提交记录。Agent 通过 MCP 或 CLI 写回进度。

Run `serve` and open the local link printed in the terminal to search nodes, browse the architecture tree, and read checkpoints. The web view is read-only. Agents write updates through MCP or the CLI.

```sh
cargo run --bin development-map -- --version
cargo run --bin mapctl -- migrate /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve /absolute/path/new-map.sqlite
cargo run --bin development-map -- serve-scoped /absolute/path/new-map.sqlite
```

Both server modes bind only `127.0.0.1` on a dynamic port. `serve` prints a one-use launch URL for the built-in read-only browser view. `serve-scoped` prints the base URL and requires provisioned bearer identities for APIs. It does not enable browser cookie login.

Trusted local operators initialize projects, architecture and identities. Agents read and submit through `map-agent` or scoped HTTP. See [protocol](docs/PROTOCOL.md), [security](docs/SECURITY.md), [scope](docs/SCOPE.md), and [provenance](docs/PROVENANCE.md).

Native recovery references checkpoint text for remaining work and next action; it does not implement structured `progress`. Unsupported submission fields are rejected. The map never grants shell, device, scheduling, model, or task execution authority. Recorded status, submitted work, source verification and independent acceptance remain separate facts.
