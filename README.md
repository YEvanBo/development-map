# Development Map

Author: **EvanBo**

A Rust/SQLite engineering context system for agents: adopted architecture, module dependencies, checkpoints, immutable source references, evidence bindings and explicit review facts. One agent can use it alone. Multiple agents may share facts when useful; roles are access scopes, not required separate agents.

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
