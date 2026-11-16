# DSH, Codex and Gemini Antigravity
Original author: **EvanBo**. CLI/MCP/Skill companion components are Apache-2.0; retain applicable LICENSE/NOTICE. These integrations do not distribute or modify any platform, scheduler or communications bridge.

## Shared installation

Build/install the Rust binaries with `cargo install --locked --path . --bins`. Use absolute executable/database/token-file paths. The trusted operator provisions a private token file (0600 on Unix). Never place its content in a shared config, Skill or Git repo. One MCP process uses one identity; use a separate named entry for a separately authorized reader or worker when necessary. A manager role does not automatically become a worker submission identity.

Run `sh integrations/install-skill.sh /absolute/project-root` to copy the portable `development-map` Skill into `.agents/skills/`. It refuses an existing destination. It changes only the specified project and does not register a server or overwrite platform configuration. The copied Skill is self-contained with CLI reference and LICENSE/NOTICE. If the project is inside a larger Git repository, use the root the chosen platform actually discovers.

## Codex

The current [official MCP guide](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) describes config.toml stdio servers. Merge `integrations/codex/config.toml.example` into the selected `~/.codex/config.toml` or trusted project's `.codex/config.toml`, replacing all three absolute paths. Do not overwrite existing tables. Alternatively register the same executable and arguments with `codex mcp add evanbo_development_map -- /absolute/path/map-mcp /absolute/path/map.sqlite --token-file /absolute/path/private-agent-token.txt`. Inspect with `codex mcp get evanbo_development_map`.

The [official Skills guide](https://learn.chatgpt.com/docs/build-skills) supports project `.agents/skills`. After the copied Skill is discovered, request `$development-map` to recover the project's map and use the configured IDs. Restart/refresh the MCP connection after registration.

## DSH / DeepSeek Harness

The official [MCP client configuration](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/mcp/mcp-client/README.md) uses a profile composition row. Merge `integrations/dsh/mcp.yaml.example` into the active profile's composition using that installed version's profile loader; it is not an Antigravity JSON config. Replace the paths and start a new isolated DSH session. Tools should appear under the configured server namespace. No private Harness is required or copied.

For DSH headless, copy `integrations/dsh/mcp.patch.yml.example`, replace its absolute paths, and pass it with `dsh headless --patch /absolute/path/to/overlay.yml`. The CLI overlay wraps the composition row in an `insert` operation. A model-backed session also needs the selected DSH provider route configured.

DSH and Codex use the same project Skill. The current request sets the acceptance target. A dependency, gap or historical next action can explain a change, but only the current assignment authorizes the work. Record the result against the bound node.


The official [filesystem Skill provider](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/skill/skill-filesystem/README.md) discovers `.agents/skills` when that provider and the Skill consumer are enabled in the chosen profile. Use the shared installer, then confirm `development-map` appears in that profile's Skill catalog. Custom old profiles may require explicitly enabling their existing Skill provider; copying a Markdown file alone does not prove activation.

## Gemini Antigravity

In the IDE choose MCP Servers → Manage MCP Servers → View raw config; merge `integrations/antigravity/mcp_config.json.example`. The current [official MCP guide](https://antigravity.google/docs/mcp?tab=ide) also documents workspace `.agents/mcp_config.json` and global `~/.gemini/config/mcp_config.json`. Replace paths and refresh the connection. The App/CLI/IDE may expose the manager in different places; use the path displayed by the installed product.

The [official Skill guide](https://antigravity.google/docs/skills?app=antigravity) supports `.agents/skills` in the project; use the shared installer and verify discovery in a new session. This is the Gemini Antigravity route, not a claim that every old Gemini CLI configuration works unchanged.

## Acceptance and fallback

Check tools/list exposes exactly map_resume, map_project and map_submit; initialization identifies `evanbo-development-map`. A reader can read its project and must fail on another project; a worker resumes only its assignment. Submit an isolated real checkpoint, retry exact input, restart the MCP process and resume the same version. Do not run these acceptance writes against a production map without its normal authorization.

DSH headless (DeepSeek API) and Codex CLI each passed two model-backed sessions using the project Skill and stdio MCP. A read-only recovery left tracked files and checkpoints unchanged. A requested fix changed only the required helper, passed the existing test, and advanced only the bound node from v1 to v2. Both left the unrelated historical telemetry task untouched. Antigravity was outside this verification.

Protocol support is explicitly legacy MCP 2025-06-18 over stdio with initialize/initialized negotiation. Clients must allow that revision/fallback. Newer stateless 2026 protocol, HTTP MCP, prompts and resources are not advertised. No model API key is needed by the map itself.
