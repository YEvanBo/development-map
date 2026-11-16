# Rust-only release boundary

Author: EvanBo. Source baseline: `SCHED-V1-source`, selected `reference/development-map` subtree. The archive's scheduler implementation is a separate codebase; its richer map/handoff documentation does not establish this crate's capabilities.

Included: Rust map library and binaries (`development-map`, `mapctl`, `map-agent`, `map-mcp`, benchmark), SQL schema/migration, Rust integration and regression tests and SQL fixtures, built-in static viewer, small navigation client and tests, minimal standalone Rust example, portable Skill and three platform config examples, dependency lockfile and usage/security/provenance documents.

Excluded: full scheduler, fleet/node/process execution, communications bridge, commercial harness, private model adapters, historical project logs/configuration, live databases/WAL, credentials, build output, `mindmap-ui`'s separate development server and project-specific Python/speaker adapters. Excluded files are retained in the private V1 baseline; their failures are not changed into passes.

This crate has no Cargo path dependency on those excluded components. Legacy host-task validation is explicit opt-in via the `map_host_compat` marker table; an unrelated `tasks` table has no effect. Map writes only append map events. Existing legacy bridge records remain untouched. Owner handoff records map facts without controlling a real agent process; there is no result-event dispatch or scheduler.

Apache-2.0 is prepared for this Rust map and its companion components; applicable LICENSE/NOTICE attribution must remain with redistributions. The review candidate has `publish=false`; it is not a published or fully accepted open-source release.
