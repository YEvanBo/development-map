// Copyright (c) 2026 EvanBo. Provenance: EvanBo::DevelopmentMap::V1::046616abf607b642
//! Explicit, revocable project/task/owner bindings. Provisioned by the host's trusted identity adapter.
use crate::store::{Error, MapStore, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Serialize, Deserialize)]
pub struct Binding {
    pub subject: String,
    pub project_id: String,
    pub task_id: String,
    pub owner: String,
    pub role: String,
    pub node_id: Option<String>,
    pub generation: Option<i64>,
    pub source_ref: String,
    pub expires_at: i64,
}
pub fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
impl MapStore {
    pub fn bind_identity(&self, token: &str, b: &Binding) -> Result<()> {
        if token.len() < 24
            || b.subject.is_empty()
            || b.task_id.is_empty()
            || b.owner.is_empty()
            || b.source_ref.is_empty()
            || !matches!(b.role.as_str(), "reader" | "worker" | "manager")
        {
            return Err(Error::Invalid("identity binding"));
        }
        if b.role == "worker" && (b.node_id.is_none() || b.generation.is_none()) {
            return Err(Error::Invalid("worker needs node and generation"));
        }
        if b.role != "worker" && (b.node_id.is_some() || b.generation.is_some()) {
            return Err(Error::Invalid("node scope belongs to worker identity"));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let project: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM map_projects WHERE id=?)",
            [&b.project_id],
            |r| r.get(0),
        )?;
        if !project {
            return Err(Error::Missing);
        }
        if let Some(node) = &b.node_id {
            let matches:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE id=? AND project_id=? AND owner=? AND generation=?)",params![node,b.project_id,b.owner,b.generation],|r|r.get(0))?;
            if !matches {
                return Err(Error::Conflict(
                    "identity does not match current assignment",
                ));
            }
        }
        check_host_task(&tx, b)?;
        // A token cannot silently acquire a different identity. Revoke and provision a new token.
        let encoded = serde_json::to_string(b).map_err(|_| Error::Invalid("binding"))?;
        let old: Option<String> = tx
            .query_row(
                "SELECT binding FROM map_identities WHERE token_hash=?",
                [token_hash(token)],
                |r| r.get(0),
            )
            .optional()?;
        if old.as_ref().is_some_and(|v| v != &encoded) {
            return Err(Error::Conflict("token already bound"));
        }
        tx.execute(
            "INSERT OR IGNORE INTO map_identities(token_hash,binding,revoked) VALUES(?,?,0)",
            params![token_hash(token), encoded],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn revoke_identity(&self, token: &str) -> Result<()> {
        self.connect(false)?.execute(
            "UPDATE map_identities SET revoked=1 WHERE token_hash=?",
            [token_hash(token)],
        )?;
        Ok(())
    }
}
pub fn authorize(
    tx: &Transaction<'_>,
    token: &str,
    project: &str,
    node: Option<&str>,
    role: &str,
) -> Result<Binding> {
    let encoded: Option<String> = tx
        .query_row(
            "SELECT binding FROM map_identities WHERE token_hash=? AND revoked=0",
            [token_hash(token)],
            |r| r.get(0),
        )
        .optional()?;
    let b: Binding =
        serde_json::from_str(&encoded.ok_or(Error::Invalid("identity missing or revoked"))?)
            .map_err(|_| Error::Invalid("identity data"))?;
    let now: i64 = tx.query_row("SELECT unixepoch()", [], |r| r.get(0))?;
    if b.project_id != project || b.expires_at <= now || !(role == "reader" || b.role == role) {
        return Err(Error::Invalid("identity scope or expiry"));
    }
    if b.role == "worker" {
        if node != b.node_id.as_deref() {
            return Err(Error::Invalid("task node scope"));
        }
        let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_dev_nodes WHERE id=? AND project_id=? AND owner=? AND generation=?)",params![b.node_id,b.project_id,b.owner,b.generation],|r|r.get(0))?;
        if !current {
            return Err(Error::Conflict("task assignment changed"));
        }
    }
    check_host_task(tx, &b)?;
    Ok(b)
}

fn check_host_task(tx: &Transaction<'_>, b: &Binding) -> Result<()> {
    if b.role != "worker" {
        return Ok(());
    }
    // Legacy host-task validation is explicit opt-in.  Merely having an
    // unrelated table named `tasks` in the database must not change map
    // identity decisions; old hosts create `map_host_compat` after review.
    let compat: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='map_host_compat')",
        [],
        |r| r.get(0),
    )?;
    if !compat {
        return Ok(());
    }
    let host: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tasks')",
        [],
        |r| r.get(0),
    )?;
    if !host {
        return Ok(());
    }
    let columns: i64 = tx.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name IN ('id','node_id')",
        [],
        |r| r.get(0),
    )?;
    if columns != 2 {
        return Err(Error::Conflict("host task table schema unsupported"));
    }
    let matches: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=? AND node_id=?)",
        params![b.task_id, b.node_id],
        |r| r.get(0),
    )?;
    if !matches {
        return Err(Error::Conflict(
            "bound task missing or points to another node",
        ));
    }
    Ok(())
}
