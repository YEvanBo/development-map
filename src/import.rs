// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Read-only source adapters. Imported historical status is provenance, never acceptance.
use crate::store::{event, Error, MapStore, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Serialize, Deserialize)]
pub struct LegacyNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub source_status: String,
    pub source_owner: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct LegacyManifest {
    pub project_id: String,
    pub name: String,
    pub source_ref: String,
    pub nodes: Vec<LegacyNode>,
}
impl MapStore {
    pub fn import_manifest(&self, m: &LegacyManifest) -> Result<bool> {
        if m.project_id.is_empty() || m.source_ref.is_empty() || m.nodes.len() > 100000 {
            return Err(Error::Invalid("import manifest"));
        }
        let payload = serde_json::to_string(m).map_err(|_| Error::Invalid("manifest JSON"))?;
        let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM map_plan_revisions WHERE project_id=? AND revision=?)",
            params![m.project_id, digest],
            |r| r.get(0),
        )?;
        if existing {
            return Ok(false);
        }
        // Import cannot overwrite an adopted project or an unrelated existing node.
        let adopted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM map_architectures WHERE project_id=?)",
            [&m.project_id],
            |r| r.get(0),
        )?;
        if adopted {
            return Err(Error::Conflict(
                "use reviewed plan update for adopted project",
            ));
        }
        tx.execute("INSERT INTO map_projects VALUES(?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET name=excluded.name,source_ref=excluded.source_ref",params![m.project_id,m.name,m.source_ref])?;
        let ids = m
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if ids.len() != m.nodes.len() {
            return Err(Error::Invalid("duplicate imported ID"));
        }
        for n in &m.nodes {
            if n.id.is_empty()
                || n.title.is_empty()
                || n.parent_id.as_deref().is_some_and(|p| !ids.contains(p))
            {
                return Err(Error::Invalid("imported node or parent"));
            }
            let old: Option<(String, String)> = tx
                .query_row(
                    "SELECT project_id,plan_state FROM map_dev_nodes WHERE id=?",
                    [&n.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if old.is_some_and(|(p, s)| p != m.project_id || s != "legacy") {
                return Err(Error::Conflict("import would overwrite managed node"));
            }
            tx.execute("INSERT INTO map_dev_nodes(id,project_id,parent_id,kind,title,goal,sort_key,plan_state,owner,generation,updated_at) VALUES(?,?,NULL,'module',?,'',?,'legacy',NULL,0,unixepoch()) ON CONFLICT(id) DO UPDATE SET title=excluded.title,sort_key=excluded.sort_key",params![n.id,m.project_id,n.title,n.id])?;
        }
        for n in &m.nodes {
            tx.execute(
                "UPDATE map_dev_nodes SET parent_id=? WHERE id=?",
                params![n.parent_id, n.id],
            )?;
        }
        tx.execute(
            "INSERT INTO map_plan_revisions VALUES(?,?,'',?,?,?,unixepoch())",
            params![m.project_id, digest, m.source_ref, digest, payload],
        )?;
        event(&tx, &m.project_id, None, "legacy_import", &digest)?;
        tx.commit()?;
        Ok(true)
    }
}
/// Adapter for the current speaker ledger; preserves source owner/status in immutable manifest only.
pub fn speaker_ledger(bytes: &[u8], source: &str) -> Result<LegacyManifest> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| Error::Invalid("ledger JSON"))?;
    let project = v["project_id"]
        .as_str()
        .ok_or(Error::Invalid("ledger project ID"))?;
    let tasks = v["tasks"]
        .as_array()
        .ok_or(Error::Invalid("ledger tasks"))?;
    let mut nodes = Vec::new();
    for t in tasks {
        let id = t["id"].as_str().ok_or(Error::Invalid("ledger task ID"))?;
        nodes.push(LegacyNode {
            id: format!("{project}/{id}"),
            parent_id: None,
            title: t["title"].as_str().unwrap_or(id).into(),
            source_status: t["status"].as_str().unwrap_or("unknown").into(),
            source_owner: t["assignee"].as_str().map(str::to_string),
        });
    }
    Ok(LegacyManifest {
        project_id: project.into(),
        name: "智能音箱".into(),
        source_ref: source.into(),
        nodes,
    })
}
/// Only reads the YAML node graph; governance text is not executable configuration.
pub fn harness_tree(bytes: &[u8], source: &str) -> Result<LegacyManifest> {
    let v: serde_json::Value =
        serde_yaml::from_slice(bytes).map_err(|_| Error::Invalid("tree YAML"))?;
    let project = v["project"]["id"]
        .as_str()
        .ok_or(Error::Invalid("tree project ID"))?;
    let nodes = v["nodes"]
        .as_array()
        .ok_or(Error::Invalid("tree nodes"))?
        .iter()
        .map(|n| {
            let id = n["id"].as_str().ok_or(Error::Invalid("tree node ID"))?;
            Ok(LegacyNode {
                id: format!("{project}/{id}"),
                parent_id: n["parent"].as_str().map(|p| format!("{project}/{p}")),
                title: n["label"].as_str().unwrap_or(id).into(),
                source_status: n["status"].as_str().unwrap_or("unknown").into(),
                source_owner: n["owner"].as_str().map(str::to_string),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(LegacyManifest {
        project_id: project.into(),
        name: v["project"]["name"].as_str().unwrap_or(project).into(),
        source_ref: source.into(),
        nodes,
    })
}
