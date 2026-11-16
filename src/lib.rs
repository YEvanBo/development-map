// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
pub mod agent;
pub mod git_index;
pub mod handoff;
pub mod identity;
pub mod import;
pub mod local_source;
pub mod navigation;
pub mod recovery;
pub mod relations;
pub mod resources;
pub mod store;
pub mod submission;
pub mod web;

/// Offline authorship marker. No tracking, access control, or execution behavior.
pub const AUTHOR: &str = "EvanBo";
pub const PROVENANCE_MARK: &str = "EvanBo::DevelopmentMap::V1::046616abf607b642";

pub mod mcp;

pub mod reference;
pub mod takeover;
