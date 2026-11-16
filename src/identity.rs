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
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostScope {
    pub provider: String,
    pub workspace: String,
}

pub(crate) const HOST_CONFLICT: &str = "host task already associated; operator review required";
pub(crate) fn audit_conflict(
    tx: &Transaction,
    b: &Binding,
    scope: Option<&HostScope>,
) -> Result<()> {
    let detail = serde_json::json!({"host_scope":scope,"task_id":b.task_id,"attempted_node":b.node_id,"source_ref":b.source_ref});
    crate::store::event(
        tx,
        &b.project_id,
        None,
        "host_binding_conflict",
        &detail.to_string(),
    )
}

impl MapStore {
    pub fn bind_identity(&self, token: &str, b: &Binding) -> Result<()> {
        self.bind_scoped(token, b, None)
    }
    pub fn bind_scoped(&self, token: &str, b: &Binding, scope: Option<&HostScope>) -> Result<()> {
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let result = Self::bind_tx(&tx, token, b, scope);
        if let Err(Error::Conflict(HOST_CONFLICT)) = result {
            audit_conflict(&tx, b, scope)?;
            tx.commit()?;
            return result;
        }
        result?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn bind_tx(
        tx: &Transaction,
        token: &str,
        b: &Binding,
        scope: Option<&HostScope>,
    ) -> Result<()> {
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
        check_host_task(tx, b)?;
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
        if let Some(scope) = scope {
            crate::store::require_current_schema(tx)?;
            if tx.query_row("SELECT ?<=unixepoch() OR EXISTS(SELECT 1 FROM map_identities WHERE token_hash=? AND revoked<>0)",
                params![b.expires_at,token_hash(token)],|r|r.get::<_,bool>(0))? {
                return Err(Error::Invalid("inactive credential"));
            }
            let node = b
                .node_id
                .as_deref()
                .ok_or(Error::Invalid("worker binding required"))?;
            if scope.provider.trim().is_empty()
                || scope.workspace.trim().is_empty()
                || b.task_id.trim().is_empty()
            {
                return Err(Error::Invalid("stable host scope and task required"));
            }
            let key = params![
                scope.provider,
                scope.workspace,
                b.task_id,
                b.project_id,
                node
            ];
            let created = tx.execute("INSERT INTO map_host_routes(provider,workspace,task_id,project_id,node_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(provider,workspace,task_id) DO NOTHING", key)?;
            let detail =
                serde_json::json!({"scope":scope,"task_id":b.task_id,"source_ref":b.source_ref})
                    .to_string();
            if created == 1 {
                crate::store::event(tx,&b.project_id,Some(node),"host_bound",&detail)?;
            } else if !tx.query_row("SELECT EXISTS(SELECT 1 FROM map_host_routes WHERE provider=?1 AND workspace=?2 AND task_id=?3 AND project_id=?4 AND node_id=?5)",key,|r|r.get::<_,bool>(0))? {
                return Err(Error::Conflict(HOST_CONFLICT));
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO map_identities(token_hash,binding,revoked) VALUES(?,?,0)",
            params![token_hash(token), encoded],
        )?;
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
pub(crate) fn load_binding(tx: &Transaction, token: &str) -> Result<Binding> {
    let row: Option<(String, bool)> = tx
        .query_row(
            "SELECT binding,revoked FROM map_identities WHERE token_hash=?",
            [token_hash(token)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (encoded, revoked) = row.ok_or(Error::Invalid("identity missing"))?;
    if revoked {
        return Err(Error::Invalid("identity revoked"));
    }
    serde_json::from_str(&encoded).map_err(|_| Error::Invalid("identity data"))
}

pub fn authorize(
    tx: &Transaction<'_>,
    token: &str,
    project: &str,
    node: Option<&str>,
    role: &str,
) -> Result<Binding> {
    let b = load_binding(tx, token)?;
    let now: i64 = tx.query_row("SELECT unixepoch()", [], |r| r.get(0))?;
    if b.expires_at <= now {
        return Err(Error::Invalid("identity expired"));
    }
    if b.project_id != project || !(role == "reader" || b.role == role) {
        return Err(Error::Invalid("identity scope"));
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

/// Operator-only lookup. A route is a candidate until the worker credential passes resume.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostTask {
    project_hint: Option<String>,
    pub provider: String,
    pub workspace: String,
    pub task_id: String,
}
impl MapStore {
    pub fn host_target(&self, key: &HostTask) -> Result<Option<(String, String, i64)>> {
        if [&key.provider, &key.workspace, &key.task_id]
            .iter()
            .any(|v| v.trim().is_empty())
        {
            return Err(Error::Invalid("stable host scope and task required"));
        }
        self.read(|tx| {
            let candidate: Option<(String,String,i64)>=tx.query_row(
                "SELECT project_id,node_id,revision FROM map_host_routes WHERE provider=? AND workspace=? AND task_id=?",
                params![key.provider,key.workspace,key.task_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let (Some(hint),Some((project,_,_)))=(&key.project_hint,&candidate) {
                if hint!=project {return Err(Error::Conflict("project hint conflicts with the assigned route; ask the user"));}
            }
            Ok(candidate)
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRepair {
    key: HostTask,
    expected_revision: i64,
    old_project: String,
    old_node: String,
    project: String,
    node: String,
    decision_ref: String,
}

impl MapStore {
    pub fn repair_host(&self, r: &RouteRepair) -> Result<()> {
        if r.key.project_hint.is_some() {
            return Err(Error::Invalid(
                "repair requires explicit old and new projects",
            ));
        }
        if r.decision_ref.trim().is_empty()
            || r.decision_ref.len() > 512
            || (r.old_project == r.project && r.old_node == r.node)
        {
            return Err(Error::Invalid(
                "explicit route correction and decision reference required",
            ));
        }
        let mut db = self.connect(false)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::store::require_current_schema(&tx)?;
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM map_identities WHERE revoked=0
            AND json_extract(binding,'$.expires_at')>unixepoch() AND json_extract(binding,'$.task_id')=?
            AND json_extract(binding,'$.project_id')=? AND json_extract(binding,'$.node_id')=?)",
            params![r.key.task_id,r.old_project,r.old_node],|row|row.get(0))?;
        if active {
            return Err(Error::Conflict(
                "revoke old assignment credentials before repair",
            ));
        }
        let changed=tx.execute("UPDATE map_host_routes SET project_id=?1,node_id=?2,revision=revision+1
            WHERE provider=?3 AND workspace=?4 AND task_id=?5 AND revision=?6 AND project_id=?7 AND node_id=?8
            AND EXISTS(SELECT 1 FROM map_dev_nodes WHERE project_id=?1 AND id=?2)",
            params![r.project,r.node,r.key.provider,r.key.workspace,r.key.task_id,r.expected_revision,r.old_project,r.old_node])?;
        if changed != 1 {
            return Err(Error::Conflict(
                "route or target changed; inspect before repair",
            ));
        }
        // Each project's history mentions only its side of the correction.
        for (project, node, kind) in [
            (&r.old_project, &r.old_node, "host_detached"),
            (&r.project, &r.node, "host_relinked"),
        ] {
            crate::store::event(&tx, project, Some(node), kind, &r.decision_ref)?;
        }
        tx.commit()?;
        Ok(())
    }
}
